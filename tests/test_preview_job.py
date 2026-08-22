"""Tests for the explicit selected-media preview operation."""

import subprocess
import tempfile
import unittest
from pathlib import Path

from security.guardrails import SecurityError
from worker.preview import (
    FfmpegPreviewGenerator,
    PreviewRequest,
    build_preview_handler,
)
from worker.runtime import (
    InMemoryQueue,
    JobState,
    JobStore,
    PermanentJobError,
    Worker,
)


class PreviewJobTests(unittest.TestCase):
    def test_ffmpeg_adapter_is_invoked_and_output_is_recorded(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "clip.mp4"
            output = root / "previews" / "clip.jpg"
            source.write_bytes(b"synthetic media placeholder")
            calls = []

            def fake_runner(args, **_kwargs):
                calls.append(args)
                Path(args[-1]).write_bytes(b"preview")
                return subprocess.CompletedProcess(args, 0, stdout="", stderr="")

            request = PreviewRequest.from_payload(
                {"source_path": str(source), "output_path": str(output), "width": 320},
                root,
            )
            result = FfmpegPreviewGenerator(runner=fake_runner).generate(request)

            self.assertEqual(result.size_bytes, 7)
            self.assertTrue(output.is_file())
            self.assertIn("scale=320:-2", calls[0])

    def test_invalid_input_is_rejected_before_ffmpeg(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "clip.mp4"
            source.write_bytes(b"clip")
            with self.assertRaises(SecurityError):
                PreviewRequest.from_payload(
                    {"source_path": str(source), "output_path": str(root.parent / "escape.jpg")},
                    root,
                )

    def test_ffmpeg_failure_is_actionable_and_worker_records_completion(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "clip.mp4"
            source.write_bytes(b"clip")
            output = root / "clip.png"

            def failing_runner(args, **_kwargs):
                return subprocess.CompletedProcess(args, 1, stdout="", stderr="decoder failed")

            with self.assertRaises(PermanentJobError) as failure:
                FfmpegPreviewGenerator(runner=failing_runner).generate(
                    PreviewRequest.from_payload(
                        {"source_path": str(source), "output_path": str(output)}, root
                    )
                )
            self.assertIn("decoder failed", str(failure.exception))

    def test_preview_is_an_explicit_worker_operation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "clip.mp4"
            output = root / "clip.jpg"
            source.write_bytes(b"clip")

            class FakeGenerator:
                def generate(self, request):
                    request.output_path.write_bytes(b"preview")
                    return type("Result", (), {"output_path": request.output_path, "size_bytes": 7})()

            store = JobStore()
            job = store.create_job(
                "preview-request-1",
                "generate_preview",
                {"source_path": str(source), "output_path": str(output)},
            )
            queue = InMemoryQueue()
            worker = Worker(store, queue, build_preview_handler(FakeGenerator(), root))
            worker.enqueue(job)
            worker.run_once(now=0)

            self.assertEqual(job.state, JobState.COMPLETED)
            self.assertEqual(job.payload["preview_result"]["size_bytes"], 7)
