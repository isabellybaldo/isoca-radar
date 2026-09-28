# Phase 5b on the VPS: tiered storage (compact service, Parquet archive, delete guard) + scanner boot unit

One deploy: 5b (migration 0008, `compact`, scanner retention guard, Archive box) and PR #6 (analyze `restart: unless-stopped`, the scanner's systemd boot unit).
Rules: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; never start a second `compact run` (the archive lock refuses it); PC steps in Git Bash, VPS steps as root over SSH. Fill `'<…>'` values before running a step.

## On the PC

1. The repo on the branch being deployed; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s'
```
→ expected: no status lines, the branch name, then `<sha> <subject>` of the commit to deploy.
if not: commit or stash the listed changes, then run this step again.

2. Line endings: migrations and configs baked into the image must be LF.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git ls-files --eol crates/store/migrations config | grep -v 'w/lf' ; grep -rlI "$(printf '\r')" crates/store/migrations config || echo lf-ok
```
→ expected: only `lf-ok`.
if not: `git checkout -- <each listed file>` (.gitattributes pins LF) and run this step again.
**STOP**

3. Migration checksums: 0001–0007's bytes must match what production recorded.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p store --test migration_checksums 2>&1 | tail -3
```
→ expected: `test result: ok. 2 passed; 0 failed`.
if not: a CRLF or edited migration: fix it as in step 2 and run again.
**STOP**

4. The Postgres-backed tests (store, compact, api, analyze) really run against the PC Postgres on 127.0.0.1:15432 (password read from `.env`, never printed).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && (exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && ( set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1; cargo test -p store -p compact -p api -p analyze -- --nocapture 2>&1 | grep -E '^test result|SKIPPED|panicked' | sort | uniq -c; echo "cargo exit=${PIPESTATUS[0]}" )
```
→ expected: `pg-up`, then only `<n> test result: ok. …; 0 failed …` lines (no `FAILED`, no `SKIPPED`, no `panicked`), then `cargo exit=0`.
if not: no `pg-up` = start the PC Postgres first; a `SKIPPED` line means a DB test did not run: **STOP**.
**STOP**

