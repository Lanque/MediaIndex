# MediaIndex API

The API package exposes the first executable HTTP boundary for the versioned
cloud sync contract. The default local runtime uses the in-memory sync server;
it is a reference implementation for cursor, idempotency, and ownership
behavior before a PostgreSQL adapter is enabled.

## Run locally

From the repository root:

```powershell
python -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install -r api\requirements-dev.txt
python -m uvicorn api.app:app --reload
```

The health endpoint is available at `http://127.0.0.1:8000/v1/health`.
Interactive OpenAPI documentation is available at
`http://127.0.0.1:8000/docs`.

## Contract boundary

- `X-User-Id` represents the authenticated user at this reference boundary.
- A project is created before a device can submit sync changes.
- `POST /v1/projects/{project_id}/sync` accepts only the v1 payload shape.
- A content hash identifies an asset; a local path is only a location.
- Repeating an idempotency key returns the original response.
- A stale cursor or contradictory batch returns HTTP 409.

The in-memory state is intentionally process-local and is not suitable for
production. The PostgreSQL migration, row-level security policies, and sync
semantics are documented in [cloud-contracts](../docs/cloud-contracts.md) and
[cloud-sync](../docs/cloud-sync.md).
