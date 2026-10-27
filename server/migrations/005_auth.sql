CREATE TABLE api_auth_members (
    subject text PRIMARY KEY CHECK (length(subject) BETWEEN 1 AND 255),
    tenant_id uuid NOT NULL,
    person_id uuid NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    administrator boolean NOT NULL DEFAULT false,
    UNIQUE (tenant_id, person_id)
);
CREATE TABLE api_auth_challenges (
    challenge_id uuid PRIMARY KEY,
    nonce text NOT NULL,
    expires_at timestamptz NOT NULL DEFAULT (now() + interval '5 minutes')
);
CREATE TABLE api_auth_sessions (
    session_id uuid PRIMARY KEY,
    subject text NOT NULL REFERENCES api_auth_members(subject),
    access_hash bytea NOT NULL UNIQUE CHECK (octet_length(access_hash)=32),
    access_expires_at timestamptz NOT NULL DEFAULT (now() + interval '15 minutes'),
    expires_at timestamptz NOT NULL DEFAULT (now() + interval '7 days'),
    revoked boolean NOT NULL DEFAULT false
);
CREATE TABLE api_auth_refresh (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash)=32),
    session_id uuid NOT NULL REFERENCES api_auth_sessions(session_id) ON DELETE CASCADE,
    used boolean NOT NULL DEFAULT false
);
CREATE INDEX api_auth_refresh_session ON api_auth_refresh(session_id);
CREATE INDEX api_auth_sessions_subject ON api_auth_sessions(subject);
