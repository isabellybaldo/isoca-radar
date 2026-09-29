# Phase 7a step A on the VPS: latency ladder, paper epochs and the reset route (backend + api)

Note: one deploy: migration 0009 (paper_epochs, paper_reset_requests, paper_reset_rw, a paper_ledger index), the scanner with the ladder policies (epoch 1), analyze, compact on the 7a image, the api with its reset pool; the web is unchanged (step B, plan 7a-B).
Note: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker.
Note: PC steps in Git Bash from the main checkout on the merged commit; VPS steps as root over SSH.
Note: VPS order is fixed: `.env` key (step 14) → migrate (step 20) → role login (step 22) → compact on 7a (step 27) → api (step 28); `compose.web.yml` refuses to parse without `RESET_RW_DB_PASSWORD`, and an api started while `paper_reset_rw` is NOLOGIN crash-loops (dashboard down).

## On the PC

1. The repo on the commit being deployed; the tree must be clean.
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
if not: `git checkout -- <each listed file>` and run this step again.
**STOP**

3. Migration checksums: 0001–0008 must match production.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p store --test migration_checksums 2>&1 | tail -3
```
→ expected: `test result: ok. 2 passed; 0 failed`.
if not: a CRLF or edited migration: fix it as in step 2 and run again.
**STOP**

4. The Postgres-backed tests really run (store, api, analyze, scanner).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && (exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && ( set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1; cargo test -p store -p api -p analyze -p scanner -- --nocapture 2>&1 | grep -E '^test result|SKIPPED|panicked' | sort | uniq -c; echo "cargo exit=${PIPESTATUS[0]}" )
```
→ expected: `pg-up`, only `<n> test result: ok. …; 0 failed …` lines, then `cargo exit=0`.
if not: no `pg-up` = start the PC Postgres first; any `SKIPPED`/`panicked`/`FAILED`: **STOP**.
**STOP**

