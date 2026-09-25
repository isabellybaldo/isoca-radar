-- F2: capture drops, writer drops, and capture-budget deletions are now surfaced as integrity
-- events (see bins/scanner health task), so the CHECK constraint must allow their kinds too.
ALTER TABLE integrity_events DROP CONSTRAINT IF EXISTS integrity_events_kind_check;
ALTER TABLE integrity_events ADD CONSTRAINT integrity_events_kind_check CHECK (kind IN (
    'crossed_book', 'mirror_mismatch', 'top_mismatch', 'drift',
    'drop', 'reconnect_gap', 'parse_error', 'shard_parked',
    'capture_drop', 'writer_drop', 'capture_budget_delete'
));
