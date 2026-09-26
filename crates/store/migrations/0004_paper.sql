-- S3c: paper trading (spec §5.6 accounting, §11 policies). Rows are per run: live runs and replay
-- runs each write under their own run_id, so two replays of one capture compare with EXCEPT.
-- Money, sizes and prices are unconstrained NUMERIC (house rule since S2b: exact, never rounded on
-- insert). Timestamps are BIGINT ms; *_at columns are for humans.
-- Grants are explicit (0003 revoked the scanner's default INSERT/UPDATE and sequence USAGE on
-- every table created after it): the scanner may only APPEND history (ON CONFLICT DO NOTHING needs
-- no UPDATE) and upsert paper_state. No DELETE anywhere. Every key is natural, so there are no
-- sequences to grant. analyze_ro reads everything (its default SELECT still applies; spelled out).

CREATE TABLE paper_accounts (
    run_id           BIGINT  NOT NULL REFERENCES runs (run_id),
    account_id       TEXT    NOT NULL,
    policy           TEXT    NOT NULL CHECK (policy IN ('P0-converter', 'P1-settlement', 'P2-combo', 'P3-naive')),
    scenario         TEXT    NOT NULL,
    fee_bips         NUMERIC NOT NULL,
    bankroll         NUMERIC NOT NULL,
    per_trade        NUMERIC NOT NULL,
    policies_sha256  TEXT    NOT NULL,
    PRIMARY KEY (run_id, account_id)
);

CREATE TABLE paper_intents (
    run_id        BIGINT NOT NULL,
    account_id    TEXT   NOT NULL,
    episode_id    TEXT   NOT NULL,
    decided_ms    BIGINT NOT NULL,
    exec_ms       BIGINT,
    skip_reason   TEXT,
    q_target      NUMERIC,
    detected_q    NUMERIC,
    detected_pnl  NUMERIC,
    reserve       NUMERIC,
    PRIMARY KEY (run_id, account_id, episode_id),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

CREATE TABLE paper_execs (
    run_id        BIGINT  NOT NULL,
    account_id    TEXT    NOT NULL,
    episode_id    TEXT    NOT NULL,
    exec_ms       BIGINT  NOT NULL,
    q_eff         NUMERIC NOT NULL,
    leg_risk_qty  NUMERIC NOT NULL,
    pnl_at_tx     NUMERIC,
    hit           BOOLEAN NOT NULL,
    PRIMARY KEY (run_id, account_id, episode_id),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

CREATE TABLE paper_fills (
    run_id       BIGINT  NOT NULL,
    account_id   TEXT    NOT NULL,
    episode_id   TEXT    NOT NULL,
    market_id    TEXT    NOT NULL,
    yes_token    TEXT    NOT NULL,
    outcome      TEXT    NOT NULL CHECK (outcome IN ('yes', 'no')),
    side         TEXT    NOT NULL CHECK (side IN ('buy', 'sell')),
    t_ms         BIGINT  NOT NULL,
    limit_price  NUMERIC,
    requested    NUMERIC NOT NULL,
    filled       NUMERIC NOT NULL,
    vwap         NUMERIC,
    fees         NUMERIC NOT NULL,
    levels_used  INTEGER NOT NULL,
    PRIMARY KEY (run_id, account_id, episode_id, market_id, outcome, side),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

CREATE TABLE paper_ledger (
    run_id          BIGINT  NOT NULL,
    account_id      TEXT    NOT NULL,
    seq             BIGINT  NOT NULL,
    t_ms            BIGINT  NOT NULL,
    kind            TEXT    NOT NULL CHECK (kind IN ('buy', 'sell', 'convert', 'settle', 'mark')),
    episode_id      TEXT,
    market_id       TEXT,
    yes_token       TEXT,
    outcome         TEXT CHECK (outcome IN ('yes', 'no')),
    qty             NUMERIC NOT NULL,
    price           NUMERIC,
    cash_delta      NUMERIC NOT NULL,
    fees            NUMERIC NOT NULL,
    realized_delta  NUMERIC NOT NULL,
    cash_after      NUMERIC NOT NULL,
    equity          NUMERIC,
    t_at            TIMESTAMPTZ GENERATED ALWAYS AS (to_timestamp(t_ms::double precision / 1000)) STORED,
    PRIMARY KEY (run_id, account_id, seq),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);
CREATE INDEX paper_ledger_episode ON paper_ledger (episode_id);

CREATE TABLE paper_markouts (
    run_id      BIGINT  NOT NULL,
    account_id  TEXT    NOT NULL,
    episode_id  TEXT    NOT NULL,
    market_id   TEXT    NOT NULL,
    yes_token   TEXT    NOT NULL,
    outcome     TEXT    NOT NULL CHECK (outcome IN ('yes', 'no')),
    fill_vwap   NUMERIC NOT NULL,
    offset_ms   BIGINT  NOT NULL,
    mid         NUMERIC,
    markout     NUMERIC,
    PRIMARY KEY (run_id, account_id, episode_id, market_id, outcome, offset_ms),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

-- One AccountRow per mark (every minute of input time, at resolutions, at close).
CREATE TABLE paper_marks (
    run_id        BIGINT  NOT NULL,
    account_id    TEXT    NOT NULL,
    t_ms          BIGINT  NOT NULL,
    cash          NUMERIC NOT NULL,
    equity        NUMERIC NOT NULL,
    realized      NUMERIC NOT NULL,
    unrealized    NUMERIC NOT NULL,
    fees_paid     NUMERIC NOT NULL,
    locked        NUMERIC NOT NULL,
    max_drawdown  NUMERIC NOT NULL,
    n_positions   BIGINT  NOT NULL,
    t_at          TIMESTAMPTZ GENERATED ALWAYS AS (to_timestamp(t_ms::double precision / 1000)) STORED,
    PRIMARY KEY (run_id, account_id, t_ms),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

-- Live only: the accounts that survive a restart (one serialized sim Account per row, exact JSON).
CREATE TABLE paper_state (
    account_id  TEXT   PRIMARY KEY,
    state       TEXT   NOT NULL,
    updated_ms  BIGINT NOT NULL
);

GRANT SELECT, INSERT ON
    paper_accounts, paper_intents, paper_execs, paper_fills, paper_ledger, paper_markouts, paper_marks
TO scanner_rw;
GRANT SELECT, INSERT, UPDATE ON paper_state TO scanner_rw;
GRANT SELECT ON
    paper_accounts, paper_intents, paper_execs, paper_fills, paper_ledger, paper_markouts,
    paper_marks, paper_state
TO analyze_ro;

-- Q19: a resolution whose payout vector is not {1,0}/{0,1} (UMA 50-50, voided leg) is a flagged risk class.
-- The list is 0003's (which added unit_oversize) plus non_binary_payout.
ALTER TABLE integrity_events DROP CONSTRAINT IF EXISTS integrity_events_kind_check;
ALTER TABLE integrity_events ADD CONSTRAINT integrity_events_kind_check CHECK (kind IN (
    'crossed_book', 'mirror_mismatch', 'top_mismatch', 'drift',
    'drop', 'reconnect_gap', 'parse_error', 'shard_parked',
    'capture_drop', 'writer_drop', 'capture_budget_delete',
    'unit_oversize', 'non_binary_payout'
));
