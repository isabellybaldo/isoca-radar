# S2c soak (Phase 2 exit)

Spec §19: 24 h without drops; replay reproduces episodes. Keep the PC awake. Clock synced first (Phase 0: `w32tm` peers).

**Disk first.** Docker Desktop's VM cannot see the Windows drive: inside the container the disk looks nearly empty even when C: is full (smoke test 2026-09-25: VM 936 GB free vs host 4.2 GB). Before the soak, make sure C: has at least **25 GB free** (`Get-PSDrive C`). The scanner caps its own capture at `capture.max_capture_gb` (10 GB), but Postgres and Docker's virtual disk also grow.

```powershell
docker compose --profile core up -d
# after ~24 h:
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select count(*) minutes, sum(drops) drops, sum(reconnects) reconnects, max(parked_shards) parked, bool_or(capture_paused) paused, percentile_disc(0.99) within group (order by lag_p99_ms) lag_p99 from health where run_id = (select max(run_id) from runs where mode='live')"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*) from integrity_events where run_id = (select max(run_id) from runs where mode='live') group by 1 order by 2 desc"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*), sum((censor_left or censor_right or censor_gap)::int) censored, sum(stale_transport::int) stale from episodes where run_id = (select max(run_id) from runs where mode='live') group by 1"
# replay one captured hour with no pause and no capture drops, then compare:
docker compose run --rm scanner replay /data/capture/<YYYY-MM-DD> 
docker compose exec postgres psql -U ct_owner -d cute_trading -c "<compare query below>"
```

Compare query (episodes of the live run vs the replay run, over the replayed hour's window, by event/kind/open time):
```sql
with l as (select event_id, kind, opened_server_ms, duration_ms, peak_pnl_net_usd from episodes where run_id = <LIVE> and opened_server_ms between <T0> and <T1>),
     r as (select event_id, kind, opened_server_ms, duration_ms, peak_pnl_net_usd from episodes where run_id = <REPLAY> and opened_server_ms between <T0> and <T1>)
select (select count(*) from l) live, (select count(*) from r) replay,
       (select count(*) from (select * from l except select * from r) x) only_live,
       (select count(*) from (select * from r except select * from l) y) only_replay;
```
Pass: `drops = 0` (pm-api `Dropped` never fired), `parked = 0`, and `only_live = only_replay = 0` for the replayed window.
Differences from REST re-check timing are expected in `observations`, never in `episodes`.

## Results
(fill in: dates, minutes, drops, reconnects, parked, lag p99, integrity counts, episode counts, replay comparison)
