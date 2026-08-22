"""Dependency-free validation for the cloud sync contract.

The JSON Schema is the language-neutral source of truth. This small validator
keeps the initial API checks runnable in the repository without requiring a
Python web framework or a third-party schema package.
"""

from __future__ import annotations

from typing import Any
from uuid import UUID


def validate_sync_request(payload: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(payload, dict):
        return ["request must be an object"]

    required = {"project_id", "client_id", "idempotency_key", "base_cursor", "changes"}
    errors.extend(_missing(payload, required, "request"))
    errors.extend(_unknown(payload, required, "request"))

    if "project_id" in payload and not _uuid(payload["project_id"]):
        errors.append("project_id must be a UUID string")
    errors.extend(_bounded_text(payload, "client_id", 128))
    errors.extend(_bounded_text(payload, "idempotency_key", 200))

    cursor = payload.get("base_cursor")
    if not _integer(cursor) or cursor < 0:
        errors.append("base_cursor must be a non-negative integer")

    changes = payload.get("changes")
    if not isinstance(changes, list):
        errors.append("changes must be an array")
    elif len(changes) > 1000:
        errors.append("changes must contain at most 1000 items")
    else:
        for index, change in enumerate(changes):
            errors.extend(_validate_change(change, index))

    return errors


def require_valid_sync_request(payload: Any) -> None:
    errors = validate_sync_request(payload)
    if errors:
        raise ValueError("invalid sync request: " + "; ".join(errors))


def _validate_change(change: Any, index: int) -> list[str]:
    prefix = f"changes[{index}]"
    if not isinstance(change, dict):
        return [f"{prefix} must be an object"]

    required = {"operation", "asset", "local_file"}
    errors = _missing(change, required, prefix)
    errors.extend(_unknown(change, required, prefix))
    if change.get("operation") not in {"UPSERT", "DELETE"}:
        errors.append(f"{prefix}.operation must be UPSERT or DELETE")

    asset = change.get("asset")
    if not isinstance(asset, dict):
        errors.append(f"{prefix}.asset must be an object")
    else:
        asset_required = {"content_hash", "size_bytes", "metadata"}
        errors.extend(_missing(asset, asset_required, f"{prefix}.asset"))
        if not isinstance(asset.get("content_hash"), str) or len(asset.get("content_hash", "")) != 64:
            errors.append(f"{prefix}.asset.content_hash must be a 64-character SHA-256 hex string")
        elif any(character not in "0123456789abcdef" for character in asset["content_hash"]):
            errors.append(f"{prefix}.asset.content_hash must contain lowercase hexadecimal characters")
        if not _integer(asset.get("size_bytes")) or asset["size_bytes"] < 0:
            errors.append(f"{prefix}.asset.size_bytes must be a non-negative integer")
        if not isinstance(asset.get("metadata"), dict):
            errors.append(f"{prefix}.asset.metadata must be an object")

    local_file = change.get("local_file")
    if not isinstance(local_file, dict):
        errors.append(f"{prefix}.local_file must be an object")
    else:
        local_required = {"path", "status", "observed_at"}
        errors.extend(_missing(local_file, local_required, f"{prefix}.local_file"))
        if not isinstance(local_file.get("path"), str) or not local_file.get("path", "").strip():
            errors.append(f"{prefix}.local_file.path must be a non-empty string")
        if local_file.get("status") not in {"ACTIVE", "MISSING"}:
            errors.append(f"{prefix}.local_file.status must be ACTIVE or MISSING")
        if not isinstance(local_file.get("observed_at"), str) or "T" not in local_file.get("observed_at", ""):
            errors.append(f"{prefix}.local_file.observed_at must be an ISO date-time string")

    return errors


def _missing(payload: dict[str, Any], required: set[str], prefix: str) -> list[str]:
    return [f"{prefix}.{key} is required" for key in sorted(required - payload.keys())]


def _unknown(payload: dict[str, Any], allowed: set[str], prefix: str) -> list[str]:
    return [f"{prefix}.{key} is not allowed" for key in sorted(payload.keys() - allowed)]


def _bounded_text(payload: dict[str, Any], key: str, maximum: int) -> list[str]:
    value = payload.get(key)
    if not isinstance(value, str) or not value.strip() or len(value) > maximum:
        return [f"{key} must be a non-empty string of at most {maximum} characters"]
    return []


def _integer(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _uuid(value: Any) -> bool:
    if not isinstance(value, str):
        return False
    try:
        UUID(value)
    except ValueError:
        return False
    return True
