-- Phase 5a (spec 2026-09-26-phase5a-cards-design.md §2–§4, §11): live cards. Runs as the owner (ct_owner).

-- 1. The changed-row cursor analyze reads. The store's episode upsert sets updated_ms on every
--    insert and update. Existing rows get opened_server_ms, so analyze's first start sees them.
--    The NOTIFY trigger is paused for this one-time fill (it would fire once per existing row).
ALTER TABLE episodes ADD COLUMN updated_ms BIGINT NOT NULL DEFAULT 0;
ALTER TABLE episodes DISABLE TRIGGER episodes_notify;
UPDATE episodes SET updated_ms = opened_server_ms;
ALTER TABLE episodes ENABLE TRIGGER episodes_notify;
CREATE INDEX episodes_updated ON episodes (updated_ms);
-- Hour-overlap reads (G3, capital overlap): episodes still open, or closed after an hour starts.
CREATE INDEX episodes_closed ON episodes (closed_server_ms);
-- Edge capture: an account's previous mark before a window, by index, not by scan.
CREATE INDEX paper_marks_account_t ON paper_marks (account_id, t_ms);

-- 2. The premises each run was started under (spec §4). Append-only: the scanner inserts, never
--    updates (host arrives as its own row after the run's first 10 health minutes).
CREATE TABLE run_premises (
    run_id   BIGINT NOT NULL REFERENCES runs (run_id),
    area     TEXT   NOT NULL,
    version  TEXT   NOT NULL,
    PRIMARY KEY (run_id, area)
);
GRANT SELECT, INSERT ON run_premises TO scanner_rw;

-- 3. Per-minute percentiles of the scanner's own CLOB request durations (pm-api times requests it
--    already makes; no new traffic). No new GRANT: the scanner already writes health.
ALTER TABLE health ADD COLUMN rest_rtt_p50_ms INTEGER NULL;
ALTER TABLE health ADD COLUMN rest_rtt_p99_ms INTEGER NULL;

-- 4. Cards (spec §3, §7). JSON holds exact decimals as strings and integer counts, never floats.
--    card_cells.card is the cell family (g1, dur, g4, edge); card_results.card is the card (G1…G4, EDGE).
CREATE TABLE card_cells (
    card         TEXT   NOT NULL,
    premise_key  TEXT   NOT NULL,
    hour_ms      BIGINT NOT NULL,
    data         JSONB  NOT NULL,
    computed_ms  BIGINT NOT NULL,
    PRIMARY KEY (card, premise_key, hour_ms)
);
CREATE TABLE card_results (
    card         TEXT   NOT NULL,
    win          TEXT   NOT NULL CHECK (win IN ('24h', '7d', '30d', 'all', 'today')),
    premise_key  TEXT   NOT NULL,
    status       TEXT   NOT NULL CHECK (status IN ('live', 'warming', 'thin', 'under_review')),
    body         JSONB  NOT NULL,
    computed_ms  BIGINT NOT NULL,
    PRIMARY KEY (card, win)
);
CREATE TABLE card_meta (
    key    TEXT  PRIMARY KEY,
    value  JSONB NOT NULL
);

-- 5. analyze_rw (spec §2): reads the scanner's tables, writes only the card tables. A migration
--    can't carry a secret, so the role is NOLOGIN here; the runbook sets LOGIN PASSWORD once, and
--    fresh volumes get it from docker/postgres/init/01-roles.sh. Roles are cluster-wide: tolerate a
--    role that 01-roles.sh or another database's migration already created.
DO $$ BEGIN
    CREATE ROLE analyze_rw NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;
-- Per database, so parallel test databases never update the same pg_db_role_setting row.
DO $$ BEGIN
    EXECUTE format('ALTER ROLE analyze_rw IN DATABASE %I SET statement_timeout = %L',
                   current_database(), '10s');
END $$;
GRANT USAGE ON SCHEMA public TO analyze_rw;
GRANT SELECT ON episodes, runs, run_premises, health, paper_marks, paper_accounts TO analyze_rw;
GRANT SELECT, INSERT, UPDATE, DELETE ON card_cells, card_results, card_meta TO analyze_rw;
-- The dashboard api (analyze_ro) reads results only. 0001's default privileges already give it
-- SELECT on every new table; spelled out for the two it uses.
GRANT SELECT ON card_results, card_meta TO analyze_ro;

-- 6. Backfill (spec §3, §11; list approved 2026-09-26): live scanner runs from run_id 7 (soak run 3,
--    image 2c5b84d) on, which ran today's episode, censoring, detector, fee and depth logic. Runs
--    1–6 and every replay, safe, census and breaker_reset run are never backfilled. sim and host are
--    not backfilled: those runs measured no REST round trip, so Edge capture starts at the deploy.
--    version = code || '+' || sha8(section), from the literal sections below, which
--    bins/scanner/tests/premises.rs pins to config/scanner.toml and arb_core::premises.
INSERT INTO run_premises (run_id, area, version)
SELECT r.run_id, p.area, p.code || '+' || left(encode(sha256(convert_to(p.section, 'UTF8')), 'hex'), 8)
FROM runs r
CROSS JOIN (VALUES
-- backfill:begin
    ('censoring', '1', 'gap_ms=30000'),
    ('depth', '1', ''),
    ('detector_no', '1', ''),
    ('detector_yes', '1', ''),
    ('episode_def', '1', 'stale_lag_ms=500;suspect_window_ms=60000;l1_suspect_window_ms=3600000;drift_settle_ms=2000'),
    ('fees', '1', '')
-- backfill:end
) AS p (area, code, section)
WHERE r.run_id >= 7 AND r.service = 'scanner' AND r.mode = 'live'
ON CONFLICT DO NOTHING;
