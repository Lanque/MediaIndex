"""Explicit selected-media preview generation behind a testable FFmpeg seam."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Protocol, Any

from security.guardrails import SecurityError, validate_media_path
from worker.runtime import Job, PermanentJobError


class PreviewGenerator(Protocol):
    def generate(self, request: "PreviewRequest") -> "PreviewResult":
        ...


@dataclass(frozen=True)
class PreviewRequest:
    source_path: Path
    output_path: Path
    width: int = 640

    @classmethod
    def from_payload(cls, payload: dict[str, Any], allowed_root: str | Path) -> "PreviewRequest":
        source = payload.get("source_path")
        output = payload.get("output_path")
        if not isinstance(source, str) or not isinstance(output, str):
            raise SecurityError("preview requires source_path and output_path")

        source_path = validate_media_path(source, allowed_root)
        output_path = _validate_output_path(output, allowed_root)
        width = payload.get("width", 640)
        if not isinstance(width, int) or isinstance(width, bool) or not 64 <= width <= 4096:
            raise SecurityError("preview width must be an integer between 64 and 4096")
        return cls(source_path=source_path, output_path=output_path, width=width)


@dataclass(frozen=True)
class PreviewResult:
    output_path: Path
    size_bytes: int


CommandRunner = Callable[..., subprocess.CompletedProcess[str]]


@dataclass
class FfmpegPreviewGenerator:
    executable: str | Path = "ffmpeg"
    runner: CommandRunner | None = None

    def generate(self, request: PreviewRequest) -> PreviewResult:
        request.output_path.parent.mkdir(parents=True, exist_ok=True)
        command = [
            str(self.executable),
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(request.source_path),
            "-frames:v",
            "1",
            "-vf",
            f"scale={request.width}:-2",
            str(request.output_path),
        ]
        runner = self.runner or subprocess.run
        try:
            completed = runner(command, capture_output=True, text=True, check=False)
        except FileNotFoundError as error:
            raise PermanentJobError(f"FFmpeg executable is unavailable: {self.executable}") from error
        except OSError as error:
            raise PermanentJobError(f"cannot start FFmpeg: {error}") from error

        if completed.returncode != 0:
            stderr = (completed.stderr or "").strip()
            detail = stderr or f"exit code {completed.returncode}"
            raise PermanentJobError(f"FFmpeg preview failed: {detail}")
        if not request.output_path.is_file() or request.output_path.stat().st_size == 0:
            raise PermanentJobError("FFmpeg completed without producing a non-empty preview")
        return PreviewResult(request.output_path, request.output_path.stat().st_size)


def build_preview_handler(
    generator: PreviewGenerator, allowed_root: str | Path
) -> Callable[[Job], None]:
    """Build a worker handler for explicit preview jobs."""

    def handle(job: Job) -> None:
        try:
            request = PreviewRequest.from_payload(job.payload, allowed_root)
        except SecurityError as error:
            raise PermanentJobError(f"invalid preview input: {error}") from error
        result = generator.generate(request)
        job.payload["preview_result"] = {
            "output_path": str(result.output_path),
            "size_bytes": result.size_bytes,
        }

    return handle


def _validate_output_path(path: str, allowed_root: str | Path) -> Path:
    root = Path(allowed_root).resolve(strict=True)
    candidate = Path(path)
    if candidate.is_symlink():
        raise SecurityError("preview output cannot be a symbolic link")
    resolved = candidate.resolve(strict=False)
    if resolved == root or root not in resolved.parents:
        raise SecurityError("preview output must remain inside the selected root")
    if resolved.suffix.lower() not in {".jpg", ".jpeg", ".png"}:
        raise SecurityError("preview output must be JPG or PNG")
    return resolved
