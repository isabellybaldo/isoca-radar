# S2c soak (Phase 2 exit)

Spec §19 exit: **24 h without drops; replay reproduces episodes.** Keep the PC awake the whole time.

## 0. Before starting

- **Clock:** synced (Phase 0 fix: `w32tm` peers `a.st1.ntp.br` / `time.google.com`). Check it with `w32tm /stripchart /computer:a.st1.ntp.br /samples:3 /dataonly`; the offset should be under 50 ms.
- **Disk:** Docker Desktop's VM cannot see the Windows drive. Inside the container the disk looks nearly empty even when C: is full (smoke test 2026-09-25: VM 936 GB free vs host 4.2 GB). Make sure C: has **at least 25 GB free** (`Get-PSDrive C`).
  - The scanner caps capture at `capture.max_capture_gb` (10 GB ≈ 4–5 h at full rate) by deleting the oldest hours.
  - Postgres and Docker's virtual disk also grow.
- **Fresh capture directory**, so the replay prefix starts exactly at the soak's start:
```powershell
docker compose --profile core stop scanner
docker run --rm -v cute-trading_data:/data alpine sh -c "rm -rf /data/capture /data/replay-prefix; mkdir -p /data/capture; chown 65532:65532 /data/capture"
docker compose run --rm scanner breaker reset
```

## 1. Start and note the start

```powershell
docker compose --profile core up -d
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select run_id, started_ms from runs where mode='live' order by run_id desc limit 1"
```
Write down `LIVE` (the run_id) and `START` (started_ms).

## 2. Copy the replay prefix out (between ~1 h 10 min and ~3 h after the start)

The capture budget later deletes the oldest hours, so save the first two **complete** UTC hours plus every universe record now. Hour files are named `HH.jsonl.zst` in `YYYY-MM-DD` folders (UTC):
```powershell
docker run --rm -v cute-trading_data:/data alpine sh -c "ls -l /data/capture/*/"
# copy the day folder(s), then delete from the copy every hour file newer than the 2nd complete hour:
docker run --rm -v cute-trading_data:/data alpine sh -c "cp -a /data/capture /data/replay-prefix"
docker run --rm -v cute-trading_data:/data alpine sh -c "ls /data/replay-prefix/*/"
# e.g. if the soak started at 14:23 UTC, keep 14.jsonl.zst and 15.jsonl.zst (15 is complete once it is past 16:00):
docker run --rm -v cute-trading_data:/data alpine sh -c "rm -f /data/replay-prefix/<YYYY-MM-DD>/16.jsonl.zst /data/replay-prefix/<YYYY-MM-DD>/17.jsonl.zst"
```
`CUTOFF` = the end of the last kept hour, in ms (e.g. 16:00:00 UTC that day). Keep every `universe-*.json.zst` file in the copy.

## 3. After ~24 h: the no-drops check

```powershell
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select count(*) minutes, sum(drops) drops, sum(reconnects) reconnects, max(parked_shards) parked, bool_or(capture_paused) paused, percentile_disc(0.99) within group (order by lag_p99_ms) lag_p99 from health where run_id = <LIVE>"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*), sum(coalesce(nullif(detail,'')::bigint, 1)) n from integrity_events where run_id = <LIVE> and kind in ('capture_drop','writer_drop','capture_budget_delete') group by 1"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*) from integrity_events where run_id = <LIVE> group by 1 order by 2 desc"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*), sum((censor_left or censor_right or censor_gap)::int) censored, sum(stale_transport::int) stale, sum(engine_suspect::int) suspect from episodes where run_id = <LIVE> group by 1"
docker compose --profile core stop scanner
```
Pass:
- `drops = 0` (pm-api `Dropped` never fired), `parked = 0`, `minutes ≈ 1440`;
- no `capture_drop` or `writer_drop` rows during the prefix window. `capture_budget_delete` rows are expected later in the run.

## 4. Replay the prefix and compare

```powershell
docker compose run --rm scanner replay /data/replay-prefix
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select max(run_id) from runs where mode='replay'"
```
Write down `REPLAY`. Compare only episodes that are fully decided inside the prefix: opened at or after `START`, closed before `CUTOFF`, and not right-censored. Replay closes still-open episodes at its last input, so those can never match. Episode ids differ (`live-…` vs `replay-…`), so compare by content:
```sql
with l as (select event_id, kind, opened_server_ms, closed_server_ms, duration_ms, censor_left, censor_gap, peak_pnl_net_usd, peak_max_size
           from episodes where run_id = <LIVE> and opened_server_ms >= <START> and closed_server_ms < <CUTOFF> and not censor_right),
     r as (select event_id, kind, opened_server_ms, closed_server_ms, duration_ms, censor_left, censor_gap, peak_pnl_net_usd, peak_max_size
           from episodes where run_id = <REPLAY> and opened_server_ms >= <START> and closed_server_ms < <CUTOFF> and not censor_right)
select (select count(*) from l) live, (select count(*) from r) replay,
       (select count(*) from (select * from l except select * from r) x) only_live,
       (select count(*) from (select * from r except select * from l) y) only_replay;
```
Pass: `only_live = only_replay = 0` (and `live > 0`).
- `observations` differ by design: REST re-checks aren't part of replay.
- `engine_suspect` can differ only where a REST-driven suspect mark wasn't captured. That would be a bug, so report it.

## Results
(fill in: dates, START/CUTOFF, minutes, drops, reconnects, parked, lag p99, drop/budget integrity rows, episode counts by kind, replay comparison numbers)
