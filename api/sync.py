"""Transport-agnostic incremental sync reference implementation.

The in-memory server makes cursor, authorization, and idempotency behavior
observable in tests. A production HTTP adapter can delegate to the same
semantics without changing the client retry contract.
"""

from __future__ import annotations

import copy
import hashlib
import json
from dataclasses import dataclass, field
from typing import Any, Callable

from api.contracts import accept_v1_sync_request


class SyncError(RuntimeError):
    """Base class for actionable sync failures."""


class AuthorizationError(SyncError):
    """The authenticated user does not own the requested project."""


class ProjectNotFoundError(SyncError):
    """The requested project is not available to the sync service."""


class StaleCursorError(SyncError):
    def __init__(self, expected: int, received: int) -> None:
        self.expected = expected
        self.received = received
        super().__init__(f"stale sync cursor: expected {expected}, received {received}")


class SyncConflictError(SyncError):
    """The batch contains contradictory changes that must be resolved."""


@dataclass
class _ProjectState:
    project_id: str
    owner_user_id: str
    cursor: int = 0
    assets: dict[str, dict[str, Any]] = field(default_factory=dict)
    local_files: dict[str, dict[str, Any]] = field(default_factory=dict)
    idempotent_responses: dict[str, dict[str, Any]] = field(default_factory=dict)


class InMemorySyncServer:
    """Small deterministic server model used by contract/integration tests."""

    def __init__(self) -> None:
        self._projects: dict[str, _ProjectState] = {}

    def create_project(self, project_id: str, owner_user_id: str) -> None:
        self._projects[project_id] = _ProjectState(project_id, owner_user_id)

    def sync(self, request: dict[str, Any], user_id: str) -> dict[str, Any]:
        accept_v1_sync_request(request)
        project = self._projects.get(request["project_id"])
        if project is None:
            raise ProjectNotFoundError("project is not available")
        if project.owner_user_id != user_id:
            raise AuthorizationError("authenticated user cannot access this project")

        idempotency_key = request["idempotency_key"]
        cached = project.idempotent_responses.get(idempotency_key)
        if cached is not None:
            return copy.deepcopy(cached)

        if request["base_cursor"] != project.cursor:
            raise StaleCursorError(project.cursor, request["base_cursor"])

        seen_paths: set[str] = set()
        for change in request["changes"]:
            path = change["local_file"]["path"]
            if path in seen_paths:
                raise SyncConflictError(f"batch contains multiple changes for {path}")
            seen_paths.add(path)

        for change in request["changes"]:
            asset = copy.deepcopy(change["asset"])
            local_file = copy.deepcopy(change["local_file"])
            content_hash = asset["content_hash"]
            path = local_file["path"]
            project.assets[content_hash] = asset
            project.local_files[path] = {
                **local_file,
                "content_hash": content_hash,
            }

        if request["changes"]:
            project.cursor += 1
        response = {
            "next_cursor": project.cursor,
            "accepted_changes": len(request["changes"]),
            "conflicts": [],
        }
        project.idempotent_responses[idempotency_key] = copy.deepcopy(response)
        return response

    def snapshot(self, project_id: str, user_id: str) -> dict[str, Any]:
        project = self._projects.get(project_id)
        if project is None:
            raise ProjectNotFoundError("project is not available")
        if project.owner_user_id != user_id:
            raise AuthorizationError("authenticated user cannot access this project")
        return {
            "cursor": project.cursor,
            "assets": copy.deepcopy(project.assets),
            "local_files": copy.deepcopy(project.local_files),
        }

    def get_changes(self, project_id: str, user_id: str, since_cursor: int = 0) -> dict[str, Any]:
        project = self._projects.get(project_id)
        if project is None:
            raise ProjectNotFoundError("project is not available")
        if project.owner_user_id != user_id:
            raise AuthorizationError("authenticated user cannot access this project")
        return {
            "current_cursor": project.cursor,
            "since_cursor": since_cursor,
            "assets": copy.deepcopy(project.assets),
            "local_files": copy.deepcopy(project.local_files),
        }


class SyncClient:
    """Cursor-aware client that keeps an in-flight batch until it is acknowledged."""

    def __init__(self, project_id: str, client_id: str, cursor: int = 0) -> None:
        self.project_id = project_id
        self.client_id = client_id
        self.cursor = cursor
        self.pending_changes: list[dict[str, Any]] = []
        self._in_flight: tuple[dict[str, Any], int] | None = None

    def queue_change(self, change: dict[str, Any]) -> None:
        self.pending_changes.append(copy.deepcopy(change))

    @property
    def pending_count(self) -> int:
        return len(self.pending_changes)

    def sync(
        self,
        transport: Callable[[dict[str, Any], str], dict[str, Any]],
        user_id: str,
        batch_size: int = 100,
    ) -> dict[str, Any]:
        if batch_size < 1:
            raise ValueError("batch_size must be positive")
        total_accepted = 0

        while self.pending_changes:
            if self._in_flight is None:
                batch = copy.deepcopy(self.pending_changes[:batch_size])
                idempotency_key = _batch_id(
                    self.project_id, self.client_id, self.cursor, batch
                )
                request = {
                    "project_id": self.project_id,
                    "client_id": self.client_id,
                    "idempotency_key": idempotency_key,
                    "base_cursor": self.cursor,
                    "changes": batch,
                }
                self._in_flight = (request, len(batch))

            request, batch_count = self._in_flight
            response = transport(request, user_id)
            self.cursor = response["next_cursor"]
            accepted = response["accepted_changes"]
            if accepted != batch_count:
                raise SyncConflictError(
                    f"server acknowledged {accepted} changes for a {batch_count}-change batch"
                )
            del self.pending_changes[:batch_count]
            self._in_flight = None
            total_accepted += accepted

        return {"next_cursor": self.cursor, "accepted_changes": total_accepted}


def _batch_id(
    project_id: str, client_id: str, cursor: int, changes: list[dict[str, Any]]
) -> str:
    payload = json.dumps(
        {"project_id": project_id, "client_id": client_id, "cursor": cursor, "changes": changes},
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()
