-- cute-trading v2 schema (spec §10). Runs as the owner role (ct_owner).
-- Login roles scanner_rw / analyze_ro are created by docker/postgres/init/01-roles.sh.

GRANT USAGE ON SCHEMA public TO scanner_rw, analyze_ro;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE ON TABLES TO scanner_rw;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO scanner_rw;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO analyze_ro;

-- Every long-running service writes a row at start and fills ended_ms/exit_reason at exit (§8.1.2).
-- mode: live | safe | replay | census | breaker_reset
CREATE TABLE runs (
    run_id       BIGSERIAL PRIMARY KEY,
    service      TEXT    NOT NULL,
    started_ms   BIGINT  NOT NULL,
    ended_ms     BIGINT,
    version      TEXT    NOT NULL,
    config_hash  TEXT    NOT NULL,
    mode         TEXT    NOT NULL CHECK (mode IN ('live', 'safe', 'replay', 'census', 'breaker_reset')),
    exit_reason  TEXT,
    started_at   TIMESTAMPTZ GENERATED ALWAYS AS (to_timestamp(started_ms::double precision / 1000)) STORED
);
CREATE INDEX runs_service_started ON runs (service, started_ms);

-- Warm-start cache: the scanner's serialized universe (§8.1.2). Only the latest few are kept.
CREATE TABLE universe_snapshots (
    id        BIGSERIAL PRIMARY KEY,
    taken_ms  BIGINT  NOT NULL,
    n_events  INTEGER NOT NULL,
    body      TEXT    NOT NULL
);
CREATE INDEX universe_snapshots_taken ON universe_snapshots (taken_ms DESC);
GRANT DELETE ON universe_snapshots TO scanner_rw;

CREATE TABLE events (
    event_id            TEXT PRIMARY KEY,
    slug                TEXT,
    title               TEXT,
    neg_risk_market_id  TEXT,
    augmented           BOOLEAN NOT NULL,
    n_legs              INTEGER NOT NULL,
    first_seen_ms       BIGINT  NOT NULL,
    last_seen_ms        BIGINT  NOT NULL,
    gone_ms             BIGINT
);

-- Current state of each leg; fees as seen at capture are also copied onto episode_legs.
CREATE TABLE legs (
    market_id      TEXT PRIMARY KEY,
    event_id       TEXT NOT NULL REFERENCES events (event_id),
    yes_token      TEXT NOT NULL,
    no_token       TEXT NOT NULL,
    status         TEXT NOT NULL,
    fee_rate       NUMERIC(8,6) NOT NULL,
    fee_exponent   SMALLINT     NOT NULL,
    tick           NUMERIC(8,6),
    min_size       NUMERIC,
    seconds_delay  INTEGER,
    end_date       TEXT,
    first_seen_ms  BIGINT NOT NULL,
    last_seen_ms   BIGINT NOT NULL,
    gone_ms        BIGINT
);
CREATE INDEX legs_event ON legs (event_id);

CREATE TABLE episodes (
    episode_id                TEXT PRIMARY KEY,
    run_id                    BIGINT NOT NULL REFERENCES runs (run_id),
    source                    TEXT NOT NULL CHECK (source IN ('live', 'replay')),
    kind                      TEXT NOT NULL,
    realization               TEXT NOT NULL,
    event_id                  TEXT NOT NULL,
    event_slug                TEXT,
    event_title               TEXT,
    neg_risk_market_id        TEXT,
    pair_id                   TEXT,
    category                  TEXT,
    n_outcomes                INTEGER NOT NULL,
    n_legs                    INTEGER NOT NULL,
    legs_dropped              INTEGER NOT NULL,
    augmented                 BOOLEAN NOT NULL,
    incomplete_set            BOOLEAN NOT NULL,
    uncovered_legs            INTEGER NOT NULL,
    opened_server_ms          BIGINT  NOT NULL,
    closed_server_ms          BIGINT,
    duration_ms               BIGINT,
    censor_left               BOOLEAN NOT NULL,
    censor_right              BOOLEAN NOT NULL,
    censor_gap                BOOLEAN NOT NULL,
    detect_lag_ms             BIGINT,
    n_updates                 INTEGER NOT NULL,
    peak_server_ms            BIGINT  NOT NULL,
    peak_edge_top             NUMERIC NOT NULL,
    peak_edge_top_gross       NUMERIC NOT NULL,
    peak_max_size             NUMERIC NOT NULL,
    peak_cost_usd             NUMERIC NOT NULL,
    peak_payout_usd           NUMERIC NOT NULL,
    peak_fees_usd             NUMERIC NOT NULL,
    peak_pnl_net_usd          NUMERIC NOT NULL,
    peak_pnl_gross_usd        NUMERIC NOT NULL,
    peak_profit_per_dollar    NUMERIC,
    pnl_net_budget100_usd     NUMERIC,
    bottleneck_leg_idx        INTEGER,
    bottleneck_notional_usd   NUMERIC,
    below_min_size            BOOLEAN,
    max_seconds_delay         INTEGER,
    days_to_end               NUMERIC,
    annualized_return         NUMERIC,
    post_end                  BOOLEAN,
    engine_suspect            BOOLEAN NOT NULL,
    stale_transport           BOOLEAN NOT NULL,
    open_lag_ms               BIGINT,
    oracle_downgraded         BOOLEAN,
    scanner_version           TEXT NOT NULL,
    config_hash               TEXT NOT NULL,
    opened_at                 TIMESTAMPTZ GENERATED ALWAYS AS (to_timestamp(opened_server_ms::double precision / 1000)) STORED
);
CREATE INDEX episodes_opened ON episodes (opened_server_ms);
CREATE INDEX episodes_event ON episodes (event_id);
CREATE INDEX episodes_run ON episodes (run_id);

