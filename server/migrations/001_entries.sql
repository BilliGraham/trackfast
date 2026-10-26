CREATE TABLE api_entries (
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    entry_id uuid NOT NULL,
    version bigint NOT NULL CHECK (version > 0),
    deleted boolean NOT NULL DEFAULT false,
    data jsonb NOT NULL,
    PRIMARY KEY (tenant_id, person_id, entry_id)
);
CREATE TABLE api_operations (
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    operation_id uuid NOT NULL,
    request jsonb NOT NULL,
    response jsonb,
    PRIMARY KEY (tenant_id, person_id, operation_id)
);
CREATE TABLE api_audit (
    audit_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    entry_id uuid NOT NULL,
    operation_id uuid NOT NULL,
    action text NOT NULL,
    data jsonb NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now()
);
