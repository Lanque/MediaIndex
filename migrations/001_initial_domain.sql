BEGIN;

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id TEXT NOT NULL,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE media_assets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    content_hash CHAR(64) NOT NULL CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, content_hash)
);

CREATE TABLE local_files (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    media_asset_id UUID NOT NULL REFERENCES media_assets(id) ON DELETE CASCADE,
    path TEXT NOT NULL CHECK (char_length(path) > 0),
    status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'MISSING')),
    observed_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, path)
);

CREATE TABLE sync_cursors (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    client_id TEXT NOT NULL CHECK (char_length(client_id) BETWEEN 1 AND 128),
    cursor BIGINT NOT NULL DEFAULT 0 CHECK (cursor >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, client_id)
);

CREATE TABLE sync_requests (
    idempotency_key TEXT PRIMARY KEY CHECK (char_length(idempotency_key) BETWEEN 1 AND 200),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    client_id TEXT NOT NULL,
    response JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX media_assets_project_hash_idx ON media_assets(project_id, content_hash);
CREATE INDEX local_files_project_status_idx ON local_files(project_id, status);
CREATE INDEX sync_requests_project_client_idx ON sync_requests(project_id, client_id);

ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE media_assets ENABLE ROW LEVEL SECURITY;
ALTER TABLE local_files ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_cursors ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_requests ENABLE ROW LEVEL SECURITY;

CREATE POLICY projects_owner_access ON projects
    USING (owner_user_id = current_setting('app.user_id', true));

CREATE POLICY media_assets_project_access ON media_assets
    USING (EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = media_assets.project_id
          AND projects.owner_user_id = current_setting('app.user_id', true)
    ));

CREATE POLICY local_files_project_access ON local_files
    USING (EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = local_files.project_id
          AND projects.owner_user_id = current_setting('app.user_id', true)
    ));

CREATE POLICY sync_cursors_project_access ON sync_cursors
    USING (EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = sync_cursors.project_id
          AND projects.owner_user_id = current_setting('app.user_id', true)
    ));

CREATE POLICY sync_requests_project_access ON sync_requests
    USING (EXISTS (
        SELECT 1 FROM projects
        WHERE projects.id = sync_requests.project_id
          AND projects.owner_user_id = current_setting('app.user_id', true)
    ));

COMMIT;