-- Per leg at peak. Replaced as a set whenever the episode row is rewritten.
CREATE TABLE episode_legs (
    episode_id    TEXT    NOT NULL REFERENCES episodes (episode_id),
    leg_idx       INTEGER NOT NULL,
    market_id     TEXT    NOT NULL,
    token         TEXT    NOT NULL,
    action        TEXT    NOT NULL CHECK (action IN ('buy_yes', 'buy_no')),
    best_price    NUMERIC(8,6),
    vwap          NUMERIC,
    levels        INTEGER NOT NULL,
    size_at_best  NUMERIC,
    filled        NUMERIC NOT NULL,
    notional      NUMERIC NOT NULL,
    fees          NUMERIC NOT NULL,
    in_s          BOOLEAN NOT NULL,
    fee_rate      NUMERIC(8,6) NOT NULL,
    fee_exponent  SMALLINT     NOT NULL,
    PRIMARY KEY (episode_id, leg_idx)
);
GRANT DELETE ON episode_legs TO scanner_rw;

-- Re-checks at open + offset (§7, §8.2).
CREATE TABLE observations (
    id                     BIGSERIAL PRIMARY KEY,
    episode_id             TEXT    NOT NULL REFERENCES episodes (episode_id),
    offset_ms              INTEGER NOT NULL,
    source                 TEXT    NOT NULL CHECK (source IN ('ws', 'rest')),
    observed_ms            BIGINT,
    skipped                BOOLEAN NOT NULL,
    skip_reason            TEXT,
    edge                   NUMERIC,
    q_max                  NUMERIC,
    pnl                    NUMERIC,
    still_profitable       BOOLEAN,
    fill_at_original_size  BOOLEAN
);
CREATE INDEX observations_episode ON observations (episode_id);

CREATE TABLE integrity_events (
    id        BIGSERIAL PRIMARY KEY,
    run_id    BIGINT NOT NULL REFERENCES runs (run_id),
    ts_ms     BIGINT NOT NULL,
    kind      TEXT   NOT NULL CHECK (kind IN ('crossed_book', 'mirror_mismatch', 'top_mismatch', 'drift',
                                              'drop', 'reconnect_gap', 'parse_error', 'shard_parked')),
    shard     INTEGER,
    token     TEXT,
    event_id  TEXT,
    detail    TEXT
);
CREATE INDEX integrity_events_run_ts ON integrity_events (run_id, ts_ms);

-- One row per run per minute (§10).
CREATE TABLE health (
    run_id           BIGINT  NOT NULL REFERENCES runs (run_id),
    minute_ms        BIGINT  NOT NULL,
    tokens           INTEGER NOT NULL,
    shards           INTEGER NOT NULL,
    msgs             BIGINT  NOT NULL,
    bytes            BIGINT  NOT NULL,
    drops            INTEGER NOT NULL,
    reconnects       INTEGER NOT NULL,
    rest_p50_ms      INTEGER,
    rest_p99_ms      INTEGER,
    lag_p50_ms       INTEGER,
    lag_p99_ms       INTEGER,
    clock_offset_ms  INTEGER,
    ok               BOOLEAN,
    capture_paused   BOOLEAN NOT NULL,
    parked_shards    INTEGER NOT NULL,
    dropped_groups   INTEGER NOT NULL,
    PRIMARY KEY (run_id, minute_ms)
);

-- The api turns this into SSE for the radar (§10).
CREATE FUNCTION notify_episode() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('episode', NEW.episode_id);
    RETURN NEW;
END
$$;
CREATE TRIGGER episodes_notify AFTER INSERT OR UPDATE ON episodes
    FOR EACH ROW EXECUTE FUNCTION notify_episode();
