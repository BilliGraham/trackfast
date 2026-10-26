CREATE TABLE api_settings (
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    version bigint NOT NULL CHECK (version > 0),
    data jsonb NOT NULL,
    PRIMARY KEY (tenant_id, person_id)
);
CREATE TABLE api_settings_audit (
    audit_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    operation_id uuid NOT NULL,
    data jsonb NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now()
);
