"""Contract tests for the versioned cloud sync payload."""

import copy
import unittest
from uuid import uuid4

from api.contracts import load_v1_contract, validate_v1_sync_request


def valid_request() -> dict:
    return {
        "project_id": str(uuid4()),
        "client_id": "desktop-laptop",
        "idempotency_key": "scan-2026-08-21-001",
        "base_cursor": 12,
        "changes": [
            {
                "operation": "UPSERT",
                "asset": {
                    "content_hash": "a" * 64,
                    "size_bytes": 1048576,
                    "metadata": {"container": "mp4", "width": 1920},
                },
                "local_file": {
                    "path": "D:/Footage/scene-a.mp4",
                    "status": "ACTIVE",
                    "observed_at": "2026-08-21T10:15:00Z",
                },
            }
        ],
    }


class CloudContractTests(unittest.TestCase):
    def test_v1_contract_is_versioned_and_has_sync_definitions(self) -> None:
        contract = load_v1_contract()
        self.assertIn("/v1/", contract["$id"])
        self.assertIn("SyncRequest", contract["$defs"])
        self.assertIn("SyncResponse", contract["$defs"])

    def test_valid_sync_request_is_accepted(self) -> None:
        self.assertEqual(validate_v1_sync_request(valid_request()), [])

    def test_invalid_payload_reports_object_level_contract_errors(self) -> None:
        payload = copy.deepcopy(valid_request())
        payload["project_id"] = "not-a-uuid"
        payload["changes"][0]["asset"]["content_hash"] = "not-a-hash"
        payload["changes"][0]["local_file"]["status"] = "UNKNOWN"

        errors = validate_v1_sync_request(payload)
        self.assertTrue(any("project_id" in error for error in errors))
        self.assertTrue(any("content_hash" in error for error in errors))
        self.assertTrue(any("status" in error for error in errors))

    def test_unknown_fields_are_rejected(self) -> None:
        payload = valid_request()
        payload["owner_user_id"] = "user-1"
        errors = validate_v1_sync_request(payload)
        self.assertIn("request.owner_user_id is not allowed", errors)


if __name__ == "__main__":
    unittest.main()
