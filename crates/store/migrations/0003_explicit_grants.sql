-- S3a: least privilege for scanner_rw (spec §10 roles). 0001 granted it INSERT/UPDATE on EVERY
-- table the owner would ever create; from now on a new table (paper_*, api, lab, oracle) is
-- writable by the scanner only if its migration grants that explicitly. SELECT stays default.
-- Default privileges belong to the role running the migrations (the owner), same as in 0001.
ALTER DEFAULT PRIVILEGES IN SCHEMA public REVOKE INSERT, UPDATE ON TABLES FROM scanner_rw;
ALTER DEFAULT PRIVILEGES IN SCHEMA public REVOKE USAGE ON SEQUENCES FROM scanner_rw;

-- The scanner's own tables: exactly what it had, now spelled out per table.
GRANT SELECT, INSERT, UPDATE ON
    runs, universe_snapshots, events, legs, episodes, episode_legs,
    observations, integrity_events, health
TO scanner_rw;
GRANT DELETE ON universe_snapshots, episode_legs TO scanner_rw;
GRANT USAGE, SELECT ON SEQUENCE
    runs_run_id_seq, universe_snapshots_id_seq, observations_id_seq, integrity_events_id_seq
TO scanner_rw;

-- S3a: a unit (NegRisk event) above the Phase 0 per-connection limit is not subscribed and is
-- reported with this kind (bins/scanner shards::MAX_UNIT_TOKENS).
ALTER TABLE integrity_events DROP CONSTRAINT IF EXISTS integrity_events_kind_check;
ALTER TABLE integrity_events ADD CONSTRAINT integrity_events_kind_check CHECK (kind IN (
    'crossed_book', 'mirror_mismatch', 'top_mismatch', 'drift',
    'drop', 'reconnect_gap', 'parse_error', 'shard_parked',
    'capture_drop', 'writer_drop', 'capture_budget_delete',
    'unit_oversize'
));
