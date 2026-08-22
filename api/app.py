"""Executable FastAPI boundary for the versioned cloud sync contract.

The default runtime deliberately uses the deterministic in-memory sync server.
It keeps the HTTP behavior executable and testable while PostgreSQL and the
authentication provider are introduced behind the same service boundary.
"""

from __future__ import annotations

from datetime import datetime
from typing import Annotated, Literal
from uuid import UUID, uuid4

from fastapi import FastAPI, Header, HTTPException, status
from pydantic import BaseModel, ConfigDict, Field

from api.sync import (
    AuthorizationError,
    InMemorySyncServer,
    ProjectNotFoundError,
    StaleCursorError,
    SyncConflictError,
)


class ContractModel(BaseModel):
    model_config = ConfigDict(extra="forbid")


class MediaMetadataPayload(ContractModel):
    duration_ms: int | None = Field(default=None, ge=0)
    size_bytes: int | None = Field(default=None, ge=0)
    container: str | None = None
    video_codec: str | None = None
    audio_codec: str | None = None
    width: int | None = Field(default=None, ge=1)
    height: int | None = Field(default=None, ge=1)
    frame_rate: str | None = None
    start_time: str | None = None
    creation_time: str | None = None


class MediaAssetPayload(ContractModel):
    content_hash: str = Field(pattern=r"^[a-f0-9]{64}$")
    size_bytes: int = Field(ge=0)
    metadata: MediaMetadataPayload


class LocalFilePayload(ContractModel):
    path: str = Field(min_length=1)
    status: Literal["ACTIVE", "MISSING"]
    observed_at: datetime


class SyncChangePayload(ContractModel):
    operation: Literal["UPSERT", "DELETE"]
    asset: MediaAssetPayload
    local_file: LocalFilePayload


class SyncRequestPayload(ContractModel):
    project_id: UUID
    client_id: str = Field(min_length=1, max_length=128)
    idempotency_key: str = Field(min_length=1, max_length=200)
    base_cursor: int = Field(ge=0)
    changes: list[SyncChangePayload] = Field(max_length=1000)


class SyncResponsePayload(ContractModel):
    next_cursor: int = Field(ge=0)
    accepted_changes: int = Field(ge=0)
    conflicts: list[dict[str, object]]


class ProjectCreatePayload(ContractModel):
    name: str = Field(min_length=1, max_length=200)


class ProjectPayload(ContractModel):
    id: UUID
    name: str
    owner_user_id: str


app = FastAPI(title="MediaIndex API", version="1.0.0")
_sync_server = InMemorySyncServer()
_project_names: dict[str, str] = {}


def reset_runtime() -> None:
    """Reset the reference runtime for isolated tests and local development."""

    global _sync_server, _project_names
    _sync_server = InMemorySyncServer()
    _project_names = {}


UserId = Annotated[str, Header(alias="X-User-Id", min_length=1, max_length=200)]


@app.get("/v1/health")
def health() -> dict[str, str]:
    return {"status": "ok", "contract_version": "v1"}


@app.post(
    "/v1/projects",
    response_model=ProjectPayload,
    status_code=status.HTTP_201_CREATED,
)
def create_project(payload: ProjectCreatePayload, user_id: UserId) -> ProjectPayload:
    project_id = uuid4()
    project_id_text = str(project_id)
    _sync_server.create_project(project_id_text, user_id)
    _project_names[project_id_text] = payload.name
    return ProjectPayload(id=project_id, name=payload.name, owner_user_id=user_id)


@app.post(
    "/v1/projects/{project_id}/sync",
    response_model=SyncResponsePayload,
)
def sync_project(
    project_id: UUID,
    payload: SyncRequestPayload,
    user_id: UserId,
) -> SyncResponsePayload:
    if payload.project_id != project_id:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
            detail="payload project_id does not match the URL project_id",
        )

    try:
        response = _sync_server.sync(payload.model_dump(mode="json"), user_id)
    except ProjectNotFoundError as error:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail=str(error)) from error
    except AuthorizationError as error:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail=str(error)) from error
    except StaleCursorError as error:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail={
                "code": "STALE_CURSOR",
                "expected_cursor": error.expected,
                "received_cursor": error.received,
            },
        ) from error
    except SyncConflictError as error:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail={"code": "SYNC_CONFLICT", "message": str(error)},
        ) from error

    return SyncResponsePayload.model_validate(response)
