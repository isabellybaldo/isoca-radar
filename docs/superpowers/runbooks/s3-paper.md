# S3 paper trading (Phase 3 exit)

Spec §19 exit: **deterministic replay; equity identity holds.** Everything here is read-only: no
ports, no credentials beyond the database role passwords in `.env`. It runs on the PC (PowerShell)
or on the VPS (bash): the `docker compose … psql` commands are the same.
Project disk budget: **30 GB**. Run `sh scripts/disk-budget.sh` (VPS) or
`powershell -ExecutionPolicy Bypass -File scripts\disk-budget.ps1` (PC) before step 1 and after
step 3.

## Notes before comparing runs (S3c fix round 2, ruling 26)

- **`capture_drops > 0` voids a live-vs-replay ledger comparison.** A dropped capture line can be
  a dropped `Ctl::Resolved`, which is a lost settlement in replay: the two ledgers then diverge for
  a reason that has nothing to do with a bug. Check
  `select coalesce(sum(drops), 0) from health where run_id = <LIVE>` (and any `capture_drop`
  integrity rows) before trusting a mismatch either way. Replay-vs-replay (step 1) is unaffected:
  it replays the same capture file twice.
- **A paper reset (Phase 7a) is a capture line too.** The live scanner captures
  `Ctl::PaperReset { t }` before it resets, and replay resets at that payload `t`, so live and
  replay ledgers still agree across a reset. A dropped `PaperReset` line (capture is best-effort)
  voids the comparison across it, like a dropped `Resolved`. Replay writes no `paper_epochs` row.
  Pre-7a images can neither replay nor compact an hour holding one.
- **Live-vs-replay ledgers match only for a run started from fresh accounts.** A scanner restart
  restores `paper_state`, and a restart also forgets consumed liquidity (S3c fix round 1: consumed
  liquidity is in-memory only, never persisted), so a restored trader can re-take liquidity it had
  already consumed before the restart. Compare live against replay only across a stretch with no
  restart in the middle, or reset `paper_state` first.
