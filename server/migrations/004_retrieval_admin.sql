CREATE TABLE api_page_cursors (
    cursor_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    kind text NOT NULL,
    filters jsonb NOT NULL,
    position jsonb NOT NULL,
    expires_at timestamptz
);
CREATE INDEX api_page_cursors_expiry ON api_page_cursors(expires_at);
CREATE TABLE api_entry_clocks (
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 0,
    PRIMARY KEY(tenant_id, person_id)
);
CREATE TABLE api_entry_changes (
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    revision bigint NOT NULL,
    entry_id uuid NOT NULL,
    version bigint NOT NULL,
    deleted boolean NOT NULL,
    data jsonb NOT NULL,
    PRIMARY KEY(tenant_id, person_id, revision)
);
INSERT INTO api_entry_changes
SELECT tenant_id,person_id,row_number() OVER (PARTITION BY tenant_id,person_id ORDER BY entry_id),entry_id,version,deleted,data FROM api_entries;
INSERT INTO api_entry_clocks SELECT tenant_id,person_id,max(revision) FROM api_entry_changes GROUP BY tenant_id,person_id;
ALTER TABLE api_projects ADD COLUMN version bigint NOT NULL DEFAULT 1 CHECK(version > 0);
CREATE TABLE api_project_audit (
    audit_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    project_id uuid NOT NULL,
    operation_id uuid NOT NULL,
    data jsonb NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now()
);
