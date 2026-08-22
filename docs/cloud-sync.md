# Incremental cloud sync

Issue #10 uses a cursor and idempotency key as the durable sync boundary. The
reference implementation lives in [`api/sync.py`](../api/sync.py); it is
transport-agnostic so the retry semantics can be tested without requiring a
running cloud service.

## Request lifecycle

1. The client queues local knowledge changes and remembers its last confirmed
   cursor.
2. It sends a bounded batch with the project ID, client ID, base cursor, and a
   deterministic idempotency key.
3. The server authenticates the user, checks project ownership, rejects stale
   cursors, applies the batch transactionally, and stores the response by
   idempotency key.
4. The client advances its cursor and removes the batch only after receiving a
   response.

If the network fails after the server commits, the client keeps the in-flight
batch and retries the same request. The server returns the cached response
instead of applying the changes twice. If the cursor is stale, the client
keeps the pending batch and surfaces an actionable `StaleCursorError` for
conflict resolution.

Original media bytes and local file paths remain on the desktop. Only
project-scoped knowledge is synchronized, and PostgreSQL row-level security
provides the final authorization boundary.
