CREATE TABLE sync_enrollment (
    id INTEGER PRIMARY KEY CHECK(id=1),
    server TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    person_id TEXT NOT NULL,
    boundary INTEGER NOT NULL CHECK(boundary >= 0)
);
CREATE TABLE sync_project_mappings (
    local_project_id INTEGER PRIMARY KEY REFERENCES projects(id),
    remote_project_id TEXT NOT NULL
);
