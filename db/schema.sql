-- sqlgate persistence schema — source of truth.
-- Apply with: make db-migrate (plain psql -f, no migration framework).

-- Query requests — the core entity.
CREATE TABLE IF NOT EXISTS requests (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    query_text      TEXT NOT NULL,
    query_hash      TEXT NOT NULL UNIQUE,
    target_kind     TEXT NOT NULL CHECK (target_kind IN ('postgres', 'mysql')),
    target_db       TEXT NOT NULL,
    target_topology TEXT NOT NULL DEFAULT 'primary' CHECK (target_topology IN ('primary', 'replica')),
    requester_email TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'submitted'
                    CHECK (status IN ('submitted', 'previewed', 'pending_approval',
                                      'approved', 'rejected', 'executed', 'expired')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Preview results — stored once per request when the preview engine runs.
CREATE TABLE IF NOT EXISTS previews (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id   UUID NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    preview_json JSONB NOT NULL,
    row_count    INT NOT NULL,
    duration_ms  INT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Approval decisions — one row per approval action.
CREATE TABLE IF NOT EXISTS approvals (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id     UUID NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    approver_email TEXT NOT NULL,
    decision       TEXT NOT NULL CHECK (decision IN ('approved', 'rejected')),
    expires_at     TIMESTAMPTZ,  -- NULL for rejections
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Execution records — one row per successful or failed execution attempt.
CREATE TABLE IF NOT EXISTS executions (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id           UUID NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    executed_query_hash  TEXT NOT NULL,
    hash_matched         BOOLEAN NOT NULL,
    rows_affected        INT,
    error_message        TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Audit log — append-only, no update/delete path anywhere in the codebase.
CREATE TABLE IF NOT EXISTS audit_log (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id  UUID REFERENCES requests(id) ON DELETE SET NULL,
    event_type  TEXT NOT NULL,
    actor_email TEXT NOT NULL,
    details     JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Indexes for common queries.
CREATE INDEX IF NOT EXISTS idx_requests_status ON requests(status);
CREATE INDEX IF NOT EXISTS idx_requests_requester ON requests(requester_email);
CREATE INDEX IF NOT EXISTS idx_approvals_request ON approvals(request_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_request ON audit_log(request_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at);