- **Changing anything in `policies.toml` starts every account fresh on the next restart**, not
  just the account whose own settings moved: the whole file's sha256 is recorded with every save,
  and any change to it (a scenario's `net_ms`/`match_ms`/burst, `[p1]` thresholds,
  `min_order_shares`, `settle_delay_ms`, markout offsets, an account's `bankroll`/`per_trade`/...)
  is a new pre-registered experiment. The scanner logs it once at startup:
  `paper: policies changed (saved <old|none> != current <new>); all accounts start fresh`. Seeing
  that line is expected right after any policies edit; NOT seeing it when you expected a fresh
  start is the bug to chase.
- **Before the first evaluation run, verify `umaResolutionStatus` against a live response.** Fetch
  one resolved NegRisk market from `gamma-api.polymarket.com/markets` and confirm that field name
  is exactly right (spec ruling 5: it is unverified against a live payload). A 50-50 (voided leg)
  market is accepted only when its payout vector is fractional AND that status reads `resolved`;
  if the field name is wrong, every 50-50 market silently never settles. Print no secrets when you
  do this — it's a public, unauthenticated endpoint.

## 0. Image and schema

The image must contain S3c, including `/etc/cute-trading/policies.toml`. Rebuild it once (it costs
disk: prune the build cache afterwards), then apply migration 0004:
```bash
docker compose --profile core build migrate
docker builder prune -f
docker compose --profile core run --rm migrate
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select count(*) from paper_state"
```
Note the policies hash; every paper account row must carry it:
```bash
sha256sum config/policies.toml                                            # VPS
Get-FileHash config\policies.toml -Algorithm SHA256 | % { $_.Hash.ToLower() }   # PC
```
Write down `POLICIES_SHA`.

## 1. Deterministic replay: the same prefix twice

Use a replay prefix copied as in `s2c-soak.md` §2 / `s2c-soak-vps.md` §3 (two complete hours plus
every `universe-*` file, under `/data/replay-prefix`). Replay it TWICE:
```bash
docker compose run --rm --no-deps scanner replay /data/replay-prefix
docker compose run --rm --no-deps scanner replay /data/replay-prefix
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select run_id, config_hash from runs where mode='replay' order by run_id desc limit 2"
```
Write down the two run ids as `R1` < `R2`. Both `config_hash` values must end in `+<POLICIES_SHA>`.

Episode ids embed the run id (`replay-<run>-<n>`), so strip that prefix before comparing:
```sql
with a as (select account_id, seq, t_ms, kind, regexp_replace(coalesce(episode_id, ''), '^replay-[0-9]+-', '') ep,
                  market_id, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity
           from paper_ledger where run_id = <R1>),
     b as (select account_id, seq, t_ms, kind, regexp_replace(coalesce(episode_id, ''), '^replay-[0-9]+-', '') ep,
                  market_id, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity
           from paper_ledger where run_id = <R2>)
select (select count(*) from a) a_rows, (select count(*) from b) b_rows,
       (select count(*) from (select * from a except select * from b) x) only_a,
       (select count(*) from (select * from b except select * from a) x) only_b;
```
Pass: `only_a = only_b = 0` and `a_rows = b_rows > 0` (every account marks at least once, at the
replay's close). The same check on marks:
```sql
select (select count(*) from (select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R1>
                              except select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R2>) x) only_a,
       (select count(*) from (select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R2>
                              except select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R1>) x) only_b;
```
Pass: `0, 0`.

## 2. The equity identity

`equity = bankroll + realized + unrealized`, written out from the mark's own cash:
`cash + (equity − cash) = bankroll + realized + unrealized`.
```sql
select count(*) marks,
       count(*) filter (where m.cash + (m.equity - m.cash) <> a.bankroll + m.realized + m.unrealized) broken
from paper_marks m join paper_accounts a using (run_id, account_id)
where m.run_id in (<R1>, <R2>);
select distinct policies_sha256 from paper_accounts where run_id in (<R1>, <R2>);
```
Pass: `broken = 0` with `marks > 0`, and the only hash is `POLICIES_SHA`.

Context for the write-up (not a pass/fail):
```sql
select a.account_id, count(*) filter (where i.skip_reason is null) taken, count(i.*) decided,
       max(m.realized) filter (where m.t_ms = (select max(t_ms) from paper_marks where run_id = <R1>)) realized_at_end
from paper_accounts a left join paper_intents i using (run_id, account_id)
     left join paper_marks m using (run_id, account_id)
where a.run_id = <R1> group by 1 order by 1;
select kind, count(*) from integrity_events where run_id = <R1> and kind = 'non_binary_payout' group by 1;
```
Then free the prefix: `docker run --rm -v cute-trading_data:/data alpine rm -rf /data/replay-prefix`.

## 3. Live check (short)

```bash
docker compose --profile core up -d
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select run_id from runs where mode='live' order by run_id desc limit 1"
```
Write down `LIVE`. After about 3 minutes:
```sql
select count(*) from paper_accounts where run_id = <LIVE>;                              -- = accounts in policies.toml (15)
select account_id, count(*) marks, max(t_ms) from paper_marks where run_id = <LIVE> group by 1 order by 1;   -- ≥ 2 each
select count(*) marks, count(*) filter (where m.cash + (m.equity - m.cash) <> a.bankroll + m.realized + m.unrealized) broken
from paper_marks m join paper_accounts a using (run_id, account_id) where m.run_id = <LIVE>;                  -- broken = 0
select count(*), max(updated_ms) from paper_state;                                       -- 15, within the last ~60 s
select kind, count(*) from paper_ledger where run_id = <LIVE> group by 1;
```
Engine cost with the trader attached (S3a-2 timing line, one per minute; it includes the trader's
per-frame hooks):
```bash
docker compose logs --since 5m scanner | grep "engine: frames"
```
Pass: `on_frame_us` p50 and p99 in the same range as the last pre-S3c soak's lines (write both
down), and `select coalesce(sum(drops), 0) from health where run_id = <LIVE>` is 0 (the
shard→engine channel never filled). A p99 that doubled means a paper hook is too expensive: stop
and profile before the soak.
Restart and check that the accounts carry over (the restored cash equals the last saved cash):
```bash
docker compose --profile core restart scanner
```
```sql
select run_id, account_id, t_ms, cash, realized from paper_marks
where account_id = 'P0-converter/sp-idle' order by t_ms desc limit 3;
```
Pass: the new run's first mark has the same `cash` and `realized` as the old run's final (close)
mark, unless that account traded in between. Then `docker compose --profile core stop scanner`
(or keep it running for the soak).

## Results
(fill in: date, POLICIES_SHA, R1/R2, ledger rows, only_a/only_b, marks/broken, LIVE, on_frame_us p50/p99 before and after, restart carry-over)