5. The new `.env` key on the PC (compose refuses to parse without it; the value is never printed).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && (grep -q '^COMPACT_RW_DB_PASSWORD=' .env || printf 'COMPACT_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env) && grep -c -E '^(ANALYZE_RW_DB_PASSWORD|SCANNER_HOST|COMPACT_RW_DB_PASSWORD)=' .env
```
→ expected: `3`.
if not: add the missing key the same way (`openssl rand -hex 24`), never with an editor that echoes it.

6. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `cargo clean` and `docker builder prune -f --keep-storage 5GB`, then run this step again.

7. Build both images (Rust: scanner, api, analyze, compact; web).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA" && docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web
```
→ expected: the short sha, then both builds end without `ERROR` (a `CRLF found` failure means step 2 was skipped).
if not: read the first `ERROR` line; fix and run this step again.
**STOP**

8. The compactor and its config are in the image (`verify` on a missing day loads the config, then fails on the day).
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/compact cute-trading/rust:dev --help | head -1; MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/compact cute-trading/rust:dev verify 2000-01-01
```
→ expected: `raw capture hours → the Parquet archive (paper trading only)`, then `compact: read /archive/2000-01-01/manifest.json: No such file or directory (os error 2)`.
if not: a line naming `compact.toml` = the config isn't in the image; no first line = step 7 built from the wrong tree; rebuild.

9. (Benchmark, on the VPS) Copy the newest closed raw hour and the universe records its control lines name.
```bash
C=/var/lib/docker/volumes/cute-trading_data/_data/capture; F=$(ls $C/*/[0-9][0-9].jsonl.zst | tail -2 | head -1); D=$(basename "$(dirname "$F")"); command -v zstdcat >/dev/null || apt-get install -y zstd; mkdir -p ~/bench/$D && cp "$F" ~/bench/$D/ && for f in $(zstdcat "$F" | grep '"kind":"universe"' | grep -o 'universe-[0-9]*-[0-9]*\.json\.zst' | sort -u); do t=${f#universe-}; t=${t%%-*}; d=$(date -u -d @$((t / 1000)) +%F); mkdir -p ~/bench/$d && cp $C/$d/$f ~/bench/$d/; done; tar -cf ~/bench.tar -C ~ bench && rm -rf ~/bench && echo "$F" && du -h ~/bench.tar
```
→ expected: the hour's path `…/capture/<YYYY-MM-DD>/<HH>.jsonl.zst`, then `<size>  /root/bench.tar` (about one raw hour, ~1 GB).
if not: skip steps 9–12; the spec §3 defaults stay (write "no benchmark" in Results).

10. (Benchmark, on the PC) Fetch and unpack it, then delete it on the VPS.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123:/root/bench.tar "$HOME/bench.tar" && rm -rf "$HOME/bench" && tar -xf "$HOME/bench.tar" -C "$HOME" && rm "$HOME/bench.tar" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 rm /root/bench.tar && (cd "$HOME/bench" && ls */*.jsonl.zst)
```
→ expected: one line `<YYYY-MM-DD>/<HH>.jsonl.zst`.
if not: check the scp line's key path (quoted, forward slashes).

11. (Benchmark, on the PC) The release compactor on that hour under the service's limits.
```bash
H=$(cd "$HOME/bench" && ls */*.jsonl.zst) && MSYS_NO_PATHCONV=1 docker run --rm --cpus 0.25 --memory 256m -v "$(cygpath -m "$HOME/bench"):/bench:ro" -v ct-bench-out:/archive --entrypoint /usr/local/bin/compact cute-trading/rust:dev bench "/bench/$H" --out /archive/bench; docker volume rm ct-bench-out
```
→ expected: `bench: <day>/<HH>.jsonl.zst <ms> ms, … per_day_bytes=<n> …, checkpoint_bytes=<n>, then <day>/<HH>.jsonl.zst from the loaded checkpoint <ms2> ms, peak_rss_kb=<n>` with `<ms>` < 600000 and `peak_rss_kb` < 200000 (the hour, its checkpoint and a second pass from it), then `ct-bench-out`; write the line in Results.
if not: over a limit (or exit 137): **STOP**, do not deploy, report the line; `per_day_bytes` over 250 MB with depth over 50 MB a day: set `depth_every_secs = 600` in `config/compact.toml`, commit, redo steps 7–11.

12. (Benchmark, on the PC) Both agreement tests on the real hour, debug build (can take tens of minutes; don't kill it).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && TAPE_AGREEMENT_DIR="$(cygpath -m "$HOME/bench")" cargo test -p scanner --test tape_agreement --test tob_agreement -- --ignored --nocapture 2>&1 | grep -E 'tape agreement|tob agreement|test result|SKIPPED'
```
→ expected: `tape agreement: <n> lines, books equal after every one`, `tob agreement: <n> rows equal the engine's top`, two `test result: ok. 1 passed`, no `SKIPPED`; write both counts in Results, then `rm -rf "$HOME/bench"`.
if not: **STOP**, do not deploy; the failing line and token go to the plan owner.

13. Bundle the images, compose files, `.env.example`, roles script and disk-budget script.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-5b/docker/postgres/init" "$HOME/ct-5b/scripts" && cd "$HOME/ct-5b" && docker save cute-trading/rust:dev cute-trading/web:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && cp "$R/docker/postgres/init/01-roles.sh" docker/postgres/init/ && cp "$R/scripts/disk-budget.sh" scripts/ && tar -cf "$HOME/ct-5b.tar" image.tar.gz compose.yml compose.web.yml .env.example docker scripts && cd "$HOME" && rm -rf "$HOME/ct-5b" && ls -l "$HOME/ct-5b.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-5b.tar`, roughly 50–150 MB (the two images are ~75 MB and ~82 MB before gzip).
if not: free disk (step 6) and run this step again.

14. Ship the bundle to the VPS.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-5b.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-5b.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

15. Disk headroom: with the project at its 75 GB ceiling (40 archive + 30 raw + ~1 database + images) the disk (~96 GB) must keep ≥ 20 GB free (capture pauses below 10 GB free).
```bash
cd /home/isoca/projects/cute-trading && df -BG --output=size,used,avail /var/lib/docker | tail -1 && du -s --block-size=1G /var/lib/docker/volumes/cute-trading_* | awk '{s += $1} END {print "project_gb", s}'
```
→ expected: `<size>G <used>G <avail>G` then `project_gb <p>`; with `other = used − p`, `size − other − 75 ≥ 20`.
if not: set `archive_budget_gb = size − other − 51` (20 free + 30 raw + 1 database; rounded down, ≥ 1) in `config/compact.toml` on the PC, commit, redo steps 7–14.
**STOP**

16. Rollback tags on the current images (before step 18's load moves `:dev`).
```bash
cd /home/isoca/projects/cute-trading && docker tag cute-trading/rust:dev cute-trading/rust:pre5b && docker tag cute-trading/web:dev cute-trading/web:pre5b; docker image inspect -f '{{.Id}}' cute-trading/rust:pre5b cute-trading/web:pre5b; docker inspect -f '{{.Name}} {{.Image}}' cute-trading-scanner-1 cute-trading-web-1
```
→ expected: two `sha256:…` IDs, then `/cute-trading-scanner-1` and `/cute-trading-web-1` on those same two IDs.
if not: a container on another ID: `docker tag <its sha256 ID> cute-trading/rust:pre5b` (or `cute-trading/web:pre5b`) and run this step's last two commands again until they match.
**STOP**

17. Back up the database.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres pg_dump -U ct_owner -d cute_trading -Fc > /root/ct-pre-5b.dump && ls -lh /root/ct-pre-5b.dump
```
→ expected: one `ls` line, a few hundred MB.
if not: free space and run this step again; do not migrate without it.
**STOP**

18. Load the bundle (replaces compose.yml, compose.web.yml, .env.example, the roles script and disk-budget.sh), then the project's disk budget.
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-5b.tar && rm /root/ct-5b.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls 'cute-trading/*' --format '{{.Repository}}:{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/rust:dev`, `Loaded image: cute-trading/web:dev`; `:dev` on new IDs, `:pre5b` on the old ones; then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: truncated tar = re-ship (step 14); over budget = `docker image prune -f` and run the budget again.

19. Compose parses with the new service, keys and restart policies.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c -E '^  compact:$|COMPACT_RW_DB_PASSWORD' compose.yml && grep -c 'restart: unless-stopped' compose.yml
```
→ expected: `config-ok`, `3`, then `3` (postgres, analyze, compact).
if not: the bundle's compose files are old; re-bundle (step 13).
**STOP**

20. The new `.env` key on the VPS (never printed).
```bash
cd /home/isoca/projects/cute-trading && { grep -q '^COMPACT_RW_DB_PASSWORD=' .env || printf 'COMPACT_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env; } && chmod 600 .env && grep -c '^COMPACT_RW_DB_PASSWORD=' .env
```
→ expected: `1`.
if not: remove the duplicate lines with `sed -i` on the key name (never print values), then run again.

21. Stop the scanner and analyze (capture pauses until step 25), then recreate postgres on purpose (its environment gained `COMPACT_RW_DB_PASSWORD`; the Radar shows errors for a few seconds).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 scanner analyze && docker compose up -d --no-build --timeout 60 postgres && for i in $(seq 90); do [ "$(docker inspect -f '{{.State.Health.Status}}' cute-trading-postgres-1)" = healthy ] && break; sleep 2; done; docker inspect -f '{{.State.Health.Status}}' cute-trading-postgres-1
```
→ expected: scanner and analyze `Stopped`, `Container cute-trading-postgres-1 Recreated` then `Started` (`Running` if it was already current), then `healthy`.
if not: `docker compose logs --tail 50 postgres`; start nothing else.
**STOP**

22. Migrate ALONE (`--no-deps`: nothing else is touched).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core run --rm --no-deps migrate; echo "migrate exit=$?"
```
→ expected: `migrate exit=0`.
if not: `docker compose --profile core logs --tail 50 migrate`; start nothing; a checksum error means a CRLF image: redo steps 2–18.
**STOP**

23. Migration 8 is applied and the grants are right.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT max(version) FROM _sqlx_migrations WHERE success" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT has_table_privilege('compact_rw','compact_hours','UPDATE'), has_table_privilege('compact_rw','episodes','INSERT'), has_table_privilege('scanner_rw','compact_hours','INSERT'), has_table_privilege('scanner_rw','compact_hours','UPDATE'), has_table_privilege('analyze_ro','compact_status','SELECT'), has_table_privilege('scanner_rw','compact_status','SELECT')"
```
→ expected: `8` then `t|f|t|f|t|t`.
if not: start nothing; full rollback (step 45).
**STOP**

24. Activate `compact_rw` (pgdata exists, so the init script won't; password never printed).
```bash
cd /home/isoca/projects/cute-trading && PW="$(grep -E '^COMPACT_RW_DB_PASSWORD=' .env | cut -d= -f2-)"; case "$PW" in ''|*[!0-9a-f]*) echo "pw-bad"; PW= ;; *) echo pw-read ;; esac; [ -n "$PW" ] && printf '\\set pw %s\nSET log_min_error_statement = panic;\nALTER ROLE compact_rw LOGIN PASSWORD :'"'"'pw'"'"';\n' "$PW" | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -q -U ct_owner -d cute_trading && echo role-set; unset PW; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'compact_rw'"
```
→ expected: `pw-read`, `role-set`, `t`.
if not: `pw-bad` = the `.env` line isn't an `openssl rand -hex 24` value: `sed -i '/^COMPACT_RW_DB_PASSWORD=/d' .env`, redo step 20, run again.
**STOP**

25. Start the new scanner (new retention, delete guard, universe dedup) and note the deploy time. Go straight on to steps 26–27: without the compactor, the scanner's lost marks light **compactor has never reported** after 2 h.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/5b_deploy_ms && docker compose --profile core up -d --no-build --no-deps scanner && sleep 60 && docker compose --profile core logs --since 70s scanner 2>&1 | grep -E 'capture:|error|panicked' | head
```
→ expected: `capture: compactor silent (no compact_status within 6 h); retention falls back to age + cap` and no `error`/`panicked` lines.
if not: full rollback (step 45).

26. Start the new analyze (now `restart: unless-stopped`) and check the restart policies.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps analyze && sleep 20 && docker inspect -f '{{.Name}} {{.HostConfig.RestartPolicy.Name}} {{.State.Status}}' cute-trading-analyze-1 cute-trading-scanner-1 && docker compose --profile core logs --since 30s analyze 2>&1 | grep -c -E 'error|panicked'
```
→ expected: `/cute-trading-analyze-1 unless-stopped running`, `/cute-trading-scanner-1 on-failure running`, then `0`.
if not: `docker compose --profile core logs --tail 30 analyze`.

27. Start the compactor (cold start at 00:00 of the first raw hour's day; earlier hours become `scanner_stopped` in seconds). Set `BENCH_MS` first.
```bash
BENCH_MS=600000   # step 11's <ms>; keep 600000 without a benchmark
cd /home/isoca/projects/cute-trading && C=/var/lib/docker/volumes/cute-trading_data/_data/capture; N=$(ls $C/*/[0-9][0-9].jsonl.zst | wc -l); echo "raw_hours $N"; echo "catch-up bound (estimate) $(( N * BENCH_MS * 3600000 / (3600000 - BENCH_MS) / 60000 + 10 )) min from $(date -u +%H:%M) UTC" | tee /root/5b_catchup; docker compose --profile core up -d --no-build --no-deps compact && sleep 90 && docker compose --profile core logs --since 100s compact 2>&1 | head -8
```
→ expected: `raw_hours <N>`, `catch-up bound (estimate) <m> min …`, then `compact: every 60 s, …`, `compact: no checkpoint: starting cold at <day>T00 (first raw hour <day>T<HH>)`, `compact: <day>T<HH> done in <ms> ms, …` lines; the Radar's **compactor behind: raw at risk** heart stays lit until about the bound.
if not: `database unreachable` = redo step 24, then `docker compose --profile core restart compact`; any `refusing to start` = stop the compactor (step 44) and report.

28. Scanner websockets baseline, then restart api and web (do it when capture matters least; the last `web` recreate cost the scanner 40 reconnects).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/5b_web_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/5b_scanner_started && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs, lag_p50_ms FROM health ORDER BY minute_ms DESC LIMIT 3" && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web && docker compose -f compose.yml -f compose.web.yml --profile web ps api web --format '{{.Service}} {{.Status}}'
```
→ expected: the scanner's start time (step 25), three `HH:MI|<shards>|0|0|<msgs>|<lag>` rows, `Container cute-trading-api-1 …` and `Container cute-trading-web-1 …` lines only (no network, no other container), then `api Up …` and `web Up …`.
if not: reconnects already > 0 in the baseline rows: note them in Results and go on (step 29 then expects no more than that); api/web not `Up`: `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 api web`.

29. After the restart: no websocket disconnect, and the scanner now waits for the compactor.
```bash
cd /home/isoca/projects/cute-trading && sleep 180; T0=$(cat /root/5b_web_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/5b_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs, lag_p50_ms FROM health WHERE minute_ms >= $T0 - 60000 ORDER BY minute_ms" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"; docker compose --profile core logs scanner 2>&1 | grep 'capture: compactor' | tail -1
```
→ expected: `scanner-not-restarted`, 3–4 health rows all `…|0|0|…` (msgs steady), `0` reconnect gaps, then `capture: compactor reporting; raw hours wait for compaction`.
if not: reconnects > 0: do NOT roll back; record the rows and count in Results; no `reporting` line: check step 31.

30. Install the scanner boot unit (the scanner keeps `on-failure:5`, since safe mode exits 0 on purpose).
```bash
D=$(command -v docker) && echo "docker $D" && cat > /etc/systemd/system/isoca-radar-scanner.service <<EOF
[Unit]
Description=isoca-radar: start the scanner at boot
Requires=docker.service
After=docker.service network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/home/isoca/projects/cute-trading
ExecStart=$D compose --profile core up -d --no-build scanner
TimeoutStartSec=300

[Install]
WantedBy=multi-user.target
EOF
grep ExecStart /etc/systemd/system/isoca-radar-scanner.service && systemctl daemon-reload && systemctl enable isoca-radar-scanner.service && systemctl is-enabled isoca-radar-scanner.service
```
→ expected: `docker /usr/bin/docker`, `ExecStart=/usr/bin/docker compose --profile core up -d --no-build scanner`, `Created symlink …`, then `enabled`.
if not: `systemctl status isoca-radar-scanner.service --no-pager`.

## On the VPS: checks

31. Catch-up progress and the status row (run any time after step 27).
```bash
cd /home/isoca/projects/cute-trading && cat /root/5b_catchup && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(updated_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), archive_bytes, oldest_day, lag_hours, last_error FROM compact_status" && docker compose --profile core logs --since 10m compact 2>&1 | grep -c ' done in '
```
→ expected: the bound, then `HH:MI|<bytes>|<YYYY-MM-DD>|<lag>|` updated within the last minutes with an empty last_error (lag > 3 is normal before the bound, 1 or 2 after it), then a count > 0 while catching up.
if not: lag > 3 after the bound, or a last_error: `docker compose --profile core logs --tail 30 compact` and report; never start a second `compact run`.

32. First compaction check (after the bound): hour H becomes `done` once hour H+2 has started.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(hour_ms/1000) AT TIME ZONE 'UTC','MM-DD HH24'), status, rows FROM compact_hours ORDER BY hour_ms DESC LIMIT 3"
```
→ expected: three `MM-DD HH|done|{"tob": …}` lines, the newest label 2–3 h before now (UTC hour).
if not: `docker compose --profile core logs --tail 30 compact`; do not leave it running with errors: stop the compactor (step 44).
**STOP**

33. Container limits hold while it works.
```bash
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}' | grep compact
```
→ expected: `cute-trading-compact-1 <≤ 25%> <under 256MiB> / 256MiB`; write it in Results.
if not: memory near the limit: report the line with step 11's `peak_rss_kb`.

34. No hour lost at the cap since the deploy.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM compact_hours WHERE status = 'lost' AND done_ms > $(cat /root/5b_deploy_ms)"; docker compose --profile core logs scanner 2>&1 | grep -c 'deleted before compaction'
```
→ expected: `0` then `0`.
if not: the compactor is too slow for the 30 GB cap: report the lag (step 31) and `docker stats` (step 33).

