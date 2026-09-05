# Local index and content identity

The desktop application keeps a machine-local SQLite index at the Tauri
application data directory as `mediaindex.sqlite3`. The database is a cache of
the local archive, not a replacement for the original media files.

## Schema

The schema is migration-ready. `schema_migrations` records applied versions;
the first migration creates:

- `media_assets`, keyed by the SHA-256 content hash of a logical media asset;
- `local_files`, keyed by path and pointing to a `media_assets` row. It also
  records whether the content identity has been verified;
- indexes for content-hash and active/missing path lookups.

A path is therefore a location, not an identity. Two local copies can point to
one content hash, and moving a file changes the location row without changing
the logical asset.

## Hashing and re-indexing

Media files are hashed during local discovery, before they are written to the
index. SHA-256 is computed with a heap-allocated 1 MiB read buffer so large
files cannot exhaust the Windows process stack. The process never uploads
media bytes.

The stored `content_hash` is a complete SHA-256 identity for every file,
including files larger than 16 MiB. A repeat scan may reuse a verified hash
when the path, file size, and modification timestamp are unchanged. Files
that fail this identity check are hashed again from byte zero. A previous
database migration marked identities created by the old sampled large-file
hasher as unverified; those rows are rehashed before they can be reused for
deduplication or AI analysis.

Unverified rows remain in the index so existing relationships are preserved,
but they are not treated as confirmed content identities: duplicate paths are
not coalesced, old AI annotations are not exposed, and an existing annotation
does not suppress a new analysis. A successful scan writes the complete hash
and marks the row verified. If a read or hash fails, the scanner emits a
warning and the index treats that scan as incomplete. Existing rows are not
marked deleted because a failed hash must not look like a missing file.

Re-indexing the same path and hash is idempotent. A changed hash is recorded as
`MODIFIED`; a new path with an existing hash is recorded as `MOVED` or
`DUPLICATE`, depending on whether the old location is still present. Locations
not seen in a complete scan become `MISSING` history rows rather than being
physically deleted.

Technical metadata is stored on the content identity when supplied by the
FFprobe adapter. FFprobe itself remains a local process and its failures are
returned as structured error state. A repeat scan reuses metadata already
stored for the same content hash, so FFprobe is only invoked for new or changed
content.

## AI annotations

AI annotations are stored separately from deterministic metadata in the
`ai_annotations` table. Each row belongs to a content hash and contains a
timestamp, short visual description, normalized labels, and an embedding. This
keeps a query such as `Fortnite kill` tied to a playable moment rather than
only to a filename.

AI analysis is explicit and samples frames through the local FFmpeg executable.
The application sends those sampled images to the configured AI provider only
after the user presses **Analyze with AI**; ordinary folder scanning never
invokes the AI provider.
