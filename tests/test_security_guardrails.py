"""Tests for security, input, and observability guardrails."""

import tempfile
import unittest
from pathlib import Path

from security.guardrails import (
    AuthorizationDenied,
    SignedUrlPolicy,
    SignedUrlPolicyError,
    UntrustedPathError,
    authorize_project,
    structured_event,
    validate_media_path,
)


class SecurityGuardrailTests(unittest.TestCase):
    def test_media_path_stays_inside_root_and_enforces_type_and_size(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            media = root / "clip.MP4"
            media.write_bytes(b"small")
            self.assertEqual(validate_media_path(media, root), media.resolve())

            outside = root.parent / "outside.mp4"
            outside.write_bytes(b"outside")
            try:
                with self.assertRaises(UntrustedPathError):
                    validate_media_path(outside, root)
            finally:
                outside.unlink()

            bad_type = root / "notes.txt"
            bad_type.write_text("not media", encoding="utf-8")
            with self.assertRaises(UntrustedPathError):
                validate_media_path(bad_type, root)

            with self.assertRaises(UntrustedPathError):
                validate_media_path(media, root, max_bytes=2)

    def test_project_authorization_is_explicit(self) -> None:
        authorize_project("user-1", "user-1")
        with self.assertRaises(AuthorizationDenied):
            authorize_project("user-2", "user-1")

    def test_signed_url_ttl_has_a_short_lived_upper_bound(self) -> None:
        policy = SignedUrlPolicy(max_ttl_seconds=900)
        self.assertEqual(policy.validate_ttl(60), 60)
        with self.assertRaises(SignedUrlPolicyError):
            policy.validate_ttl(901)

    def test_structured_events_keep_trace_ids_and_redact_secrets(self) -> None:
        event = structured_event(
            "sync_failed",
            operation_id="op-1",
            job_id="job-1",
            access_token="do-not-log",
            retryable=True,
        )
        self.assertEqual(event["operation_id"], "op-1")
        self.assertEqual(event["job_id"], "job-1")
        self.assertEqual(event["access_token"], "[REDACTED]")
        self.assertTrue(event["retryable"])


if __name__ == "__main__":
    unittest.main()
