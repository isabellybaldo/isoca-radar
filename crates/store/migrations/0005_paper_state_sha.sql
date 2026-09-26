-- S3c fix round 2 (ruling 26): paper_state records the policies sha256 it was saved under, so a
-- restore can tell a genuine restart (same policies) from a policies.toml edit anywhere (scenario
-- net_ms/match_ms/burst, [p1] thresholds, min_order_shares, settle_delay_ms, markout offsets) that
-- would otherwise restore stale cash/positions/P&L under the new sha because only the per-account
-- AccountSpec was ever compared. NULL for rows saved before this column existed: a NULL always
-- reads as a mismatch, never as a match.
-- No new GRANT needed: this alters a table 0004 already granted to scanner_rw and analyze_ro.
ALTER TABLE paper_state ADD COLUMN policies_sha256 TEXT NULL;
