CREATE TABLE api_projects (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(name) > 0),
    colour text NOT NULL CHECK (colour ~ '^#[0-9a-fA-F]{6}$'),
    archived boolean NOT NULL DEFAULT false,
    PRIMARY KEY (tenant_id, project_id)
);
CREATE TABLE api_project_members (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    person_id uuid NOT NULL,
    PRIMARY KEY (tenant_id, person_id, project_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES api_projects(tenant_id, project_id) ON DELETE CASCADE
);
CREATE TABLE api_project_cursors (
    cursor_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    after_id uuid NOT NULL,
    page_size bigint NOT NULL CHECK (page_size BETWEEN 1 AND 100),
    expires_at timestamptz NOT NULL DEFAULT (now() + interval '15 minutes')
);
CREATE INDEX api_project_cursors_expiry ON api_project_cursors(expires_at);