35. Raw capture stays under its cap (run 24 h after step 25).
```bash
du -sb /var/lib/docker/volumes/cute-trading_data/_data/capture | awk '{print ($1 <= 30000000000) ? "cap-ok" : "OVER"}' && ls /var/lib/docker/volumes/cute-trading_data/_data/capture/$(date -u +%F) | grep -c universe-
```
→ expected: `cap-ok`, then a count far below 12 per hour elapsed today.
if not: `OVER` means the cap isn't enforced: report the scanner log's `capture:` lines.

36. The first merged day (yesterday merges during the catch-up if the first raw hour was yesterday's; otherwise ~01:00–02:00 UTC the next day).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core logs compact 2>&1 | grep 'compact: merged' | tail -3
```
→ expected: `compact: merged <YYYY-MM-DD>, tob=… depth=… l2=… gaps=…, <n> B`.
if not: none yet: run again later; a `day … not merged` in last_error (step 31): report it.

37. Verify that day in a one-off container (`verify` takes no lock; the service keeps running).
```bash
cd /home/isoca/projects/cute-trading && DAY='<YYYY-MM-DD>'; docker compose --profile core run --rm --no-deps -T compact verify "$DAY"; echo "verify exit=$?"; python3 -c "import json,sys; m=json.load(open(sys.argv[1])); print('tob', m['totals']['tob'], 'ts_clamped', m['ts_clamped'], 'late_rows', m['late_rows'], 'bytes', m['bytes'])" /var/lib/docker/volumes/cute-trading_archive/_data/$DAY/manifest.json
```
→ expected: `verify <day>: ok`, `verify exit=0`, then one line with `ts_clamped` and `late_rows` small next to `tob`, and the bytes per table; write them in Results.
if not: `verify exit=137` = out of memory (report it, the archive is fine); listed problems: **STOP** compaction (step 44) and report every one.

38. (On the PC) Pin 0008's production checksum in the checksum test and commit it.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && H=$(ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 "cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc \"SELECT encode(checksum, 'hex') FROM _sqlx_migrations WHERE version = 8\"" | tr -d '\r\n ') && echo "hex_len ${#H}" && python - "$H" <<'PY' && cargo test -p store --test migration_checksums 2>&1 | tail -3 && git add crates/store/tests/migration_checksums.rs && git commit -q -m "store: pin migration 0008's checksum" && git log -1 --format='%h %s'
import re, sys
h = sys.argv[1]
assert re.fullmatch(r"[0-9a-f]{96}", h), "not a 96-char hex checksum"
p = "crates/store/tests/migration_checksums.rs"
raw = open(p, newline="").read()
s = raw.replace("\r\n", "\n")
old = "    ),\n];\n"
assert s.count(old) == 1 and "        8,\n" not in s, "APPLIED already has 8 or changed shape"
s = s.replace(old, '    ),\n    (\n        8,\n        "%s",\n    ),\n];\n' % h, 1)
open(p, "w", newline="").write(s.replace("\n", "\r\n") if "\r\n" in raw else s)
print("pinned 8")
PY
```
→ expected: `hex_len 96`, `pinned 8`, `test result: ok. 2 passed; 0 failed`, then `<sha> store: pin migration 0008's checksum`.
if not: `2 passed` missing = the local 0008 differs from production (CRLF? edit?): `git checkout -- crates/store/tests/migration_checksums.rs` and report.

39. Query the archive on the PC with DuckDB (copy a day first; prices and sizes are micro-units).
```bash
DAY='<YYYY-MM-DD>'; mkdir -p "$HOME/archive" && scp -r -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123:/var/lib/docker/volumes/cute-trading_archive/_data/$DAY "$HOME/archive/" && cd "$HOME" && duckdb <<'SQL'
-- 1. one token's top of book (every copied day)
WITH tok AS (SELECT regexp_extract(filename, '(\d{4}-\d{2}-\d{2})', 1) AS day, token_idx, token_id FROM read_parquet('archive/*/tokens.parquet', filename = true)),
     tob AS (SELECT regexp_extract(filename, '(\d{4}-\d{2}-\d{2})', 1) AS day, * FROM read_parquet('archive/*/tob.parquet', filename = true))
SELECT to_timestamp(tob.ts_ms / 1000) AS t, tob.bid / 1e6 AS bid, tob.bid_sz / 1e6 AS bid_sz, tob.ask / 1e6 AS ask, tob.ask_sz / 1e6 AS ask_sz
FROM tob JOIN tok USING (day, token_idx) WHERE tok.token_id = '<TOKEN_ID>' ORDER BY tob.day, tob.recv_ms LIMIT 20;
-- 2. the l2 book at an episode's peak (<PEAK_MS> = episodes.peak_server_ms)
WITH l AS (SELECT * FROM read_parquet('archive/<YYYY-MM-DD>/l2.parquet', file_row_number = true) WHERE episode_id = '<EPISODE_ID>' AND ts_ms <= <PEAK_MS>),
     s AS (SELECT token_idx, max(recv_ms) AS snap_recv FROM l WHERE snapshot GROUP BY token_idx),
     b AS (SELECT l.token_idx, min(l.file_row_number) AS from_row FROM l JOIN s USING (token_idx) WHERE l.snapshot AND l.recv_ms = s.snap_recv GROUP BY l.token_idx),
     book AS (SELECT l.token_idx, l.side, l.price, arg_max(l.new_size, l.file_row_number) AS size FROM l JOIN b USING (token_idx) WHERE l.file_row_number >= b.from_row GROUP BY ALL)
SELECT t.token_id, CASE book.side WHEN 0 THEN 'bid' ELSE 'ask' END AS side, book.price / 1e6 AS price, book.size / 1e6 AS size
FROM book JOIN read_parquet('archive/<YYYY-MM-DD>/tokens.parquet') t USING (token_idx) WHERE book.size > 0 ORDER BY t.token_id, side, price DESC;
-- 3. the day's gaps ('all' = the whole feed)
SELECT to_timestamp(start_ms / 1000) AS start, (end_ms - start_ms) / 1000 AS secs, coalesce(CAST(shard AS VARCHAR), 'all') AS shard, reason
FROM read_parquet('archive/<YYYY-MM-DD>/gaps.parquet') ORDER BY start_ms;
SQL
```
→ expected: three result tables (20 top-of-book rows, the book's levels, the gaps with their reasons).
if not: `duckdb` missing: install the DuckDB CLI on the PC (a standalone binary, not a crate).

## On the VPS: planned reboot test (optional, last)

**STOP** here unless steps 31–37 are green; the reboot costs 1–2 min of capture.

40. Reboot.
```bash
sudo reboot
```
→ expected: the SSH session closes. Reconnect after ~1 min.
if not: nothing; wait and reconnect.

41. Everything came back by itself (scanner via the unit, the rest via `unless-stopped`), and postgres was not recreated.
```bash
cd /home/isoca/projects/cute-trading && systemctl status isoca-radar-scanner.service --no-pager | grep -E 'Active|status=' ; journalctl -b -u isoca-radar-scanner --no-pager | grep -c 'postgres-1.*Recreate'; docker compose -f compose.yml -f compose.web.yml --profile core --profile web ps --format '{{.Service}} {{.Status}}'
```
→ expected: `Active: active (exited)` and `status=0/SUCCESS`, `0`, then `postgres`, `scanner`, `analyze`, `compact`, `api`, `web`, `oauth2-proxy` all `Up <n> minutes`.
if not: `journalctl -u isoca-radar-scanner --no-pager | tail -20` and stop here.

42. A new live scanner run, capture flowing.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, version, mode, exit_reason FROM runs WHERE service = 'scanner' ORDER BY run_id DESC LIMIT 1" && sleep 90 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), msgs, lag_p50_ms, clock_offset_ms FROM health ORDER BY minute_ms DESC LIMIT 2"
```
→ expected: a new run on `GIT_SHA`, `live`, empty exit reason; two health rows from the last minutes, msgs > 0, lag p50 ~75, clock offset within ±250.
if not: `docker compose --profile core logs --tail 30 scanner`.

43. The compactor resumed from its checkpoint.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core logs --since 10m compact 2>&1 | grep -E 'every 60 s|refusing|starting cold|done in' | head -5 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(updated_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), lag_hours, last_error FROM compact_status"
```
→ expected: `compact: every 60 s, …`, no `refusing` and no `starting cold` line; then `HH:MI|1|` or `|2|` updated within the last minute.
if not: `docker compose --profile core logs --tail 30 compact` and report.

## Rollback (only if a step above says so)

44. Stop the compactor only (the scanner returns to the age + cap rule by itself once `compact_status` is 6 h old).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop compact && docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "ALTER ROLE compact_rw NOLOGIN"
```
→ expected: `Stopped`, then `ALTER ROLE`; it stays stopped across reboots (`unless-stopped`); within 6 h the scanner logs `capture: compactor silent …`.
if not: `docker compose --profile core ps compact` must show it exited; run the stop again.

45. Full rollback: the `:pre5b` scanner, analyze, api and web images (0008 stays: additive, unused by the old code), and the boot unit removed if step 30 installed it (the old `migrate` refuses the applied 0008, so the unit could not start this scanner).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop compact scanner analyze && docker compose --profile core rm -sf compact && docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "DO \$\$ BEGIN IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'compact_rw') THEN ALTER ROLE compact_rw NOLOGIN; END IF; END \$\$" && docker tag cute-trading/rust:dev cute-trading/rust:5b && docker tag cute-trading/web:dev cute-trading/web:5b && docker tag cute-trading/rust:pre5b cute-trading/rust:dev && docker tag cute-trading/web:pre5b cute-trading/web:dev && docker compose --profile core up -d --no-build --no-deps scanner analyze && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web && { [ ! -f /etc/systemd/system/isoca-radar-scanner.service ] || { systemctl disable --now isoca-radar-scanner.service && rm /etc/systemd/system/isoca-radar-scanner.service && systemctl daemon-reload && echo unit-removed; }; } && sleep 20 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT version FROM runs WHERE service = 'scanner' ORDER BY run_id DESC LIMIT 1"
```
→ expected: `Container cute-trading-compact-1  Removed` (the old image has no compact binary), `DO`, `unit-removed` (if step 30 ran), then the pre-5b version (sha). The scanner no longer starts at boot: after any reboot run `docker compose --profile core up -d --no-build --no-deps scanner` (without `--no-deps` the old `migrate` fails).
if not: the database is unusable (should not happen, 0008 is additive): ask the owner before `pg_restore` of `/root/ct-pre-5b.dump`.

## Results

- `GIT_SHA`: · checksum test (step 3): · DB tests (step 4):
- Benchmark line (step 11): · agreement counts (step 12): · decision:
- Disk headroom (step 15) `size` / `other` / budget change:
- postgres recreated (step 21): · migrate exit / version / grants (steps 22–23): · role (step 24):
- raw hours and catch-up bound (step 27): · lag ≤ 2 reached at (step 31):
- Websockets across the api/web restart (steps 28–29): reconnects · reconnect gaps · scanner restarted?
- First `done` hour (step 32): · status (step 31): · docker stats (step 33):
- Lost since deploy (step 34): · raw size at +24 h (step 35):
- First merged day, `verify`, bytes per table, `ts_clamped`, `late_rows` (steps 36–37):
- 0008 checksum pinned (step 38): · date:
- Reboot test (steps 40–43): all back? · postgres recreated? · scanner run · compact resumed:
