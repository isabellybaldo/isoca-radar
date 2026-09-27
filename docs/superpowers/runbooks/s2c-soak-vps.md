# S2c soak on the VPS (Phase 2 exit)

The same exit as `s2c-soak.md` (**24 h without drops; replay reproduces episodes**), run on the
Hostinger KVM 2 (2 vCPU, 8 GB, 96 GB disk) so it survives PC sleep and Windows clock drift.
Read-only scanner only: no ports published, no Traefik network, no credentials except the
database role passwords in `.env`. Disk budget: **30 GB** (`sh scripts/disk-budget.sh`).

Measured on the PC (2026-09-25): the scanner uses 0.5–1.4 cores, about 640 MB RAM and about 2.4 MB/s
inbound. A VPS vCPU is slower, so the first 30 minutes double as a CPU trial (step 2).

## 0. Ship the bundle (from the PC, PowerShell)

```powershell
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" <bundle>\ct-vps.tar root@31.97.23.123:/root/
```

## 1. Install and start (on the VPS)

A fresh volume means a fresh capture directory and breaker, so the start of this run is the start of the soak.
```bash
mkdir -p /home/isoca/projects/cute-trading && cd /home/isoca/projects/cute-trading
tar -xf /root/ct-vps.tar && rm /root/ct-vps.tar
chmod 600 .env
gunzip -c image.tar.gz | docker load && rm image.tar.gz
timedatectl timesync-status | grep -E 'Server|Offset'      # offset should be under 50 ms
sh scripts/disk-budget.sh
docker compose --profile core up -d --no-build
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select run_id, started_ms from runs where mode='live' order by run_id desc limit 1"
```
Write down `LIVE` (the run_id) and `START` (started_ms).

## 2. CPU trial (about 30 min after the start)

```bash
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}'
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select count(*) minutes, sum(drops) drops, sum(reconnects) reconnects, max(parked_shards) parked, max(lag_p50_ms) lag_p50_max, max(lag_p99_ms) lag_p99_max from health where run_id = <LIVE>"
```
Continue if the scanner stays **under about 160 % CPU**, `drops = 0`, `parked = 0`, and the lag is close to the PC's (p50 about 290 ms).
Otherwise stop it (`docker compose --profile core stop scanner`) and report the numbers. The fix would be a cheaper capture compression level.

## 3. Copy the replay prefix (between about 1 h 10 min and 3 h after the start)

Hour files are `HH.jsonl.zst` in UTC `YYYY-MM-DD` folders. Copy ONLY the first two complete hours plus every universe record.
For example, if the soak started at 14:23 UTC, keep 14 and 15 (15 is complete once it is past 16:00):
```bash
docker run --rm -v cute-trading_data:/data alpine sh -c 'ls -l /data/capture/*/'
docker run --rm -v cute-trading_data:/data alpine sh -c 'd=YYYY-MM-DD; mkdir -p /data/replay-prefix/$d && cp -a /data/capture/$d/14.jsonl.zst /data/capture/$d/15.jsonl.zst /data/capture/$d/universe-* /data/replay-prefix/$d/ && du -sh /data/replay-prefix'
sh scripts/disk-budget.sh
```
`CUTOFF` = the end of the last kept hour in ms (e.g. 16:00:00 UTC that day). If the two hours span midnight UTC, copy each hour and the universe records from their own day folder.

## 4. After about 24 h: the no-drops check

```bash
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select count(*) minutes, sum(drops) drops, sum(reconnects) reconnects, max(parked_shards) parked, bool_or(capture_paused) paused, percentile_disc(0.99) within group (order by lag_p99_ms) lag_p99 from health where run_id = <LIVE>"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*), sum(coalesce(nullif(detail,'')::bigint, 1)) n from integrity_events where run_id = <LIVE> and kind in ('capture_drop','writer_drop','capture_budget_delete') group by 1"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*) from integrity_events where run_id = <LIVE> group by 1 order by 2 desc"
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select kind, count(*), sum((censor_left or censor_right or censor_gap)::int) censored, sum(stale_transport::int) stale, sum(engine_suspect::int) suspect from episodes where run_id = <LIVE> group by 1"
docker compose --profile core stop scanner
```
Pass: `drops = 0`, `parked = 0`, `minutes ≈ 1440`, and no `capture_drop` or `writer_drop` rows during the prefix window. `capture_budget_delete` rows are expected later in the run.

## 5. Replay the prefix and compare

```bash
docker compose run --rm --no-deps scanner replay /data/replay-prefix
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select max(run_id) from runs where mode='replay'"
```
Write down `REPLAY`, then run the comparison SQL from `s2c-soak.md` §4 through `docker compose exec postgres psql -U ct_owner -d cute_trading`.
Pass: `only_live = only_replay = 0` (and `live > 0`). Then free the prefix and re-check:
```bash
docker run --rm -v cute-trading_data:/data alpine rm -rf /data/replay-prefix
sh scripts/disk-budget.sh
```

## Results

