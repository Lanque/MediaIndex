# Cloud contract and PostgreSQL boundary

The cloud boundary is versioned in
[`shared/contracts/v1/media_index.schema.json`](../shared/contracts/v1/media_index.schema.json).
It describes projects, content-identified media assets, local file locations,
and cursor-based sync requests/responses. The API package uses a dependency-
free validation layer, while [`api/app.py`](../api/app.py) exposes the first
executable FastAPI boundary for the same contract.

## Local versus cloud state

The desktop SQLite database is a machine-local index. PostgreSQL stores
project-scoped knowledge that can be shared between devices. A SHA-256 content
hash identifies a logical asset in both systems, while a local path is only a
location on one machine and is never a cloud authorization key.

## Authorization boundary

The initial migration enables row-level security on every project-scoped table.
Authenticated API code must set the transaction-local `app.user_id` before
accessing rows. The policies authorize through the project owner, so a client
cannot select a project ID and use it to bypass ownership checks.

## Sync shape

Each request includes a project ID, client ID, idempotency key, base cursor, and
bounded changes. The idempotency key is stored with the resulting response so
retries can return the same outcome. The cursor is advanced only after the
transaction has durably applied the changes.

## Executable reference API

Run the local reference service with the instructions in
[`api/README.md`](../api/README.md). It currently keeps state in memory so the
HTTP behavior can be tested without external services. The production adapter
must preserve the same contract while replacing that store with PostgreSQL and
setting the authenticated user in the transaction before accessing rows.
