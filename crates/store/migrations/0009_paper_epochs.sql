-- Phase 7a (spec 2026-09-29-phase7a-latency-answer-design.md §4.1, §5.1): paper epochs, reset
-- requests and the api's reset role. Runs as the owner (ct_owner). The runbook runs it in the
-- stopped-scanner window: the new paper_ledger index locks inserts on that table while it builds.

-- 1. One reset request per UTC day (the unique utc_day is the whole race story: a second request
--    the same day fails, the api answers 429). The scanner applies requests; the api inserts them.
CREATE TABLE paper_reset_requests (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    requested_ms  BIGINT NOT NULL,
    utc_day       BIGINT GENERATED ALWAYS AS (requested_ms / 86400000) STORED UNIQUE
);

-- 2. Epochs are time boundaries: a paper row belongs to epoch e when its time is in
--    [start_ms(e), start_ms(e + 1)). Epoch 0 is implicit (everything before epoch 1). No key of any
--    paper table changes; account ids stay the same across epochs.
CREATE TABLE paper_epochs (
    epoch            INTEGER PRIMARY KEY CHECK (epoch >= 1),
    start_ms         BIGINT  NOT NULL UNIQUE,
    cause            TEXT    NOT NULL CHECK (cause IN ('policies', 'reset')),
    request_id       BIGINT  UNIQUE REFERENCES paper_reset_requests (id),
    run_id           BIGINT  NOT NULL REFERENCES runs (run_id),
    policies_sha256  TEXT    NOT NULL
);

-- 3. The ladder's per-account sums since an epoch start (spec §7.1).
CREATE INDEX paper_ledger_account_t ON paper_ledger (account_id, t_ms);

-- 4. Grants (explicit since 0003). scanner_rw reads requests and writes epoch rows; the readers
--    read both.
GRANT SELECT ON paper_reset_requests TO scanner_rw;
GRANT SELECT, INSERT ON paper_epochs TO scanner_rw;
GRANT SELECT ON paper_reset_requests, paper_epochs TO analyze_ro, analyze_rw;

-- 5. paper_reset_rw: the api's second pool. NOLOGIN here (a migration can't carry a secret); the
--    runbook sets LOGIN PASSWORD once and fresh volumes get it from 01-roles.sh. Roles are
--    cluster-wide: tolerate one that already exists. INSERT on the identity column needs no
--    sequence grant.
DO $$ BEGIN
    CREATE ROLE paper_reset_rw NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;
DO $$ BEGIN
    EXECUTE format('ALTER ROLE paper_reset_rw IN DATABASE %I SET statement_timeout = %L',
                   current_database(), '5s');
END $$;
GRANT USAGE ON SCHEMA public TO paper_reset_rw;
GRANT SELECT, INSERT ON paper_reset_requests TO paper_reset_rw;
GRANT SELECT ON paper_epochs TO paper_reset_rw;
