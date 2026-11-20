CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    scope TEXT,
    status TEXT NOT NULL,
    trigger TEXT NOT NULL,
    processed BIGINT NOT NULL DEFAULT 0,
    total BIGINT,
    detail JSONB NOT NULL DEFAULT '{}'::jsonb,
    error TEXT,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ
);

CREATE INDEX jobs_kind_scope_started_idx ON jobs (kind, scope, started_at DESC);
CREATE INDEX jobs_started_idx ON jobs (started_at DESC);
CREATE INDEX jobs_running_idx ON jobs (status) WHERE status = 'running';
