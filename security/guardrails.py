"""Dependency-free input, authorization, URL, and logging guardrails."""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


class SecurityError(ValueError):
    """Base class for rejected untrusted input or authorization."""


class UntrustedPathError(SecurityError):
    pass


class AuthorizationDenied(SecurityError):
    pass


class SignedUrlPolicyError(SecurityError):
    pass


def validate_media_path(
    path: str | Path,
    allowed_root: str | Path,
    *,
    allowed_extensions: Iterable[str] = (".mp4", ".mov", ".mkv", ".avi", ".mxf", ".webm"),
    max_bytes: int = 50 * 1024 * 1024 * 1024,
) -> Path:
    """Resolve and validate a local input before metadata or processing work."""

    root = Path(allowed_root)
    candidate = Path(path)
    try:
        resolved_root = root.resolve(strict=True)
        resolved = candidate.resolve(strict=True)
    except (FileNotFoundError, OSError) as error:
        raise UntrustedPathError(f"media path cannot be resolved: {path}") from error

    if not resolved.is_file() or resolved_root not in resolved.parents:
        raise UntrustedPathError("media path must be a regular file inside the selected root")
    if candidate.is_symlink():
        raise UntrustedPathError("symbolic links are not accepted as media inputs")

    extensions = {extension.lower() if extension.startswith(".") else f".{extension.lower()}" for extension in allowed_extensions}
    if resolved.suffix.lower() not in extensions:
        raise UntrustedPathError(f"unsupported media extension: {resolved.suffix or '(none)'}")
    size = resolved.stat().st_size
    if size > max_bytes:
        raise UntrustedPathError(f"media file exceeds the {max_bytes}-byte limit")
    return resolved


def authorize_project(user_id: str, owner_user_id: str) -> None:
    if not user_id or not owner_user_id or user_id != owner_user_id:
        raise AuthorizationDenied("authenticated user cannot access this project")


@dataclass(frozen=True)
class SignedUrlPolicy:
    max_ttl_seconds: int = 900

    def validate_ttl(self, ttl_seconds: int) -> int:
        if not isinstance(ttl_seconds, int) or isinstance(ttl_seconds, bool):
            raise SignedUrlPolicyError("signed URL TTL must be an integer")
        if ttl_seconds < 1 or ttl_seconds > self.max_ttl_seconds:
            raise SignedUrlPolicyError(
                f"signed URL TTL must be between 1 and {self.max_ttl_seconds} seconds"
            )
        return ttl_seconds


def structured_event(
    event: str,
    *,
    operation_id: str,
    job_id: str | None = None,
    logger: logging.Logger | None = None,
    **fields: Any,
) -> dict[str, Any]:
    """Create and optionally emit a structured event without secret fields."""

    record: dict[str, Any] = {
        "event": event,
        "operation_id": operation_id,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }
    if job_id:
        record["job_id"] = job_id
    for key, value in fields.items():
        if any(marker in key.lower() for marker in ("secret", "token", "password", "credential")):
            record[key] = "[REDACTED]"
        else:
            record[key] = value
    if logger is not None:
        logger.info(json.dumps(record, sort_keys=True))
    return record