5. The new `.env` key on the PC (the value is never printed).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && (grep -q '^RESET_RW_DB_PASSWORD=' .env || printf 'RESET_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env) && grep -c -E '^(ANALYZE_RW_DB_PASSWORD|COMPACT_RW_DB_PASSWORD|RESET_RW_DB_PASSWORD)=' .env
```
→ expected: `3`.
if not: add the missing key the same way, never with an editor that echoes it.

6. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `cargo clean` and `docker builder prune -f --keep-storage 5GB`, then run this step again.

7. Build the Rust image (scanner, api, analyze, compact). The web image is unchanged.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA" && docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate 2>&1 | tail -2
```
→ expected: the short sha, then the build's last lines without `ERROR`.
if not: a `CRLF found` failure means step 2 was skipped; read the first `ERROR` line, fix, run again.
**STOP**

8. The image carries the ladder policies (19 accounts) and the new migration.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/scanner cute-trading/rust:dev --help | head -1; docker create --name ct-7a-peek cute-trading/rust:dev >/dev/null && docker cp ct-7a-peek:/etc/cute-trading/policies.toml - | tar -xO | grep -c '^\[\[accounts\]\]'; docker rm ct-7a-peek >/dev/null
```
→ expected: `read-only Polymarket arbitrage scanner (paper trading only)`, then `19`.
if not: `15` = the image was built from an old tree: redo step 7 from the deployed commit.

9. (Cost gate, spec §3) The busy raw hour from the VPS: kept only if `$HOME/probe/cap` holds a `.jsonl.zst`, refetched otherwise.
```bash
K="$(find "$HOME/probe/cap" -name '*.jsonl.zst' 2>/dev/null | head -1)"; if [ -n "$K" ]; then echo "$K" && echo probe-kept; else rm -rf "$HOME/probe" && mkdir -p "$HOME/probe/cap" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 'command -v zstdcat >/dev/null || apt-get install -y zstd >/dev/null; cd /var/lib/docker/volumes/cute-trading_data/_data/capture && F=$(find . -name "[0-9][0-9].jsonl.zst" -mmin +70 -mmin -2880 -printf "%s %p\n" | sort -n | tail -1 | cut -d" " -f2) && U=$(for u in $(zstdcat "$F" | grep -o "universe-[0-9]*-[0-9]*\.json\.zst" | sort -u); do find . -name "$u"; done) && echo "busy hour: $F" >&2 && tar -cf - "$F" $U' | tar -xf - -C "$HOME/probe/cap" && find "$HOME/probe/cap" -type f | sort; fi; cd /c/Users/Isabelly/PycharmProjects/cute-trading && git log --format=%h -1 -- config/policies.toml && git show "$(git log --format=%h -1 -- config/policies.toml)~1:config/policies.toml" > "$HOME/probe/policies-old.toml" && grep -c '^\[\[accounts\]\]' "$HOME/probe/policies-old.toml"
```
→ expected: a `.jsonl.zst` path and `probe-kept` (or `busy hour: …` and the fetched files), then the ladder commit's short sha, then `15`.
if not: `busy hour:` empty = no closed hour in 2 days: drop `-mmin -2880` and run again; no `.jsonl.zst` among the fetched files = run again.

10. (Cost gate) Replay the hour twice with and twice without the ladder on the image just built.
```bash
P="$(cygpath -m "$HOME/probe")"; for i in 1 2; do for v in old new; do if [ "$v" = old ]; then M="-v $P/policies-old.toml:/etc/cute-trading/policies.toml:ro"; else M=""; fi; printf '%s ' "$v"; MSYS_NO_PATHCONV=1 docker run --rm -v "$P/cap:/probe/cap:ro" $M --entrypoint /usr/local/bin/scanner cute-trading/rust:dev replay /probe/cap --dry-run 2>/dev/null | tail -1; done; done | tee "$HOME/probe/gate.txt"
```
→ expected: four lines `old|new frames <N> parse_errors <n> episodes <n> paper_rows <n> wall_ms <ms> peak_rss_kb <kb>` with the same `<N>` (> 0).
if not: an error line = a universe record is missing: `rm -rf "$HOME/probe"` and redo step 9.

11. (Cost gate) The verdict: peak RSS grows < 10 % and < 50 MB, wall time < 10 %.
```bash
python - "$HOME/probe/gate.txt" <<'PY'
import sys
rows = {"old": [], "new": []}
for line in open(sys.argv[1]):
    f = line.split()
    kv = dict(zip(f[1::2], f[2::2]))
    rows[f[0]].append((int(kv["wall_ms"]), int(kv["peak_rss_kb"]), int(kv["frames"])))
(ow, orss, of), (nw, nrss, nf) = [(min(r[0] for r in rows[k]), max(r[1] for r in rows[k]), rows[k][0][2]) for k in ("old", "new")]
assert of == nf and of > 0, f"frames differ or zero: {of} {nf}"
drss = nrss - orss
print(f"wall_ms old {ow} new {nw} (+{100 * (nw - ow) / ow:.1f} %); peak_rss_kb old {orss} new {nrss} (+{100 * drss / orss:.1f} %, +{drss // 1024} MB)")
print("GATE PASS" if nw < ow * 1.10 and drss < orss * 0.10 and drss < 50 * 1024 else "GATE FAIL")
PY
```
→ expected: the figures line, then `GATE PASS`; write both lines in Results.
if not: `GATE FAIL`: **STOP**, do not deploy; bring the figures to the owner.
**STOP**

12. Bundle the image, compose files, `.env.example` and roles script.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-7a/docker/postgres/init" && cd "$HOME/ct-7a" && docker save cute-trading/rust:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && cp "$R/docker/postgres/init/01-roles.sh" docker/postgres/init/ && tar -cf "$HOME/ct-7a.tar" image.tar.gz compose.yml compose.web.yml .env.example docker && cd "$HOME" && rm -rf "$HOME/ct-7a" && ls -l "$HOME/ct-7a.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-7a.tar`, roughly 25–60 MB.
if not: free disk (step 6) and run this step again.

13. Ship the bundle.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7a.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-7a.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

14. The new `.env` key on the VPS, before any compose command (never printed).
```bash
cd /home/isoca/projects/cute-trading && { grep -q '^RESET_RW_DB_PASSWORD=' .env || printf 'RESET_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env; } && chmod 600 .env && grep -c '^RESET_RW_DB_PASSWORD=' .env
```
→ expected: `1`.
if not: remove duplicate lines with `sed -i` on the key name (never print values), then run again.

15. Rollback tags on the current images (before step 17's load moves `:dev`).
```bash
cd /home/isoca/projects/cute-trading && docker tag cute-trading/rust:dev cute-trading/rust:pre7a && docker tag cute-trading/web:dev cute-trading/web:pre7a; docker image inspect -f '{{.Id}}' cute-trading/rust:pre7a cute-trading/web:pre7a; docker inspect -f '{{.Name}} {{.Image}}' cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1 cute-trading-api-1 cute-trading-web-1
```
→ expected: two `sha256:…` IDs, then scanner, analyze, compact and api on the first ID and web on the second.
if not: a container on another ID: `docker tag <its sha256 ID> cute-trading/rust:pre7a` and run the last two commands again until they match.
**STOP**

16. Back up the database.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres pg_dump -U ct_owner -d cute_trading -Fc > /root/ct-pre-7a.dump && ls -lh /root/ct-pre-7a.dump
```
→ expected: one `ls` line, a few hundred MB.
if not: free space and run this step again; do not migrate without it.
**STOP**

17. Load the bundle (replaces compose.yml, compose.web.yml, .env.example and the roles script).
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-7a.tar && rm /root/ct-7a.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls 'cute-trading/*' --format '{{.Repository}}:{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/rust:dev`; `rust:dev` on a new ID, `rust:pre7a` and `web:dev`/`web:pre7a` on the old ones; then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: truncated tar = re-ship (step 13); over budget = `docker image prune -f` and run the budget again.

18. Compose parses with the new keys.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c 'RESET_RW_DB_PASSWORD' compose.yml compose.web.yml && grep -c 'POLICIES_PATH' compose.web.yml
```
→ expected: `config-ok`, `compose.yml:1`, `compose.web.yml:1`, then `1`.
if not: the bundle's compose files are old: re-bundle (step 12).
**STOP**

19. Stop the scanner, analyze and compact (capture pauses until step 23), then recreate postgres on purpose (its environment gained `RESET_RW_DB_PASSWORD`; the Radar shows errors for a few seconds) and check the data is the same before and after.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 scanner analyze compact && B="$(docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT max(version) FROM _sqlx_migrations WHERE success) || '|' || (SELECT count(*) FROM runs) || '|' || (SELECT count(*) FROM paper_accounts)")" && echo "before $B" && docker compose up -d --no-build --timeout 60 postgres && for i in $(seq 90); do [ "$(docker inspect -f '{{.State.Health.Status}}' cute-trading-postgres-1)" = healthy ] && break; sleep 2; done; docker inspect -f '{{.State.Health.Status}}' cute-trading-postgres-1; A="$(docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT max(version) FROM _sqlx_migrations WHERE success) || '|' || (SELECT count(*) FROM runs) || '|' || (SELECT count(*) FROM paper_accounts)")"; echo "after $A"; [ -n "$B" ] && [ "$A" = "$B" ] && echo data-kept
```
→ expected: scanner, analyze and compact `Stopped`, `before 8|<runs>|<accounts>`, `Container cute-trading-postgres-1 Recreated` then `Started`, `healthy`, the same line as `after 8|<runs>|<accounts>`, then `data-kept`.
if not: `Running` instead of `Recreated` = the env did not change (step 14 not run): run step 14, then this step again; no `healthy` or no `data-kept` = `docker compose logs --tail 50 postgres`, start nothing else, report.
Note: recreating postgres here consumes the config drift, so the boot unit's `up scanner` (no `--no-deps`) does not recreate postgres at the next reboot; the role password itself still comes from step 22 (`01-roles.sh` runs only on an empty volume).
**STOP**

20. Migrate ALONE (it also builds the paper_ledger index while nothing inserts).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core run --rm --no-deps migrate; echo "migrate exit=$?"
```
→ expected: `migrate exit=0`.
if not: `docker compose --profile core logs --tail 50 migrate`; start nothing; a checksum error = a CRLF image: redo steps 2–17.
**STOP**

21. Migration 9 is applied, the grants are right and the index exists.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT max(version) FROM _sqlx_migrations WHERE success" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT has_table_privilege('scanner_rw','paper_epochs','INSERT'), has_table_privilege('scanner_rw','paper_epochs','UPDATE'), has_table_privilege('scanner_rw','paper_reset_requests','INSERT'), has_table_privilege('analyze_ro','paper_epochs','SELECT'), has_table_privilege('analyze_rw','paper_reset_requests','SELECT'), has_table_privilege('paper_reset_rw','paper_reset_requests','INSERT'), has_table_privilege('paper_reset_rw','paper_reset_requests','UPDATE'), has_table_privilege('paper_reset_rw','paper_epochs','INSERT'), has_table_privilege('paper_reset_rw','episodes','SELECT')" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM pg_indexes WHERE indexname = 'paper_ledger_account_t'"
```
→ expected: `9`, then `t|f|f|t|t|t|f|f|f`, then `1`.
if not: start nothing; full rollback (step 36).
**STOP**

22. Activate `paper_reset_rw` (pgdata exists, so the init script won't; the password is never printed), then log in with it over the network as the api will.
```bash
cd /home/isoca/projects/cute-trading && PW="$(grep -E '^RESET_RW_DB_PASSWORD=' .env | cut -d= -f2-)"; case "$PW" in ''|*[!0-9a-f]*) echo "pw-bad"; PW= ;; *) echo pw-read ;; esac; [ -n "$PW" ] && printf '\\set pw %s\nSET log_min_error_statement = panic;\nALTER ROLE paper_reset_rw LOGIN PASSWORD :'"'"'pw'"'"';\n' "$PW" | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -q -U ct_owner -d cute_trading && echo role-set; [ -n "$PW" ] && printf '%s' "$PW" | docker compose exec -T postgres sh -c 'PGPASSWORD="$(cat)" psql -h postgres -U paper_reset_rw -d cute_trading -tAc "SELECT current_user"'; unset PW; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'paper_reset_rw'"
```
→ expected: `pw-read`, `role-set`, `paper_reset_rw`, then `t`.
if not: `pw-bad` = the `.env` line isn't an `openssl rand -hex 24` value: `sed -i '/^RESET_RW_DB_PASSWORD=/d' .env`, redo step 14, run again; `password authentication failed` or no `paper_reset_rw` line: run again; do NOT go on to step 28 until this step prints `paper_reset_rw`.
**STOP**

23. Start the new scanner (ladder policies: a policies epoch, epoch 1) and note the deploy time.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7a_deploy_ms && docker compose --profile core up -d --no-build --no-deps scanner && sleep 60 && docker compose --profile core logs --since 70s scanner 2>&1 | grep -E 'paper:|error|panicked' | head
```
→ expected: `paper: policies changed (saved … != current …); all accounts start fresh`, then `paper: policies → epoch 1`, and no `error`/`panicked` lines.
if not: full rollback (step 36).
**STOP**

24. Gate: epoch 1 belongs to this run, 19 accounts are registered.
```bash
cd /home/isoca/projects/cute-trading && Q="(SELECT max(run_id) FROM runs WHERE service = 'scanner' AND mode = 'live')" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT epoch, cause, request_id IS NULL, run_id = $Q FROM paper_epochs ORDER BY epoch" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM paper_accounts WHERE run_id = $Q"
```
→ expected: `1|policies|t|t`, then `19`.
if not: **STOP**; scanner logs (`docker compose --profile core logs --tail 50 scanner`) to the owner.
**STOP**

25. Gate: the first minute mark of epoch 1 has 19 accounts at $1,000 (wait for the first minute boundary).
```bash
cd /home/isoca/projects/cute-trading && sleep 90 && Q="(SELECT max(run_id) FROM runs WHERE service = 'scanner' AND mode = 'live')" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*), min(equity), max(equity) FROM paper_marks WHERE run_id = $Q AND t_ms = (SELECT min(t_ms) FROM paper_marks WHERE run_id = $Q AND t_ms >= (SELECT start_ms FROM paper_epochs WHERE epoch = 1))"
```
→ expected: `19|1000|1000` (a few cents off on one or two accounts only if a trade executed in the first minute).
if not: fewer than 19 = accounts missing: **STOP** and report.
**STOP**

26. Start the new analyze.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps analyze && sleep 20 && docker inspect -f '{{.Name}} {{.State.Status}}' cute-trading-analyze-1 && docker compose --profile core logs --since 30s analyze 2>&1 | grep -c -E 'error|panicked'
```
→ expected: `/cute-trading-analyze-1 running`, then `0`.
if not: `docker compose --profile core logs --tail 30 analyze`.

27. Recreate compact on the 7a image (it must read `paper_reset` lines before any reset happens, so before the api with the reset route is up).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps compact && sleep 60 && [ "$(docker inspect -f '{{.Image}}' cute-trading-compact-1)" = "$(docker image inspect -f '{{.Id}}' cute-trading/rust:dev)" ] && echo compact-on-7a; docker compose --profile core logs --since 70s compact 2>&1 | grep -c -E 'error|refusing'
```
→ expected: `compact-on-7a`, then `0`.
if not: no `compact-on-7a` = compact still runs the old image: run this step again; do NOT go on to step 28 (no reset may be requested) until it prints `compact-on-7a`.
**STOP**

28. Scanner websockets baseline, then recreate the api only (the web container is not touched: the 2026-09-28 websocket-drop lesson).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7a_api_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/7a_scanner_started && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs FROM health ORDER BY minute_ms DESC LIMIT 3" && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && sleep 10 && docker compose -f compose.yml -f compose.web.yml --profile web logs --since 20s api 2>&1 | grep -E 'api:'
```
→ expected: the scanner's start time, three `HH:MI|<shards>|0|0|<msgs>` rows, then `api: listening on 0.0.0.0:8081 (read-only except paper reset requests, paper only)` and no `refusing` or `RESET_DATABASE_URL is not set` line.
if not: `refusing to start: the reset role can write …` = over-granted role: stop the api (`docker compose -f compose.yml -f compose.web.yml --profile web stop api`) and report; `database …` or `password authentication failed` errors = stop the api the same way and redo step 22.

29. After the api restart: no websocket disconnect.
```bash
cd /home/isoca/projects/cute-trading && sleep 120; T0=$(cat /root/7a_api_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7a_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"
```
→ expected: `scanner-not-restarted`, then `0`.
if not: reconnects > 0: do NOT roll back; write the count in Results.

## On the VPS / in the browser: checks

30. Unauthenticated requests never reach the api (forward-auth).
```bash
curl -sS -o /dev/null -w '%{http_code}\n' https://radar.isoca.space/api/paper/epochs; curl -sS -o /dev/null -w '%{http_code}\n' -X POST -H 'X-CT-Intent: reset-paper' -H 'Content-Type: application/json' --data '{"confirm":"reset"}' https://radar.isoca.space/api/paper/reset
```
→ expected: `302` or `401` twice.
if not: `200`/`202` = the api is reachable without login: stop the api at once (step 28's stop command) and report.

31. Logged in (browser, owner account): open `https://radar.isoca.space/api/paper/epochs` and `https://radar.isoca.space/api/paper/ladder`.
→ expected: epochs `{"epochs":[{"epoch":1,…,"cause":"policies"}],"current":1,"pending_request":null,"next_allowed_ms":…}`; ladder with `"epoch":1`, eight `rungs` (six with `"extra":false`, latencies 0, 25, 52, 100, 200, 300, then two extras), `"break_even":{"kind":"thin"}`, `"markets_status":"under_review"` or `"missing"`, `measured` with the host `vps-kvm2`.
if not: a 500 = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 api`; latencies `null` = `POLICIES_PATH` missing (step 18).

32. (Optional, uses today's reset: the whole path end to end, while epoch 1 is minutes old.) Only after step 27 printed `compact-on-7a`. In the browser console on `https://radar.isoca.space/` (logged in):
```js
fetch('/api/paper/reset', {method: 'POST', headers: {'X-CT-Intent': 'reset-paper', 'Content-Type': 'application/json'}, body: JSON.stringify({confirm: 'reset'})}).then(async r => console.log(r.status, await r.text()))
```
→ expected: `202 {"request_id":1,"requested_ms":…}`; the same call again prints `409 {"error":"reset_pending",…}`.
if not: `403` = the page wasn't same-origin (use the Radar tab itself); `503` = `RESET_DATABASE_URL` missing (step 18).

33. (Only after step 32) The scanner applied it as epoch 2 within ~30 s.
```bash
cd /home/isoca/projects/cute-trading && sleep 45 && docker compose --profile core logs --since 60s scanner 2>&1 | grep 'paper: reset' ; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT epoch, cause, request_id FROM paper_epochs ORDER BY epoch" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*), min(equity), max(equity) FROM paper_marks WHERE t_ms = (SELECT start_ms FROM paper_epochs WHERE epoch = 2)"
```
→ expected: `paper: reset → epoch 2 (request 1)`, then `1|policies|` and `2|reset|1`, then `19|1000|1000`; a third console call now prints `429 {"error":"reset_daily_limit",…}`.
if not: no `paper: reset` line after 60 s = the poller isn't running: scanner logs to the owner.

34. No hour lost and compact healthy (run 3 h after step 27).
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM compact_hours WHERE status = 'lost' AND done_ms > $(cat /root/7a_deploy_ms)" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT lag_hours, last_error FROM compact_status"
```
→ expected: `0`, then `<1 or 2>|` with an empty last_error.
if not: a `bad capture line` in last_error = compact runs a pre-7a image: redo step 27.

35. (On the PC) Pin 0009's production checksum and commit it.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && H=$(ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 "cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc \"SELECT encode(checksum, 'hex') FROM _sqlx_migrations WHERE version = 9\"" | tr -d '\r\n ') && echo "hex_len ${#H}" && python - "$H" <<'PY' && cargo test -p store --test migration_checksums 2>&1 | tail -3 && git add crates/store/tests/migration_checksums.rs && git commit -q -m "store: pin migration 0009's checksum" && git log -1 --format='%h %s'
import re, sys
h = sys.argv[1]
assert re.fullmatch(r"[0-9a-f]{96}", h), "not a 96-char hex checksum"
p = "crates/store/tests/migration_checksums.rs"
raw = open(p, newline="").read()
s = raw.replace("\r\n", "\n")
old = "    ),\n];\n"
assert s.count(old) == 1 and "        9,\n" not in s, "APPLIED already has 9 or changed shape"
s = s.replace(old, '    ),\n    (\n        9,\n        "%s",\n    ),\n];\n' % h, 1)
open(p, "w", newline="").write(s.replace("\n", "\r\n") if "\r\n" in raw else s)
print("pinned 9")
PY
```
→ expected: `hex_len 96`, `pinned 9`, `test result: ok. 2 passed; 0 failed`, then `<sha> store: pin migration 0009's checksum`.
if not: `2 passed` missing = the local 0009 differs from production (CRLF? edit?): `git checkout -- crates/store/tests/migration_checksums.rs` and report.

## Rollback

36. Back to the pre-7a images and policies (the old policies file is baked into `rust:pre7a`). Migration 0009 is additive and stays; the boot unit is removed (the pre-7a `migrate` refuses the applied 0009, so the unit could not start this scanner).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 scanner analyze && docker tag cute-trading/rust:pre7a cute-trading/rust:dev && docker compose --profile core up -d --no-build --no-deps scanner analyze && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && { [ ! -f /etc/systemd/system/isoca-radar-scanner.service ] || { systemctl disable --now isoca-radar-scanner.service && rm /etc/systemd/system/isoca-radar-scanner.service && systemctl daemon-reload && echo unit-removed; }; } && docker inspect -f '{{.Name}} {{.Image}}' cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-api-1
```
→ expected: `unit-removed` (if the 5b unit was installed), then all three on the `rust:pre7a` ID.
if not: a container on another ID = `docker image inspect -f '{{.Id}}' cute-trading/rust:pre7a` and retag; no `unit-removed` while `systemctl is-enabled isoca-radar-scanner.service` prints `enabled` = run the `{ … }` part again.
Note: after rollback the scanner no longer starts at boot: after any reboot run `docker compose --profile core up -d --no-build --no-deps scanner` (without `--no-deps` the pre-7a `migrate` fails on 0009); reinstall the unit (phase5b step 30) only with a 7a image back on `:dev`.
Note: a pre-7a scanner never writes `paper_epochs`; its own epoch is recorded only at the next 7a start (sha mismatch).
Note: if a reset happened (step 32), leave compact on the 7a image (a pre-7a compact hard-errors on an hour holding a `paper_reset` line); otherwise also run `docker compose --profile core up -d --no-build --no-deps compact`.
Note: a full restore from `/root/ct-pre-7a.dump` is a last resort only (it loses every row since step 16).

## Results

- Cost gate (step 11), both lines:
- First mark of epoch 1 (step 25):
- Reconnects after the api restart (step 29):
- Reset end to end (steps 32–33): done / skipped:
- Lost hours and compact status (step 34):
- 0009 checksum pinned (step 35), commit:
