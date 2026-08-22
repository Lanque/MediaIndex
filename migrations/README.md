# PostgreSQL migrations

Migration files are numbered and applied in ascending order. Each migration
must be transactional, use `ON_ERROR_STOP`, and be safe to run against a
fresh database in a clean environment.

Apply the current schema with:

```bash
psql "$DATABASE_URL" --set ON_ERROR_STOP=1 --file migrations/001_initial_domain.sql
```

The cloud schema is intentionally separate from the desktop SQLite index. A
`media_assets` content hash is the stable logical identity in both systems,
but cloud rows are scoped by `project_id`. `local_files.path` is never used as
global identity or authorization input.

The migration enables PostgreSQL row-level security. The API must set the
transaction-local `app.user_id` after authenticating a request and before
reading or writing project-scoped rows.
