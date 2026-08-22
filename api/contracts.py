"""Server-side entry point for the versioned shared contract."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from shared.contracts.validation import require_valid_sync_request, validate_sync_request


CONTRACT_PATH = Path(__file__).resolve().parents[1] / "shared/contracts/v1/media_index.schema.json"


def load_v1_contract() -> dict[str, Any]:
    return json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))


def validate_v1_sync_request(payload: Any) -> list[str]:
    return validate_sync_request(payload)


def accept_v1_sync_request(payload: Any) -> None:
    require_valid_sync_request(payload)
