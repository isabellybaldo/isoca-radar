-- Phase 5b (spec 2026-09-27-phase5b-tiered-storage-design.md §4.3, §5, §9): compaction progress,
-- the archive status, and the compactor's role. Runs as the owner (ct_owner).

-- 1. One row per raw hour the compactor finished ('done') or that was deleted before it could
--    ('lost': the scanner at its disk cap, or the compactor passing over a marked hour).
CREATE TABLE compact_hours (
    hour_ms        BIGINT PRIMARY KEY,
    status         TEXT   NOT NULL CHECK (status IN ('done', 'lost')),
    source_file    TEXT   NULL,
    source_bytes   BIGINT NULL,
    source_sha256  TEXT   NULL,
    rows           JSONB  NULL,
    done_ms        BIGINT NOT NULL
);

-- 2. The single archive status row the api shows on Health.
CREATE TABLE compact_status (
    id             SMALLINT PRIMARY KEY CHECK (id = 1),
    updated_ms     BIGINT   NOT NULL,
    archive_bytes  BIGINT   NOT NULL,
    oldest_day     DATE     NULL,
    lag_hours      INTEGER  NOT NULL,
    last_error     TEXT     NULL
);

-- 3. compact_rw: NOLOGIN here (a migration can't carry a secret); the runbook sets LOGIN PASSWORD
--    once and fresh volumes get it from docker/postgres/init/01-roles.sh. Roles are cluster-wide:
--    tolerate one that already exists.
DO $$ BEGIN
    CREATE ROLE compact_rw NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;
DO $$ BEGIN
    EXECUTE format('ALTER ROLE compact_rw IN DATABASE %I SET statement_timeout = %L',
                   current_database(), '10s');
END $$;
GRANT USAGE ON SCHEMA public TO compact_rw;
GRANT SELECT ON episodes, runs TO compact_rw;
-- SELECT too: INSERT … ON CONFLICT DO UPDATE reads the row it updates, and the compactor reads
-- which hours are lost (plan ruling R7).
GRANT SELECT, INSERT, UPDATE ON compact_hours, compact_status TO compact_rw;

-- 4. The scanner's delete guard reads statuses and inserts 'lost' at the hard cap (never UPDATE),
--    and reads compact_status to wait only for a compactor that is alive (rollback, spec §9.5).
GRANT SELECT, INSERT ON compact_hours TO scanner_rw;
GRANT SELECT ON compact_status TO scanner_rw;

-- 5. The dashboard api reads the status row and when the first hour was recorded. 0001's default
--    privileges already give analyze_ro SELECT on every new table; spelled out for the two it uses.
GRANT SELECT ON compact_status, compact_hours TO analyze_ro;