### Run 1: 2026-09-25, stopped after 89 min, NOT a Phase 2 pass
- LIVE=1, START=1790340348680 (12:45:48 UTC). Image built at commit c6c0fb7 (S2c). KVM 2: 2 vCPU, 8 GB.
- **Resources:** the scanner used 150–370 % CPU (load average 2.9–4.1 on 2 vCPU) and 1.2–1.3 GB RAM. Capture ran ~1.95 GB/h (hour 13), about 47 GB/day, over Phase 0's 33 GB/day estimate. Disk peaked at 6.1 GB.
- **Feed:** drops 0. Lag p50 was 72–85 ms in normal minutes (better than the PC's ~290 ms). p99 lag had a median of 1.9 s and a maximum of 24.3 s, with spikes when the CPU saturated.
- **Reconnects:** 136 in total. 40 at a time at every retier (13:24, 13:59): a full re-plan moved every shard. Parked max 1 near the end.
- **Integrity:** normal rate ~10 top_mismatch/min until 13:31. Then **one token** (market 4689155, "Bitcoin price on September 25?", shard 1) looped: 20,240 top_mismatch rows in 35 min.
  - The cause was a phantom bid level: ours sat 0.01–0.02 above the server's best bid.
  - REST resnapshots were rejected as older than the ws-advanced book, so the book was never repaired, and the resnapshot flood starved the REST budget.
  - crossed_book 289, drift 66, mirror_mismatch 31.
- **Store:** one `refresh persist` statement timeout. The universe upsert hit scanner_rw's 5 s statement_timeout under load.
- **Episodes (at 76 min):** yes_basket_incomplete 271 (137 stale), no_basket 72, yes_basket 9.
- **Fixes:** S3a Task 4 (stop changed slots before start; idle slots; no churn on refreshes between retiers) and **Task 8** (repair phantom levels from the stream's top; throttle integrity rows and resnapshots; 60 s universe-save timeout). Also noted for Phase 8: KVM 2 CPU headroom for the full universe plus the sim. **Not fixed (deferred):** a retier is still a full re-plan, so expect all 40 connections to reconnect every 30 min in run 2 (paced at 20/min, so about 2 min until the last shard is back). Soak 2 pass criteria must allow it, and record CPU next to the retier times.
- The replay prefix was deleted: after Task 8 the replay would diverge from this live run by design. Redeploy after S3a and restart the soak with a fresh prefix.

### Run 2: 2026-09-25, LIVE=3, stopped after 3 h 47 min. Replay equality PASSED; no-drops FAILED
- Image 088c43d (S3a). START=1790356605308 (17:16:45 UTC), CUTOFF=1790362800000 (19:00 UTC); prefix = hours 17 and 18.
- **Fixed vs run 1:**
  - The phantom-level loop is gone: the worst single token had 17 top_mismatch in total (run 1: 20,240 in 35 min).
  - Retiers (40 reconnects each) no longer trigger integrity floods.
  - 52 stream repairs in the first 30 min.
- **Failed:**
  - 19 drops: the shard→engine channel was full, because of engine CPU. `Engine::evaluate` scanned all of `pending_tops` on every frame.
  - p99 lag reached 50.7 s.
  - VPS steal time was 35–46 % under load vs 1–6 % idle, which looks like Hostinger capping sustained CPU. Budget ~1 effective vCPU.
- **Memory:** grew from 1.08 to 1.40 GB over 3.7 h. Watch it in run 3.
- **Capture cap:** not reached (6.0 GB at stop).
- **Replay equality (Phase 2 exit, part 2) PASSED.** Replay run 5 used image 2c5b84d (S3a-2), 24,539,540 frames, 0 parse errors, in 922 s (~38 µs/frame on the VPS). Episodes decided inside [START, CUTOFF): live 367, replay 367, only_live 0, only_replay 0.
- **Fixes shipped after run 2:**
  - S3a-2: exact top-of-book gate; `pending_tops` indexed by connection and by group. Release bench 365 → 19.6 µs/frame. Per-minute `engine:` timing log line.
  - aba9ce5: replay streams to a sink, so memory stays bounded. Run 5's 2.9 GB peak came from the old collect-everything path.

### Run 3: 2026-09-26 → 27, LIVE=7, 24 h. No-drops PASSED; Phase 2 exit met
- **Image and window:** image 2c5b84d (S3a-2). START=1790384263621 (2026-09-26 00:57:43 UTC). The 24 h window ended 2026-09-27 00:57:43 UTC.
- **24 h window:** 1,438 health minutes, **0 drops**, 2,205 reconnects (retiers), max 2 parked shards, capture never paused. Lag p99: median minute 202 ms; 99th percentile minute 26.3 s; worst minute 60.5 s.
- **Worst lag:** one storm at 20:47–20:51 UTC (47–60 s, 2–6 reconnects per minute, still 0 drops). Earlier isolated spikes: 09:23 (19.8 s), 13:24 (21.6 s), 14:45–14:47 (26–43 s).
- **Episodes:** 8,880 in run 7 (all net-positive by the episode definition, Π > 0 net of fees); 61 open at check time.
- **Resources:**
  - Scanner memory 1.48 GiB at 24 h, flat since ~14 h (1.51 GiB), so no leak trend.
  - CPU steal 25–59 % under load; Hostinger throttling persists, but the scanner absorbed it with no drops.
- **Disk:**
  - Found: the capture housekeeping never ran in release (an `i64::MIN` sentinel wrapped). Capture reached 24.2 GB and the project total hit 30.01 GB at ~14 h.
  - Mitigation: a host trim loop (hour files older than 120 min). Project total 10.77 GB at 24 h.
  - Fixed in code at cc51942; not yet deployed.
- **Replay equality:** not re-run. It passed on run 2's data (run 5), and the scanner is unchanged since.
- **Phase 2 exit:** both halves are met: 24 h without drops (this run) and replay reproduces episodes (run 2).
- **After the run:** the scanner keeps running as the permanent live service (owner's call, 2026-09-26). The trim loops stay until the next deploy, which carries cc51942. Run 7 is the first run the Phase 5a Gebele cards count (backfill `run_id >= 7`).
