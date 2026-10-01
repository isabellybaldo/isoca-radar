# Phase 7b step A on the VPS: the public snapshot writer and sim premise 3 (rust image; api, scanner, analyze, compact; public-data volume)

Note: one image change (the rust image: the sim fill extraction, so sim premise 2 → 3, and the api's snapshot task); the api is recreated with the new `public-data` volume at `/var/lib/ct-public`, and scanner, analyze and compact are recreated on the same image so premise 3 starts at a known minute.
Note: the web is unchanged and nothing serves the file (that is step B, plan 7b-B); no migration, and the VPS `.env` needs no new variable.
Note: capture pauses from step 18 until step 20 (about a minute); Edge capture and SWEET count anew from the scanner start of step 20.
Note: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; never add `--remove-orphans`.
Note: PC steps in Git Bash (steps 1–6 in the 7b-A worktree before merging, steps 7–13 in the main checkout on the merged commit); VPS steps as root over SSH (`ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`), run by the owner.

## Before merging (PC, the 7b-A worktree)

1. The branch is clean, on the commit to merge, and holds the identity check's base (`main` before 7b-A).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s' && git merge-base --is-ancestor 384e765c88c7e5bb909ed08b1e96ef9dba22522f HEAD && echo base-ok
```
→ expected: no status lines, `worktree-phase7b`, the branch's last commit, then `base-ok`.
if not: status lines = commit the listed changes, then run this step again; no `base-ok` = the branch does not start from `384e765`: report, do not go on.

2. Lints and the wasm32 build.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -1 && cargo check -p sim -p arb-core -p arb-wasm --target wasm32-unknown-unknown 2>&1 | tail -1
```
→ expected: two `Finished` lines and no `warning`/`error` line.
if not: fix on the branch and run again.
**STOP**

3. Every test, the Postgres-backed ones included.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && (exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && ( set -a; . /c/Users/Isabelly/PycharmProjects/cute-trading/.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1; cargo test --workspace 2>&1 | grep -E '^test result|SKIPPED|panicked' | sed 's/finished in.*//' | sort | uniq -c; echo "cargo exit=${PIPESTATUS[0]}" )
```
→ expected: `pg-up`, only `<n> test result: ok. …; 0 failed …` lines, then `cargo exit=0`.
if not: no `pg-up` = start the PC Postgres first; any `SKIPPED`/`panicked`/`FAILED`: fix on the branch.
**STOP**

4. A busy raw hour from the VPS: kept if `$HOME/probe/cap` already holds one (the reference hour is 2026-09-28 17:00), fetched otherwise.
```bash
K="$(find "$HOME/probe/cap" -name '*.jsonl.zst' 2>/dev/null | head -1)"; if [ -n "$K" ]; then echo "$K" && echo probe-kept; else rm -rf "$HOME/probe" && mkdir -p "$HOME/probe/cap" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 'command -v zstdcat >/dev/null || apt-get install -y zstd >/dev/null; cd /var/lib/docker/volumes/cute-trading_data/_data/capture && F=$(find . -name "[0-9][0-9].jsonl.zst" -mmin +70 -mmin -2880 -printf "%s %p\n" | sort -n | tail -1 | cut -d" " -f2) && U=$(for u in $(zstdcat "$F" | grep -o "universe-[0-9]*-[0-9]*\.json\.zst" | sort -u); do find . -name "$u"; done) && echo "busy hour: $F" >&2 && tar -cf - "$F" $U' | tar -xf - -C "$HOME/probe/cap" && find "$HOME/probe/cap" -type f | sort; fi
```
→ expected: `…/probe/cap/2026-09-28/17.jsonl.zst` and `probe-kept`, or `busy hour: …` and the fetched `.jsonl.zst` plus its `universe-*.json.zst` files.
if not: `busy hour:` empty = no closed hour in 2 days: drop `-mmin -2880` and run again.

5. The replay identity check (spec §3.3): base `384e765` and the branch replay the hour to the same six kinds of paper rows (intent, exec, fill, ledger, markout, account) and the same frames line (two debug builds and two replays: about 1 hour).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && bash scripts/replay-identity.sh "$HOME/probe/cap" 384e765c88c7e5bb909ed08b1e96ef9dba22522f 2>&1 | tail -7 | tee "$HOME/probe/identity.txt"
```
→ expected (reference hour): `== base: 384e765c88c7e5bb909ed08b1e96ef9dba22522f Merge pull request #11 …`, `frames 6721810 parse_errors 0 paper_rows 7552`, `== head: <branch sha> …`, `frames 6721810 parse_errors 0 paper_rows 7552`, `base rows 7552 order e8795661d1955760 sorted 38363ee57ee71d8c`, `head rows 7552 order e8795661d1955760 sorted 38363ee57ee71d8c`, then `IDENTICAL`; another hour = other figures, the same on both sides, then `IDENTICAL`.
if not: `DIFFERENT` with diff lines = the extraction changed a paper row or the frames line: do not merge, bring `$HOME/probe/identity.txt` to the owner; `no paper rows at all` = a quiet hour: `rm -rf "$HOME/probe"` and redo step 4; an `error:` line = a universe record is missing: `rm -rf "$HOME/probe"` and redo step 4.
**STOP**

6. PR CI green (rust, wasm, web, compose jobs; `cargo deny` runs only there).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && gh pr checks --json name,bucket --jq '.[] | "\(.name) \(.bucket)"' | sort
```
→ expected: `compose pass`, `rust pass`, `wasm pass`, `web pass`.
if not: `no pull requests found` = push the branch and open the 7b-A pull request first; `pending` = wait and run again; `fail` = read the job's log on GitHub, fix on the branch, redo from step 1.
**STOP**

## After merging (PC, the main checkout)

7. Merge the 7b-A pull request on GitHub, then bring the main checkout to it; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git checkout -q main && git pull -q --ff-only && git status --short --untracked-files=no && git log -1 --format='%h %s'
```
→ expected: no status lines, then the merge commit (`Merge pull request #… from isabellybaldo/…`).
if not: `Not possible to fast-forward` = local commits on `main`: report, do not build.
**STOP**

8. Line endings: migrations and configs baked into the image must be LF.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git ls-files --eol crates/store/migrations config | grep -v 'w/lf' ; grep -rlI "$(printf '\r')" crates/store/migrations config || echo lf-ok
```
→ expected: only `lf-ok`.
if not: `git checkout -- <each listed file>` and run this step again.

9. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `cargo clean` and `docker builder prune -f --keep-storage 5GB`, then run this step again.

10. Build the rust image (scanner, api, analyze, compact).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA" && docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate 2>&1 | tail -2
```
→ expected: the short sha (write it down: step 22 checks it), then the build's last lines without `ERROR`.
if not: a `CRLF found` failure = step 8 was skipped; otherwise read the first `ERROR` line, fix, run again.
**STOP**

11. The image works and carries the snapshot directory owned by 65532.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/scanner cute-trading/rust:dev --help | head -1; docker create --name ct-7b-peek cute-trading/rust:dev >/dev/null && docker cp ct-7b-peek:/var/lib/ct-public - | tar -tvf -; docker rm ct-7b-peek >/dev/null
```
→ expected: `read-only Polymarket arbitrage scanner (paper trading only)`, then one line `drwxr-xr-x 65532/65532 0 … ct-public/`.
if not: `root/root` or no line = the image was built from a tree without the 7b Dockerfile: redo step 10 from the merged commit.
**STOP**

12. Bundle the image and the compose files.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-7b" && cd "$HOME/ct-7b" && docker save cute-trading/rust:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && tar -cf "$HOME/ct-7b.tar" image.tar.gz compose.yml compose.web.yml .env.example && cd "$HOME" && rm -rf "$HOME/ct-7b" && ls -l "$HOME/ct-7b.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-7b.tar`, roughly 25–60 MB.
if not: free disk (step 9) and run this step again.

13. Ship the bundle.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7b.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-7b.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

14. The rollback tag `cute-trading/rust:pre7b`, from the running api container's image ID (never from `:dev`), only if the tag does not exist yet.
```bash
cd /home/isoca/projects/cute-trading && { docker image inspect cute-trading/rust:pre7b >/dev/null 2>&1 || docker tag "$(docker inspect -f '{{.Image}}' cute-trading-api-1)" cute-trading/rust:pre7b; } && docker image inspect -f '{{.Id}}' cute-trading/rust:pre7b && docker inspect -f '{{.Name}} {{.Image}}' cute-trading-api-1 cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1
```
→ expected: one `sha256:…` ID, then four lines (`/cute-trading-api-1`, `-scanner-1`, `-analyze-1`, `-compact-1`), all on that ID.
if not: `No such object` = a container is missing: `docker ps -a --format '{{.Names}} {{.Status}}'` and report; a container on another ID = one tag cannot roll all four back: write the IDs in Results and report, do not go on (a re-run after step 19 shows the new image on the containers and `pre7b` still on the old one: that is right).
**STOP**

15. Load the bundle (replaces compose.yml, compose.web.yml and .env.example).
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-7b.tar && rm /root/ct-7b.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls cute-trading/rust --format '{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/rust:dev`; `dev` on a new ID and `pre7b` on step 14's (older tags may be listed too); then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: a truncated tar = re-ship (step 13); over budget = `docker image prune -f` and run the budget again.

16. Compose parses, the api has its directory and volume, and the volume doesn't exist yet.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c 'public-data' compose.web.yml && grep -c 'PUBLIC_DIR: /var/lib/ct-public' compose.web.yml && docker volume ls -q | grep -c '^cute-trading_public-data$'
```
→ expected: `config-ok`, `3`, `1`, `0` (the api creates the volume at step 19, so it copies the image directory's owner 65532).
if not: `0` on either grep = the bundle's compose.web.yml is old: re-bundle (step 12); last line `1` = a volume exists already: `docker run --rm -v cute-trading_public-data:/v:ro alpine stat -c %u /v` must print `65532`; any other owner: `docker volume rm cute-trading_public-data` and run this step again.
**STOP**

17. A quiet minute (advisory): the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`.
if not: `f` = a busy minute: run this step again every few minutes; after 15 minutes without a `t`, go on to step 18 anyway (step 26 records the reconnects); an empty line = no health rows in 24 h: report before going on.
Note: run step 18 within 2 minutes of the `t` (or of the 15-minute limit).

18. Stop the scanner, analyze and compact (capture pauses until step 20).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7b_stop_ms && docker compose --profile core stop --timeout 60 scanner analyze compact && docker inspect -f '{{.Name}} {{.State.Status}}' cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, exit_reason FROM runs WHERE service = 'scanner' AND mode = 'live' ORDER BY run_id DESC LIMIT 1"
```
→ expected: scanner, analyze and compact `Stopped`, three `/cute-trading-…-1 exited` lines, then `<run>|shutdown`.
if not: a container still `running` = run this step again; an exit reason other than `shutdown` = write it in Results and go on.
Note: go straight on to steps 19 and 20: nothing is captured until the scanner is back.

19. Recreate the api on the new image, with the new volume (the web is not touched).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7b_api_ms && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && sleep 10 && docker compose -f compose.yml -f compose.web.yml --profile web logs --since 20s api 2>&1 | grep -E 'api:'
```
→ expected: a `Volume … cute-trading_public-data … Created` line, `Container cute-trading-api-1 … Recreated` then `Started`, then `api: public snapshot every 300 s into /var/lib/ct-public` and `api: listening on 0.0.0.0:8081 (read-only except paper reset requests, paper only)`, and no `refusing`, `api: reset pool:` or `RESET_DATABASE_URL is not set` line.
if not: no `public snapshot every` line = `PUBLIC_DIR` is missing (step 16); `refusing to start` or no `listening` line = roll back (step 28), which also restarts the scanner.
**STOP**

20. Start the scanner FIRST on the new image; its start time is the start of sim premise 3.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps scanner && sleep 20 && docker inspect -f '{{.State.Status}} {{.RestartCount}} {{.State.StartedAt}}' cute-trading-scanner-1 && [ "$(docker inspect -f '{{.Image}}' cute-trading-scanner-1)" = "$(docker image inspect -f '{{.Id}}' cute-trading/rust:dev)" ] && echo scanner-on-7b
```
→ expected: `Container cute-trading-scanner-1 … Recreated` then `Started` (a `Found orphan containers` warning naming api, web and oauth2-proxy is normal), `running 0 <UTC start time>` (write the time in Results as "premise 3 start"), then `scanner-on-7b`.
if not: `restarting`, `exited` or a restart count above 0 = `docker compose --profile core logs --tail 50 scanner`, then roll back (step 28); no `scanner-on-7b` = `docker compose --profile core up -d --no-build --no-deps --force-recreate scanner`, then the last two commands again.
Note: run step 21 within 4 minutes of step 19 (the second write replaces the first about 330 s after the api start).
**STOP**

21. Within 60 s of the api start (the first run is 30 s after it): the file, read without the api image (distroless, no shell), is owned by 65532, world-readable, under 64 KiB, and alone in the volume.
```bash
sleep 20; docker run --rm -v cute-trading_public-data:/v:ro alpine sh -c 'stat -c %u /v; stat -c "%n %u %a %s" /v/public.json; ls -A /v; head -c 100 /v/public.json; echo' && W=$(docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json | grep -o '"written_ms":[0-9]*' | cut -d: -f2) && echo "$W" > /root/7b_written_1 && echo "first write $(( W - $(cat /root/7b_api_ms) )) ms after the api start"
```
→ expected: (alpine's pull lines the first time, then) `65532`, `/v/public.json 65532 644 <bytes below 65536>`, `public.json` alone (no `public.json.tmp`), `{"v":1,"written_ms":…,"ladder":{"epoch":…`, then `first write <n> ms after the api start` with n between 30000 and 60000.
if not: `No such file` = no run yet or a failed one: `docker compose -f compose.yml -f compose.web.yml --profile web logs api 2>&1 | grep 'public snapshot'` names the reason (`skipped: …`); owner `0` = the volume was not created from the 7b image: `docker compose -f compose.yml -f compose.web.yml --profile web rm -sf api && docker volume rm cute-trading_public-data`, then redo steps 19 and 21 (the scanner stays up); n above 300000 = this step ran after the second write: write it in Results.
**STOP**

22. The new scanner run records sim premise 3 on the shipped commit, and its log is clean.
```bash
cd /home/isoca/projects/cute-trading && sleep 60 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT r.run_id, r.version, p.version FROM runs r JOIN run_premises p USING (run_id) WHERE r.service = 'scanner' AND r.mode = 'live' AND p.area = 'sim' ORDER BY r.run_id DESC LIMIT 2" && docker compose --profile core logs scanner 2>&1 | grep -c -E 'paper: policies changed|panicked|safe mode|error'
```
→ expected: `<new run>|<step 10's short sha>|3+<8 hex>`, then `<previous run>|<older sha>|2+<the same 8 hex>`, then `0`.
if not: the first line on `2+…` or an older sha = the scanner runs the old image: step 20's `--force-recreate`, then this step again; a count above 0 = `docker compose --profile core logs --tail 50 scanner` and report (`paper: policies changed` = every paper account started fresh: report at once).
**STOP**

23. Start analyze on the new image.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps analyze && sleep 20 && docker inspect -f '{{.Name}} {{.State.Status}}' cute-trading-analyze-1 && docker compose --profile core logs --since 30s analyze 2>&1 | grep -c -E 'error|panicked'
```
→ expected: `/cute-trading-analyze-1 running`, then `0`.
if not: `docker compose --profile core logs --tail 30 analyze` and report.

24. Start compact on the new image; all four containers run the shipped image.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps compact && sleep 60 && docker compose --profile core logs --since 70s compact 2>&1 | grep -c -E 'error|refusing'; docker image inspect -f '{{.Id}}' cute-trading/rust:dev && docker inspect -f '{{.Name}} {{.Image}} {{.State.Status}}' cute-trading-api-1 cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1
```
→ expected: `0`, the `:dev` image ID, then four lines on that ID, each `running`.
if not: a container on another ID = `docker compose --profile core up -d --no-build --no-deps --force-recreate <service>` (analyze or compact) and run the last two commands again; a count above 0 = `docker compose --profile core logs --tail 30 compact` and report.

25. The file's key paths are all on the whitelist (spec §5.2), checked with jq.
```bash
command -v jq >/dev/null || apt-get install -y jq >/dev/null; docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json | jq -r '(["v","written_ms","ladder","today"] + (["epoch","start_ms","as_of_ms","break_even","break_even.kind","break_even.latency_ms","break_even.direction","break_even.not_monotone","measured","measured.latency_ms","markets_status","rungs","markets"] | map("ladder." + .)) + (["scenario","latency_ms","extra","n_exec","n_leg_risk","n_missed","gains","leg_risk","settle","unrealized","net"] | map("ladder.rungs[]." + .)) + (["side","scenario","fee_bips","latency_ms","dimension","bucket","label","n_exec","clusters","mean","lo","hi"] | map("ladder.markets[]." + .)) + (["day_start_ms","arb","no","yes","edge_budget100","hourly","hourly[].hour","hourly[].arb"] | map("today." + .))) as $wl | ([paths | select(.[-1] | type == "string") | map(if type == "number" then "[]" else "." + . end) | join("") | ltrimstr(".")] | unique) as $got | ($got - $wl) as $extra | "v \(.v) rungs \(.ladder.rungs | length) markets \(.ladder.markets | length) hourly \(.today.hourly | length)", "extra \($extra | tojson)", (if .v == 1 and ($extra | length) == 0 then "WHITELIST OK" else "WHITELIST FAIL" end)'
```
→ expected: `v 1 rungs <8 or fewer> markets <n> hourly 24`, `extra []`, `WHITELIST OK`.
if not: `WHITELIST FAIL` with extra paths = a field ships that the spec forbids: roll back (step 28) and report; nothing serves the file yet, so nothing leaked; `jq: command not found` = `apt-get install -y jq` and run again.
**STOP**

26. Record (not a gate) the reconnects since the stop, and that capture is back.
```bash
cd /home/isoca/projects/cute-trading && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $(cat /root/7b_stop_ms)" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs FROM health ORDER BY minute_ms DESC LIMIT 3"
```
→ expected: step 20's start time, a count (any number: write it in Results), then up to three `HH:MI|<shards>|<reconnects>|0|<msgs>` rows of the last minutes with `msgs` above 0.
if not: no row newer than step 20's start time, or `msgs` 0 = the scanner is not capturing: `docker compose --profile core logs --tail 50 scanner` and report; do NOT roll back for a reconnect count.

27. The second run replaces the file about 5 minutes after the first, no run was skipped, and the file is still not public.
```bash
cd /home/isoca/projects/cute-trading && S=$(( ($(cat /root/7b_written_1) + 310000 - $(date +%s%3N)) / 1000 )); [ "$S" -gt 0 ] && sleep "$S"; N=$(docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json | grep -o '"written_ms":[0-9]*' | cut -d: -f2); echo "second write $(( N - $(cat /root/7b_written_1) )) ms after the first"; docker compose -f compose.yml -f compose.web.yml --profile web logs api 2>&1 | grep -c 'public snapshot skipped'; curl -sS -o /dev/null -w '%{http_code}\n' https://radar.isoca.space/math-data/public.json
```
→ expected: `second write <n> ms after the first` with n about 300000 (or a multiple of it, run later), then `0`, then `302` (the login redirect: not served publicly until step B).
if not: n `0` = runs are failing: `docker compose -f compose.yml -f compose.web.yml --profile web logs api 2>&1 | grep 'public snapshot skipped'` names the reason (`today: …`/`ladder: …` = a query; `write …` = the volume); `401` = also not public: write it in Results; `200` = something serves the path without login: `docker compose -f compose.yml -f compose.web.yml --profile web stop web` and report at once.
**STOP**

## Rollback

28. Back to the pre-7b image on all four containers, in the same order (the volume and its file stay; nothing reads them before step B).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 scanner analyze compact && docker tag cute-trading/rust:pre7b cute-trading/rust:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && docker compose --profile core up -d --no-build --no-deps scanner && docker compose --profile core up -d --no-build --no-deps analyze && docker compose --profile core up -d --no-build --no-deps compact && sleep 20 && docker image inspect -f '{{.Id}}' cute-trading/rust:pre7b && docker inspect -f '{{.Name}} {{.Image}} {{.State.Status}} {{.State.StartedAt}}' cute-trading-api-1 cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1
```
→ expected: the `pre7b` ID, then four lines on that ID, each `running`; write the scanner's start time in Results as "premise 3 end".
if not: a container on another ID = `docker compose --profile core up -d --no-build --no-deps --force-recreate <service>` (the api: `docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps --force-recreate api`); a container not `running` = `docker compose -f compose.yml -f compose.web.yml --profile core --profile web logs --tail 30 <service>` and report.
Note: the pre-7b api ignores `PUBLIC_DIR` and never writes the mounted volume; the pre-7b scanner records sim premise 2 again.
Note: to remove the volume as well (only if step B is not coming): `docker volume rm cute-trading_public-data` after the api runs without it (a compose.web.yml without the mount).

## Results

- Replay identity (step 5): frames, paper rows, hashes, verdict:
- PR CI (step 6):
- Image directory owner (step 11):
- Rollback tag ID, and any container on another ID (step 14):
- Quiet minute (step 17), UTC time, or went on after 15 minutes:
- Scanner exit reason at the stop (step 18):
- Premise 3 start = the scanner's start time, UTC (step 20):
- File owner, mode, size, and ms after the api start (step 21):
- Sim premise and sha of the new run (step 22):
- Whitelist check (step 25):
- Reconnects since the stop (step 26):
- Second write, skipped runs, HTTP code (step 27):
