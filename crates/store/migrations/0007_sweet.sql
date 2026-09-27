-- Phase 5d (spec 2026-09-27-phase5d-sweet-spots-design.md §2, §3): sweet spots. Runs as the owner.

-- 1. Gamma event tags, lower-cased slugs, at most 16, frozen at first sight by the universe upsert
--    (COALESCE on conflict). Nullable: NULL is "not recorded" (events gone before this deploy, or a
--    no-go on tags, spec §3). A catalog-only change: no rewrite, no long lock.
ALTER TABLE events ADD COLUMN IF NOT EXISTS tags TEXT[] NULL;

-- 2. analyze_rw reads each attempt's rows and the episodes' metadata. Still no write outside the
--    card tables (0006's startup check is unchanged). No new index: every paper table's primary key
--    starts with (run_id, account_id, episode_id), and a non-concurrent CREATE INDEX would block
--    the scanner's inserts (spec §2).
GRANT SELECT ON events, episode_legs, paper_intents, paper_execs, paper_ledger, paper_markouts TO analyze_rw;
