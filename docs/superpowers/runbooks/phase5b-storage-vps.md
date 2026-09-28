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

4. The Postgres-backed tests (store, compact service, api) really run against the PC Postgres on 127.0.0.1:15432 (password read from `.env`, never printed).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && (exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && ( set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1; cargo test -p store -p compact -p api -- --nocapture 2>&1 | grep -E '^test result|SKIPPED|panicked' | sort | uniq -c; echo "cargo exit=${PIPESTATUS[0]}" )
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

8. The compactor and its config are in the image (`verify` on a day that doesn't exist loads the config, then fails on the missing day).
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/compact cute-trading/rust:dev --help | head -1; MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/compact cute-trading/rust:dev verify 2000-01-01
```
→ expected: `raw capture hours → the Parquet archive (paper trading only)`, then `compact: read /archive/2000-01-01/manifest.json: No such file or directory (os error 2)`.
if not: a line naming `compact.toml` = the config isn't in the image; no first line = step 7 built from the wrong tree; rebuild.

9. (Benchmark, on the VPS) Copy the newest closed raw hour and the universe records its control lines name.
```bash
C=/var/lib/docker/volumes/cute-trading_data/_data/capture; F=$(ls $C/*/[0-9][0-9].jsonl.zst | tail -2 | head -1); D=$(basename "$(dirname "$F")"); command -v zstdcat >/dev/null || apt-get install -y zstd; mkdir -p ~/bench/$D && cp "$F" ~/bench/$D/ && for f in $(zstdcat "$F" | grep '"kind":"universe"' | grep -o 'universe-[0-9]*-[0-9]*\.json\.zst' | sort -u); do t=${f#universe-}; t=${t%%-*}; d=$(date -u -d @$((t / 1000)) +%F); mkdir -p ~/bench/$d && cp $C/$d/$f ~/bench/$d/; done; tar -cf ~/bench.tar -C ~ bench && rm -rf ~/bench && echo "$F" && du -h ~/bench.tar
```
→ expected: the hour's path `…/capture/<YYYY-MM-DD>/<HH>.jsonl.zst`, then `<size>  /root/bench.tar` (about the size of one raw hour, ~1 GB).
if not: skip steps 9–12; the spec §3 defaults stay (write "no benchmark" in Results).

10. (Benchmark, on the PC) Fetch and unpack it, then delete it on the VPS.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123:/root/bench.tar "$HOME/bench.tar" && rm -rf "$HOME/bench" && tar -xf "$HOME/bench.tar" -C "$HOME" && rm "$HOME/bench.tar" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 rm /root/bench.tar && (cd "$HOME/bench" && ls */*.jsonl.zst)
```
→ expected: one line `<YYYY-MM-DD>/<HH>.jsonl.zst`.
if not: check the scp line's key path (quoted, forward slashes).

11. (Benchmark, on the PC) The release compactor on that hour under the service's limits (output to a throwaway volume, not memory).
```bash
H=$(cd "$HOME/bench" && ls */*.jsonl.zst) && MSYS_NO_PATHCONV=1 docker run --rm --cpus 0.25 --memory 256m -v "$(cygpath -m "$HOME/bench"):/bench:ro" -v ct-bench-out:/archive --entrypoint /usr/local/bin/compact cute-trading/rust:dev bench "/bench/$H" --out /archive/bench; docker volume rm ct-bench-out
```
→ expected: one line `bench: <day>/<HH>.jsonl.zst <ms> ms, lines=… bad_lines=0 universe_missing=0 late_rows=…, tob=… depth=… gaps=… tokens=…, bytes tob=… depth=… gaps=… tokens=…, per_day_bytes=<n> (l2 not measured: no episodes), peak_rss_kb=<n>` with `<ms>` < 600000 and `peak_rss_kb` < 200000, then `ct-bench-out`; write the line in Results.
if not: over time or memory (or exit 137): **STOP**, do not deploy, report the line; `per_day_bytes` + 50 MB over 300 MB with depth over 50 MB a day: set `depth_every_secs = 600` in `config/compact.toml`, commit, and redo steps 7–11.

12. (Benchmark, on the PC) Both agreement tests on the real hour (the compactor's books and tob rows equal the scanner engine's).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && TAPE_AGREEMENT_DIR="$(cygpath -m "$HOME/bench")" cargo test -p scanner --test tape_agreement --test tob_agreement -- --ignored --nocapture 2>&1 | grep -E 'tape agreement|tob agreement|test result|SKIPPED'
```
→ expected: `tape agreement: <n> lines, books equal after every one`, `tob agreement: <n> rows equal the engine's top`, two `test result: ok. 1 passed`, no `SKIPPED`; write both counts in Results, then `rm -rf "$HOME/bench"`.
if not: **STOP**, do not deploy; the failing line and token go to the plan owner.

13. Bundle the images, compose files, `.env.example`, roles script and disk-budget script.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-5b/docker/postgres/init" "$HOME/ct-5b/scripts" && cd "$HOME/ct-5b" && docker save cute-trading/rust:dev cute-trading/web:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && cp "$R/docker/postgres/init/01-roles.sh" docker/postgres/init/ && cp "$R/scripts/disk-budget.sh" scripts/ && tar -cf "$HOME/ct-5b.tar" image.tar.gz compose.yml compose.web.yml .env.example docker scripts && cd "$HOME" && rm -rf "$HOME/ct-5b" && ls -l "$HOME/ct-5b.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-5b.tar` (about 1 GB).
if not: free disk (step 6) and run this step again.

14. Ship the bundle to the VPS.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-5b.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-5b.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

15. Disk headroom: with the project at its 75 GB ceiling the disk must keep ≥ 20 GB free (capture pauses below 10 GB free).
```bash
cd /home/isoca/projects/cute-trading && df -BG --output=size,used,avail /var/lib/docker | tail -1 && du -s --block-size=1G /var/lib/docker/volumes/cute-trading_* | awk '{s += $1} END {print "project_gb", s}'
```
→ expected: `<size>G <used>G <avail>G` then `project_gb <p>`; with `other = used − p`, `size − other − 75 ≥ 20`.
if not: set `archive_budget_gb = size − other − 36` (rounded down, ≥ 1) in `config/compact.toml` on the PC, commit, redo steps 7–14.
**STOP**

16. Rollback tags for the running images, and copies of the compose files and `.env`.
```bash
cd /home/isoca/projects/cute-trading && docker tag "$(docker inspect -f '{{.Image}}' "$(docker compose --profile core ps -q scanner)")" cute-trading/rust:pre5b && docker tag "$(docker inspect -f '{{.Image}}' "$(docker compose -f compose.yml -f compose.web.yml --profile web ps -q web)")" cute-trading/web:pre5b && cp compose.yml /root/compose.yml.pre5b && cp compose.web.yml /root/compose.web.yml.pre5b && cp .env .env.bak-5b && chmod 600 .env.bak-5b && docker image ls 'cute-trading/*' --format '{{.Repository}}:{{.Tag}} {{.ID}}'
```
→ expected: `cute-trading/rust:pre5b` on the same ID as `cute-trading/rust:dev`, `cute-trading/web:pre5b` on the same ID as `cute-trading/web:dev`.
if not: a service is not running; tag from `docker image ls` by the ID it last ran, then continue.

17. Back up the database.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres pg_dump -U ct_owner -d cute_trading -Fc > /root/ct-pre-5b.dump && ls -lh /root/ct-pre-5b.dump
```
→ expected: one `ls` line, a few hundred MB.
if not: free space and run this step again; do not migrate without it.
**STOP**

18. Load the bundle (replaces compose.yml, compose.web.yml, .env.example, the roles script and disk-budget.sh).
```bash
cd /home/isoca/projects/cute-trading && tar -xf /root/ct-5b.tar && rm /root/ct-5b.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls 'cute-trading/*' --format '{{.Repository}}:{{.Tag}} {{.ID}}'
```
→ expected: `Loaded image: cute-trading/rust:dev` and `Loaded image: cute-trading/web:dev`; `:dev` now on new IDs, `:pre5b` on the old ones.
if not: the tar was truncated; re-ship (step 14).

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

21. Disk budget of the project before migrating.
```bash
cd /home/isoca/projects/cute-trading && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: the rows, then `total <x> GB of 75 GB` and no `OVER BUDGET` line.
if not: free space (old images: `docker image prune -f`) and run again.

22. Stop the scanner and analyze, then migrate ALONE (capture pauses until step 25).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 scanner analyze && docker compose --profile core run --rm migrate; echo "migrate exit=$?"
```
→ expected: `migrate exit=0`.
if not: `docker compose --profile core logs --tail 50 migrate`; start nothing; a checksum error means a CRLF image: redo steps 2–18.
**STOP**

23. Migration 8 is applied and the grants are right.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT max(version) FROM _sqlx_migrations WHERE success" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT has_table_privilege('compact_rw','compact_hours','UPDATE'), has_table_privilege('compact_rw','episodes','INSERT'), has_table_privilege('scanner_rw','compact_hours','INSERT'), has_table_privilege('scanner_rw','compact_hours','UPDATE'), has_table_privilege('analyze_ro','compact_status','SELECT'), has_table_privilege('scanner_rw','compact_status','SELECT')"
```
→ expected: `8` then `t|f|t|f|t|t`.
if not: start nothing; full rollback (step 49).
**STOP**

24. Activate `compact_rw` with the `.env` password: pgdata already exists, so the init script never runs again and 0008 left the role NOLOGIN (password never printed).
```bash
cd /home/isoca/projects/cute-trading && PW="$(grep -E '^COMPACT_RW_DB_PASSWORD=' .env | cut -d= -f2-)"; case "$PW" in ''|*[!0-9a-f]*) echo "pw-bad"; PW= ;; *) echo pw-read ;; esac; [ -n "$PW" ] && printf '\\set pw %s\nSET log_min_error_statement = panic;\nALTER ROLE compact_rw LOGIN PASSWORD :'"'"'pw'"'"';\n' "$PW" | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -q -U ct_owner -d cute_trading && echo role-set; unset PW; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'compact_rw'"
```
→ expected: `pw-read`, `role-set`, `t`.
if not: `pw-bad` means the `.env` line isn't an `openssl rand -hex 24` value: delete it with `sed -i '/^COMPACT_RW_DB_PASSWORD=/d' .env`, redo step 20, run again.
**STOP**

25. Start the new scanner (new retention, delete guard, universe dedup) and note the deploy time.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/5b_deploy_ms && docker compose --profile core up -d --no-build --no-deps scanner && sleep 60 && docker compose --profile core logs --since 70s scanner 2>&1 | grep -E 'capture:|error|panicked' | head
```
→ expected: `capture: compactor silent (no compact_status within 6 h); retention falls back to age + cap` and no `error`/`panicked` lines.
if not: full rollback (step 49).

26. Start the new analyze (now `restart: unless-stopped`) and check every restart policy.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps analyze && sleep 20 && docker inspect -f '{{.Name}} {{.HostConfig.RestartPolicy.Name}} {{.State.Status}}' cute-trading-analyze-1 cute-trading-scanner-1 && docker compose --profile core logs --since 30s analyze 2>&1 | grep -c -E 'error|panicked'
```
→ expected: `/cute-trading-analyze-1 unless-stopped running`, `/cute-trading-scanner-1 on-failure running`, then `0`.
if not: `docker compose --profile core logs --tail 30 analyze`.

27. Start the compactor. It starts cold at 00:00 of the first raw hour's day: hours before that file get `scanner_stopped` sidecars in seconds, then every raw hour on disk is compacted in order.
```bash
BENCH_MS=600000   # step 11's <ms>; keep 600000 without a benchmark
cd /home/isoca/projects/cute-trading && C=/var/lib/docker/volumes/cute-trading_data/_data/capture; N=$(ls $C/*/[0-9][0-9].jsonl.zst | wc -l); echo "raw_hours $N first $(ls $C/*/[0-9][0-9].jsonl.zst | head -1 | sed 's#.*/capture/##')"; echo "catch-up bound $(( N * BENCH_MS * 3600000 / (3600000 - BENCH_MS) / 60000 + 10 )) min from $(date -u +%H:%M) UTC" | tee /root/5b_catchup; docker compose --profile core up -d --no-build --no-deps compact && sleep 90 && docker compose --profile core logs --since 100s compact 2>&1 | head -8
```
→ expected: `raw_hours <N> first <YYYY-MM-DD>/<HH>.jsonl.zst`, `catch-up bound <m> min from HH:MM UTC`, then `compact: every 60 s, /data/capture → /archive (writes compact_hours/compact_status only, paper only)`, `compact: no checkpoint: starting cold at <YYYY-MM-DD>T00 (first raw hour <YYYY-MM-DDTHH>)`, `compact: <H> h behind (lag_alarm_hours 3): raw hours are at risk`, and `compact: <YYYY-MM-DDTHH> done in <ms> ms, tob=… depth=… l2=… gaps=…, <n> B` lines (fast, `tob=0` for the hours before the first file). During the catch-up the Radar's **compactor behind: raw at risk** heart is lit; it must go out within the bound.
if not: `database unreachable` = role or password: redo step 24, then `docker compose --profile core restart compact`; `refusing to start: another …` = something else holds the archive lock (never start a second `compact run` by hand); `refusing to start: the database role can write …` = wrong role in `DATABASE_URL`: full rollback (step 49).

28. Scanner websockets before the api/web restart (baseline; the restart must not touch them).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/5b_web_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/5b_scanner_started && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs, lag_p50_ms FROM health ORDER BY minute_ms DESC LIMIT 3"
```
→ expected: the scanner's start time (step 25), then three `HH:MI|<shards>|0|0|<msgs>|<lag>` rows from the last minutes (msgs > 0, lag p50 ~75).
if not: reconnects already > 0: note the rows and go on (the baseline is what step 30 compares with).

29. Restart api and web (health fields, Archive box).
```bash
cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web && docker compose -f compose.yml -f compose.web.yml --profile web ps api web --format '{{.Service}} {{.Status}}'
```
→ expected: `Container cute-trading-api-1 …` and `Container cute-trading-web-1 …` lines only (no network, no other container), then `api Up …` and `web Up …`.
if not: `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 api web`.

30. Scanner websockets after the api/web restart: no disconnect.
```bash
cd /home/isoca/projects/cute-trading && sleep 180; T0=$(cat /root/5b_web_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/5b_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs, lag_p50_ms FROM health WHERE minute_ms >= $T0 - 60000 ORDER BY minute_ms" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"
```
→ expected: `scanner-not-restarted`, 3–4 health rows all `…|0|0|…` (reconnects 0, parked 0, msgs steady), then `0` reconnect gaps.
if not: do NOT roll back; write in Results the step-28 time (`date -u -d @$(( $(cat /root/5b_web_ms) / 1000 ))`), the `HH:MI` rows with reconnects > 0 and the reconnect count, for investigation.

31. The scanner now waits for the compactor.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core logs --since 15m scanner 2>&1 | grep 'capture: compactor' | tail -1
```
→ expected: `capture: compactor reporting; raw hours wait for compaction`.
if not: check `compact_status` (step 36); the scanner keeps the age + cap rule meanwhile, nothing is lost.

32. Docker path the boot unit uses.
```bash
command -v docker
```
→ expected: `/usr/bin/docker`.
if not: use the printed path in `ExecStart` in step 33.

33. Install the scanner boot unit (starts the scanner at boot; the scanner keeps `on-failure:5` because safe mode exits 0 on purpose).
```bash
cat > /etc/systemd/system/isoca-radar-scanner.service <<'EOF'
[Unit]
Description=isoca-radar: start the scanner at boot
Requires=docker.service
After=docker.service network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/home/isoca/projects/cute-trading
ExecStart=/usr/bin/docker compose --profile core up -d --no-build scanner
TimeoutStartSec=300

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload && systemctl enable isoca-radar-scanner.service && systemctl is-enabled isoca-radar-scanner.service
```
→ expected: `Created symlink /etc/systemd/system/multi-user.target.wants/isoca-radar-scanner.service → …` then `enabled`.
if not: `systemctl status isoca-radar-scanner.service --no-pager`.

## On the VPS: checks

34. Catch-up progress (run any time after step 27): the lag falls every few minutes until 1–2.
```bash
cd /home/isoca/projects/cute-trading && cat /root/5b_catchup && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(updated_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), lag_hours, last_error FROM compact_status" && docker compose --profile core logs --since 10m compact 2>&1 | grep -c ' done in '
```
→ expected: the bound from step 27, then `HH:MI|<lag>|` (updated within the last minutes, empty last_error; lag > 3 is normal before the bound, 1 or 2 after it), then a count > 0 while catching up.
if not: lag still > 3 after the bound, or a last_error: `docker compose --profile core logs --tail 30 compact` and report; never start a second `compact run`.

35. First compaction check (after the bound): hours become `done` 1–2 h after they close.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(hour_ms/1000) AT TIME ZONE 'UTC','MM-DD HH24'), status, rows FROM compact_hours ORDER BY hour_ms DESC LIMIT 3"
```
→ expected: three `MM-DD HH|done|{"tob": …}` lines, the newest 1–2 hours before now (UTC).
if not: `docker compose --profile core logs --tail 30 compact`; do not leave it running with errors: stop the compactor (step 48).
**STOP**

36. Status row, lag and errors.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(updated_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), archive_bytes, oldest_day, lag_hours, last_error FROM compact_status"
```
→ expected: `HH:MI|<bytes>|<YYYY-MM-DD>|1|` or `…|2|` (empty last_error), updated within the last minute; the Radar's compactor heart is back.
if not: the last_error text says what failed; lag above 3 after the catch-up costs a heart on Radar.

37. Container limits hold while it works.
```bash
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}' | grep compact
```
→ expected: `cute-trading-compact-1 <≤ 25%> <under 256MiB> / 256MiB`; write it in Results.
if not: memory near the limit: report the line with step 11's `peak_rss_kb`.

38. No hour lost at the cap since the deploy.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM compact_hours WHERE status = 'lost' AND done_ms > $(cat /root/5b_deploy_ms)"; docker compose --profile core logs scanner 2>&1 | grep -c 'deleted before compaction'
```
→ expected: `0` then `0`.
if not: the compactor is too slow for the 15 GB cap: report the lag (step 36) and `docker stats` (step 37).

39. Raw capture stays under its cap (run 24 h after step 25).
```bash
du -sh /var/lib/docker/volumes/cute-trading_data/_data/capture && ls /var/lib/docker/volumes/cute-trading_data/_data/capture/$(date -u +%F) | grep -c universe-
```
→ expected: under 15G (never over), then a count far below 12 per hour elapsed today.
if not: over 15G means the cap isn't enforced: report the scanner log's `capture:` lines.

40. The first merged day (a day merges once its hour 23 is compacted, ~01:00–02:00 UTC the next day; the first day starts with `scanner_stopped` hours).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core logs compact 2>&1 | grep 'compact: merged' | tail -3
```
→ expected: `compact: merged <YYYY-MM-DD>, tob=… depth=… l2=… gaps=…, <n> B`.
if not: none yet: run again after 02:00 UTC; a `day … not merged` in last_error (step 36): report it.

41. Verify that day, in a separate one-off container (it reads the image's `/etc/cute-trading/compact.toml`; `verify` takes no lock, the service keeps running).
```bash
cd /home/isoca/projects/cute-trading && DAY='<YYYY-MM-DD>'; docker compose --profile core run --rm --no-deps -T compact verify "$DAY"; echo "verify exit=$?"; python3 -c "import json,sys; m=json.load(open(sys.argv[1])); print('tob', m['totals']['tob'], 'ts_clamped', m['ts_clamped'], 'late_rows', m['late_rows'], 'bytes', m['bytes'])" /var/lib/docker/volumes/cute-trading_archive/_data/$DAY/manifest.json
```
→ expected: `verify <day>: ok`, `verify exit=0`, then one line with `ts_clamped` and `late_rows` small next to `tob`, and the bytes per table; write them in Results.
if not: `verify exit=137` = out of memory (report it, the archive is fine); listed problems: **STOP** compaction (step 48) and report every one.

42. (On the PC) Pin 0008's production checksum in the checksum test and commit it.
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

43. Query the archive on the PC with DuckDB (copy a day first; prices and sizes are micro-units).
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

**STOP** here unless steps 34–41 are green; the reboot costs 1–2 min of capture.

44. Reboot.
```bash
sudo reboot
```
→ expected: the SSH session closes. Reconnect after ~1 min.
if not: nothing; wait and reconnect.

45. Everything came back by itself: the scanner through the boot unit, postgres, analyze, compact, api, web and oauth2-proxy through `unless-stopped`.
```bash
cd /home/isoca/projects/cute-trading && systemctl status isoca-radar-scanner.service --no-pager | grep -E 'Active|status=' ; docker compose -f compose.yml -f compose.web.yml --profile core --profile web ps --format '{{.Service}} {{.Status}}'
```
→ expected: `Active: active (exited)` and `status=0/SUCCESS`; `postgres`, `scanner`, `analyze`, `compact`, `api`, `web`, `oauth2-proxy` all `Up <n> minutes`.
if not: `journalctl -u isoca-radar-scanner --no-pager | tail -20` and stop here.

46. A new live scanner run, capture flowing.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, version, mode, exit_reason FROM runs WHERE service = 'scanner' ORDER BY run_id DESC LIMIT 1" && sleep 90 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), msgs, lag_p50_ms, clock_offset_ms FROM health ORDER BY minute_ms DESC LIMIT 2"
```
→ expected: a new run on `GIT_SHA`, `live`, empty exit reason; two health rows from the last minutes, msgs > 0, lag p50 ~75, clock offset within ±250.
if not: `docker compose --profile core logs --tail 30 scanner`.

47. The compactor resumed from its checkpoint (the previous process's lock died with it).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core logs --since 10m compact 2>&1 | grep -E 'every 60 s|refusing|unreachable|done in' | head -5 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(updated_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), lag_hours, last_error FROM compact_status"
```
→ expected: `compact: every 60 s, …` (possibly after `database unreachable, retrying next poll` lines while postgres started), no `refusing` line, no `no checkpoint: starting cold`; then `HH:MI|1|` or `|2|` updated within the last minute.
if not: `docker compose --profile core logs --tail 30 compact` and report.

## Rollback (only if a step above says so)

48. Stop the compactor (the scanner returns to the age + cap rule by itself once `compact_status` is 6 h old).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop compact && docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "ALTER ROLE compact_rw NOLOGIN"
```
→ expected: `Stopped`, then `ALTER ROLE`; it stays stopped across reboots (`unless-stopped`); within 6 h the scanner logs `capture: compactor silent …; retention falls back to age + cap`.
if not: `docker compose --profile core ps compact` must show it exited; run the stop again.

49. Full rollback: the previous scanner, analyze, api and web images (0008 stays: additive, unused by the old code).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop compact scanner analyze && docker tag cute-trading/rust:dev cute-trading/rust:5b && docker tag cute-trading/web:dev cute-trading/web:5b && docker tag cute-trading/rust:pre5b cute-trading/rust:dev && docker tag cute-trading/web:pre5b cute-trading/web:dev && docker compose --profile core up -d --no-build --no-deps scanner analyze && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT version FROM runs WHERE service = 'scanner' ORDER BY run_id DESC LIMIT 1"
```
→ expected: the newest scanner run on the pre-5b version (sha); then run step 50 too (the old image's `migrate` refuses the applied 0008, so the boot unit could not start this scanner).
if not: the database is unusable (should not happen, 0008 is additive): ask the owner before `pg_restore` of `/root/ct-pre-5b.dump`.

50. Remove the boot unit.
```bash
systemctl disable --now isoca-radar-scanner.service && rm /etc/systemd/system/isoca-radar-scanner.service && systemctl daemon-reload && echo unit-removed
```
→ expected: `Removed …` then `unit-removed`; the running scanner is untouched (the unit is a oneshot).
if not: `systemctl status isoca-radar-scanner.service --no-pager`.

Keep the scanner down across a planned reboot (maintenance, not a rollback): `systemctl disable isoca-radar-scanner.service`, and `systemctl enable isoca-radar-scanner.service` after.

## Results

- `GIT_SHA`: · checksum test (step 3): · DB tests (step 4):
- Benchmark line (step 11): · agreement counts (step 12): · decision:
- Disk headroom (step 15) `size` / `other` / budget change:
- migrate exit / version / grants (steps 22–23): · role (step 24):
- raw hours and catch-up bound (step 27): · lag ≤ 2 reached at (step 34):
- Websockets across the api/web restart (steps 28–30): reconnects · reconnect gaps · scanner restarted?
- First `done` hour and its lag (step 35): · status (step 36): · docker stats (step 37):
- Lost since deploy (step 38): · raw size at +24 h (step 39):
- First merged day, `verify`, bytes per table, `ts_clamped`, `late_rows` (steps 40–41):
- 0008 checksum pinned (step 42): · date:
- Reboot test (steps 44–47): all back? · scanner run · compact resumed:
