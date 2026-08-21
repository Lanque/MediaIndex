# Local index and content identity

The desktop application keeps a machine-local SQLite index at the Tauri
application data directory as `mediaindex.sqlite3`. The database is a cache of
the local archive, not a replacement for the original media files.

## Schema

The schema is migration-ready. `schema_migrations` records applied versions;
the first migration creates:

- `media_assets`, keyed by the SHA-256 content hash of a logical media asset;
- `local_files`, keyed by path and pointing to a `media_assets` row;
- indexes for content-hash and active/missing path lookups.

A path is therefore a location, not an identity. Two local copies can point to
one content hash, and moving a file changes the location row without changing
the logical asset.

## Hashing and re-indexing

Media files are hashed during local discovery, before they are written to the
index. SHA-256 is computed with a 1 MiB read buffer. The process never uploads
media bytes.

The current implementation restarts a file hash from byte zero on the next
scan; it does not persist partial digest state. If a read or hash fails, the
scanner emits a warning and the index treats that scan as incomplete. Existing
rows are not marked deleted because a failed hash must not look like a missing
file.

Re-indexing the same path and hash is idempotent. A changed hash is recorded as
`MODIFIED`; a new path with an existing hash is recorded as `MOVED` or
`DUPLICATE`, depending on whether the old location is still present. Locations
not seen in a complete scan become `MISSING` history rows rather than being
physically deleted.

Technical metadata is stored on the content identity when supplied by the
FFprobe adapter. FFprobe itself remains a local process and its failures are
returned as structured error state.
