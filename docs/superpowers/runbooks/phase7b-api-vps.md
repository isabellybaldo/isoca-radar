# Phase 7b step A on the VPS: the public snapshot writer and sim premise 3 (rust image; api, scanner, analyze, compact; public-data volume)

Note: one image change (the rust image: the sim fill extraction, so sim premise 2 → 3, and the api's snapshot task); the api is recreated with the new `public-data` volume at `/var/lib/ct-public`, and scanner, analyze and compact are recreated on the same image so premise 3 starts at a known minute.
Note: the web is unchanged and nothing serves the file (that is step B, plan 7b-B); no migration, and the VPS `.env` needs no new variable.
Note: order on the VPS: analyze and compact stop (step 20), the api is recreated while the scanner still captures on the old image (step 21), then the scanner alone is recreated in one command (step 23), then analyze and compact start (steps 25–26).
Note: capture pauses only while step 23 recreates the scanner (under a minute); Edge capture and SWEET count anew from that scanner start.
Note: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; never add `--remove-orphans`; never install a package on the VPS.
Note: PC steps in Git Bash (steps 1–6 in the 7b-A worktree before merging, steps 7–14 in the main checkout on the merged commit); VPS steps as root over SSH (`ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`), run by the owner.

## Before merging (PC, the 7b-A worktree)

1. The branch is clean, on the commit to merge, and holds the identity check's base (`main` before 7b-A).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s' && git merge-base --is-ancestor 384e765c88c7e5bb909ed08b1e96ef9dba22522f HEAD && echo base-ok
```
→ expected: no status lines, `worktree-phase7b`, the branch's last commit, then `base-ok`.
if not: status lines = commit the listed changes, then run this step again; no `base-ok` = the branch does not start from `384e765`: report, do not go on.
**STOP**

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

4. A busy raw hour from the VPS: kept if `$HOME/probe/cap` already holds one (the reference hour is 2026-09-28 17:00), fetched otherwise (nothing is installed on the VPS: without `zstdcat` on the host, the postgres container's is used).
```bash
K="$(find "$HOME/probe/cap" -name '*.jsonl.zst' 2>/dev/null | head -1)"; if [ -n "$K" ]; then echo "$K" && echo probe-kept; else rm -rf "$HOME/probe" && mkdir -p "$HOME/probe/cap" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 'if command -v zstdcat >/dev/null; then Z=zstdcat; else Z="docker exec -i cute-trading-postgres-1 zstdcat"; echo "zstd-missing: reading through the postgres container" >&2; fi; cd /var/lib/docker/volumes/cute-trading_data/_data/capture && F=$(find . -name "[0-9][0-9].jsonl.zst" -mmin +70 -mmin -2880 -printf "%s %p\n" | sort -n | tail -1 | cut -d" " -f2) && U=$(for u in $($Z < "$F" | grep -o "universe-[0-9]*-[0-9]*\.json\.zst" | sort -u); do find . -name "$u"; done) && echo "busy hour: $F" >&2 && tar -cf - "$F" $U' | tar -xf - -C "$HOME/probe/cap" && find "$HOME/probe/cap" -type f | sort; fi
```
→ expected: `…/probe/cap/2026-09-28/17.jsonl.zst` and `probe-kept`, or (`zstd-missing: …` when the host has no zstd, then) `busy hour: …` and the fetched `.jsonl.zst` plus its `universe-*.json.zst` files.
if not: `busy hour:` empty = no closed hour in 2 days: drop `-mmin -2880` and run again; only the `.jsonl.zst` and no `universe-*` file = the decompression failed on the VPS: report, install nothing.

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

9. Free the worktree's build output (about 26 GiB inside the main checkout's folder, counted by step 10), and check `.claude` is out of the Docker build context.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && cargo clean 2>&1 | tail -1; grep -c '^\.claude' /c/Users/Isabelly/PycharmProjects/cute-trading/.dockerignore
```
→ expected: `Removed <n> files, <x>GiB total` (about 26 GiB the first time), then `1`.
if not: `No such file or directory` = the worktree is already removed: go on; `Removed 0 files` = already clean: go on; `0` = the main checkout's `.dockerignore` lacks `.claude` (not the merged commit): redo step 7, do not build.

10. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `cargo clean` in `/c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b` (step 9) and in `/c/Users/Isabelly/PycharmProjects/cute-trading`, and `docker builder prune -f --keep-storage 5GB`, then run this step again.

11. Build the rust image (scanner, api, analyze, compact).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA" && docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate 2>&1 | tail -2
```
→ expected: the short sha (write it down: step 24 checks it), then the build's last lines without `ERROR`.
if not: a `CRLF found` failure = step 8 was skipped; otherwise read the first `ERROR` line, fix, run again.
**STOP**

12. The image works and carries the snapshot directory owned by 65532.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/scanner cute-trading/rust:dev --help | head -1; docker create --name ct-7b-peek cute-trading/rust:dev >/dev/null && docker cp ct-7b-peek:/var/lib/ct-public - | tar -tvf -; docker rm ct-7b-peek >/dev/null
```
→ expected: `read-only Polymarket arbitrage scanner (paper trading only)`, then one line `drwxr-xr-x 65532/65532 0 … ct-public/`.
if not: `root/root` or no line = the image was built from a tree without the 7b Dockerfile: redo step 11 from the merged commit.
**STOP**

13. Bundle the image and the compose files.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-7b" && cd "$HOME/ct-7b" && docker save cute-trading/rust:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && tar -cf "$HOME/ct-7b.tar" image.tar.gz compose.yml compose.web.yml .env.example && cd "$HOME" && rm -rf "$HOME/ct-7b" && ls -l "$HOME/ct-7b.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-7b.tar`, roughly 25–60 MB.
if not: free disk (steps 9–10) and run this step again.

14. Ship the bundle.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7b.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-7b.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

15. The rollback tag `cute-trading/rust:pre7b`, from the running api container's image ID (never from `:dev`), only if the tag does not exist yet.
```bash
cd /home/isoca/projects/cute-trading && { docker image inspect cute-trading/rust:pre7b >/dev/null 2>&1 || docker tag "$(docker inspect -f '{{.Image}}' cute-trading-api-1)" cute-trading/rust:pre7b; } && docker image inspect -f '{{.Id}}' cute-trading/rust:pre7b && docker inspect -f '{{.Name}} {{.Image}}' cute-trading-api-1 cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1
```
→ expected: one `sha256:…` ID, then four lines (`/cute-trading-api-1`, `-scanner-1`, `-analyze-1`, `-compact-1`), all on that ID.
if not: `No such object` = a container is missing: `docker ps -a --format '{{.Names}} {{.Status}}'` and report; a container on another ID = one tag cannot roll all four back: write the IDs in Results and report, do not go on (a re-run after step 21 shows the new image on the containers and `pre7b` still on the old one: that is right).
**STOP**

16. Keep the pre-7b compose files, then load the bundle (replaces compose.yml, compose.web.yml and .env.example); compose.yml must come out unchanged.
```bash
cd /home/isoca/projects/cute-trading && { [ -e /root/compose.yml.pre7b ] || cp -p compose.yml /root/compose.yml.pre7b; } && { [ -e /root/compose.web.yml.pre7b ] || cp -p compose.web.yml /root/compose.web.yml.pre7b; } && tar --no-same-owner -xf /root/ct-7b.tar && rm /root/ct-7b.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls cute-trading/rust --format '{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh; cmp -s compose.yml /root/compose.yml.pre7b && echo compose-yml-same
```
→ expected: `Loaded image: cute-trading/rust:dev`; `dev` on a new ID and `pre7b` on step 15's (older tags may be listed too); `total <x> GB of 75 GB` and no `OVER BUDGET`; then `compose-yml-same`.
if not: a truncated tar = bundle and ship again (steps 13–14), then this step; over budget = `docker image prune -f` and `LIMIT_GB=75 sh scripts/disk-budget.sh` again; no `compose-yml-same` = the VPS compose.yml differed from the merged one (7b-A does not change it): `diff /root/compose.yml.pre7b compose.yml`, report, do not go on.
**STOP**

17. The alpine image, pulled now (visible, before anything is stopped), for reading the volume without the api image (distroless, no shell).
```bash
docker pull -q alpine && docker image inspect -f '{{.Id}}' alpine
```
→ expected: `docker.io/library/alpine:latest`, then one `sha256:…` ID.
if not: no download = install nothing, and read the volume from its host path as root: step 22's whole block becomes `sleep 30; P=/var/lib/docker/volumes/cute-trading_public-data/_data; stat -c %u "$P"; stat -c '/v/public.json %u %a %s' "$P/public.json"; ls -A "$P"; head -c 100 "$P/public.json"; echo; W=$(grep -o '"written_ms":[0-9]*' "$P/public.json" | cut -d: -f2) && echo "$W" > /root/7b_written_1 && echo "first write $(( W - $(cat /root/7b_api_ms) )) ms after the api start"` (the same lines in the same format as step 22 expects); in steps 27 and 29 replace `docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json` by `cat /var/lib/docker/volumes/cute-trading_public-data/_data/public.json`; in step 18's if-not replace the `docker run … alpine stat -c %u /v` by `stat -c %u /var/lib/docker/volumes/cute-trading_public-data/_data`.

18. Compose parses, the api has its directory and volume, and the volume doesn't exist yet.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c 'public-data' compose.web.yml && grep -c 'PUBLIC_DIR: /var/lib/ct-public' compose.web.yml && docker volume ls -q | grep -c '^cute-trading_public-data$'
```
→ expected: `config-ok`, `3`, `1`, `0` (the api creates the volume at step 21, so it copies the image directory's owner 65532).
if not: `0` on either grep = the bundle's compose.web.yml is old: bundle and ship again (steps 13–14), then step 16; last line `1` = a volume exists already: `docker run --rm -v cute-trading_public-data:/v:ro alpine stat -c %u /v` must print `65532`; any other owner: `docker volume rm cute-trading_public-data` and run this step again.
**STOP**

19. A quiet minute (advisory): the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`.
if not: `f` = a busy minute: run this step again every few minutes; after 15 minutes without a `t`, go on to step 20 anyway (steps 23 and 28 record the reconnects); an empty line = no health rows in 24 h: report before going on.
Note: run steps 20 to 23 one after the other, starting within 2 minutes of the `t` (or of the 15-minute limit).

20. Stop analyze and compact (the scanner keeps capturing on the old image).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 analyze compact && docker inspect -f '{{.Name}} {{.State.Status}}' cute-trading-analyze-1 cute-trading-compact-1 cute-trading-scanner-1
```
→ expected: analyze and compact `Stopped`, then `/cute-trading-analyze-1 exited`, `/cute-trading-compact-1 exited`, `/cute-trading-scanner-1 running`.
if not: analyze or compact still `running` = run this step again; the scanner not `running` = `docker compose --profile core logs --tail 50 scanner` and report, do not go on.
**STOP**

21. Recreate the api on the new image, with the new volume, while the scanner still runs on the old image (the web is not touched).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7b_api_ms && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && sleep 10 && docker compose -f compose.yml -f compose.web.yml --profile web logs --since 20s api 2>&1 | grep -E 'api:'
```
→ expected: a `Volume … cute-trading_public-data … Created` line, `Container cute-trading-api-1 … Recreated` then `Started` (on step 22's redo, which removed the container: `Created`, not `Recreated`), then `api: public snapshot every 300 s into /var/lib/ct-public` and `api: listening on 0.0.0.0:8081 (read-only except paper reset requests, paper only)`, and no `refusing`, `api: reset pool:` or `RESET_DATABASE_URL is not set` line.
if not: no `public snapshot every` line, a `refusing to start` line or no `listening` line = roll back with step 30 (the scanner is still capturing on the old image, and step 30 leaves it running), then report.
**STOP**

22. Within 60 s of the api start (the first run is 30 s after it): the file is owned by 65532, world-readable, under 64 KiB, and alone in the volume.
```bash
sleep 30; docker run --rm -v cute-trading_public-data:/v:ro alpine sh -c 'stat -c %u /v; stat -c "%n %u %a %s" /v/public.json; ls -A /v; head -c 100 /v/public.json; echo' && W=$(docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json | grep -o '"written_ms":[0-9]*' | cut -d: -f2) && echo "$W" > /root/7b_written_1 && echo "first write $(( W - $(cat /root/7b_api_ms) )) ms after the api start"
```
→ expected: `65532`, `/v/public.json 65532 644 <bytes below 65536>`, `public.json` alone (no `public.json.tmp`), `{"v":1,"written_ms":…,"ladder":{"epoch":…`, then `first write <n> ms after the api start` with n between 30000 and 60000 (the same on the redo below, where step 21 printed `Container cute-trading-api-1 … Created`, not `Recreated`).
if not: `Unable to find image 'alpine` or a pull error = no alpine image: run step 17's host-path block instead (same expected lines); `No such file` = no run yet or a failed one: `docker compose -f compose.yml -f compose.web.yml --profile web logs api 2>&1 | grep 'public snapshot'` names the reason (`skipped: …`): report; owner `0` = the volume was not created from the 7b image: `docker compose -f compose.yml -f compose.web.yml --profile web rm -sf api && docker volume rm cute-trading_public-data`, then redo steps 21 and 22 (the scanner keeps capturing; step 21 then prints `Created` for the container, not `Recreated`); n above 300000 = this step ran after the second write: write it in Results.
**STOP**

23. The reconnects since the api recreate (recorded, not a gate), then the scanner alone recreated on the new image in one command; its start time is the start of sim premise 3.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $(cat /root/7b_api_ms)" && date +%s%3N > /root/7b_scanner_ms && docker compose --profile core up -d --no-build --no-deps --timeout 60 scanner && sleep 20 && docker inspect -f '{{.State.Status}} {{.RestartCount}} {{.State.StartedAt}}' cute-trading-scanner-1
```
→ expected: a count (any number: write it in Results), `Container cute-trading-scanner-1 … Recreated` then `Started` (a `Found orphan containers` warning naming api, web and oauth2-proxy is normal), then `running 0 <UTC start time>` (write the time in Results as "premise 3 start").
if not: `Running` and no `Recreated` = compose saw no new image: `docker compose --profile core up -d --no-build --no-deps --timeout 60 --force-recreate scanner`, then the `docker inspect` again; `restarting`, `exited` or a restart count above 0 = `docker compose --profile core logs --tail 50 scanner`, then roll back (step 30).
**STOP**

24. The scanner runs the shipped image, its new run records sim premise 3 on the shipped commit, the old run ended cleanly, and the log is clean.
```bash
cd /home/isoca/projects/cute-trading && [ "$(docker inspect -f '{{.Image}}' cute-trading-scanner-1)" = "$(docker image inspect -f '{{.Id}}' cute-trading/rust:dev)" ] && echo scanner-on-7b; sleep 40; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT r.run_id, r.version, p.version, coalesce(r.exit_reason, '') FROM runs r JOIN run_premises p USING (run_id) WHERE r.service = 'scanner' AND r.mode = 'live' AND p.area = 'sim' ORDER BY r.run_id DESC LIMIT 2" && docker compose --profile core logs scanner 2>&1 | grep -c -E 'paper: policies changed|panicked|safe mode|error'
```
→ expected: `scanner-on-7b`, `<new run>|<step 11's short sha>|3+<8 hex>|`, then `<previous run>|<older sha>|2+<the same 8 hex>|shutdown`, then `0`.
if not: no `scanner-on-7b`, or the first row on `2+…` or an older sha = the scanner runs the old image: step 23's `--force-recreate`, then this step again; a previous exit reason other than `shutdown` = write it in Results and go on; a count above 0 = `docker compose --profile core logs --tail 50 scanner` and report (`paper: policies changed` = every paper account started fresh: report at once).
**STOP**

25. Start analyze on the new image.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps analyze && sleep 20 && docker inspect -f '{{.Name}} {{.State.Status}}' cute-trading-analyze-1 && docker compose --profile core logs --since 30s analyze 2>&1 | grep -c -E 'error|panicked'
```
→ expected: `/cute-trading-analyze-1 running`, then `0`.
if not: `docker compose --profile core logs --tail 30 analyze` and report.

26. Start compact on the new image; all four containers run the shipped image.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps compact && sleep 60 && docker compose --profile core logs --since 70s compact 2>&1 | grep -c -E 'error|refusing'; docker image inspect -f '{{.Id}}' cute-trading/rust:dev && docker inspect -f '{{.Name}} {{.Image}} {{.State.Status}}' cute-trading-api-1 cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1
```
→ expected: `0`, the `:dev` image ID, then four lines on that ID, each `running`.
if not: analyze or compact on another ID = `docker compose --profile core up -d --no-build --no-deps --force-recreate <service>` and run the last two commands again; a count above 0 = `docker compose --profile core logs --tail 30 compact` and report.

27. (On the PC) The file's key paths are all among the 48 whitelisted ones (spec §5.2).
```bash
ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 "docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json" > "$HOME/7b-public.json" && python - "$HOME/7b-public.json" <<'PY'
import json, sys
WL = set("v written_ms ladder today".split()) | {"ladder." + k for k in "epoch start_ms as_of_ms break_even break_even.kind break_even.latency_ms break_even.direction break_even.not_monotone measured measured.latency_ms markets_status rungs markets".split()} | {"ladder.rungs[]." + k for k in "scenario latency_ms extra n_exec n_leg_risk n_missed gains leg_risk settle unrealized net".split()} | {"ladder.markets[]." + k for k in "side scenario fee_bips latency_ms dimension bucket label n_exec clusters mean lo hi".split()} | {"today." + k for k in "day_start_ms arb no yes edge_budget100 hourly hourly[].hour hourly[].arb".split()}
def walk(v, at, out):
    if isinstance(v, dict):
        for k, x in v.items():
            p = f"{at}.{k}" if at else k
            out.add(p)
            walk(x, p, out)
    elif isinstance(v, list):
        for x in v:
            walk(x, at + "[]", out)
raw = open(sys.argv[1], "rb").read()
v = json.loads(raw)
got = set()
walk(v, "", got)
extra = sorted(got - WL)
print("v", v["v"], "bytes", len(raw), "rungs", len(v["ladder"]["rungs"]), "markets", len(v["ladder"]["markets"]), "hourly", len(v["today"]["hourly"]), "whitelist", len(WL))
print("extra", extra)
print("WHITELIST OK" if v["v"] == 1 and not extra and len(raw) < 65536 else "WHITELIST FAIL")
PY
```
→ expected: `v 1 bytes <n below 65536> rungs <8 or fewer> markets <n> hourly 24 whitelist 48`, `extra []`, `WHITELIST OK`.
if not: `WHITELIST FAIL` with extra paths = a field ships that the spec forbids: roll back (step 30) and report; nothing serves the file yet, so nothing leaked; a Python traceback = the file did not arrive whole: run this step again.
**STOP**

28. (On the VPS) Record (not a gate) the reconnects since the scanner recreate, and that capture is back.
```bash
cd /home/isoca/projects/cute-trading && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $(cat /root/7b_scanner_ms)" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs FROM health ORDER BY minute_ms DESC LIMIT 3"
```
→ expected: step 23's start time, a count (any number: write it in Results), then three `HH:MI|<shards>|<reconnects>|0|<msgs>` rows, the newest after step 23's start time with `msgs` above 0.
if not: no row newer than step 23's start time, or `msgs` 0 = the scanner is not capturing: `docker compose --profile core logs --tail 50 scanner` and report; do NOT roll back for a reconnect count.

29. The second run replaces the file about 5 minutes after the first, no run was skipped, and the file is still not public.
```bash
cd /home/isoca/projects/cute-trading && S=$(( ($(cat /root/7b_written_1) + 310000 - $(date +%s%3N)) / 1000 )); [ "$S" -gt 0 ] && sleep "$S"; N=$(docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json | grep -o '"written_ms":[0-9]*' | cut -d: -f2); echo "second write $(( N - $(cat /root/7b_written_1) )) ms after the first"; docker compose -f compose.yml -f compose.web.yml --profile web logs api 2>&1 | grep -c 'public snapshot skipped'; curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/math-data/public.json
```
→ expected: `second write <n> ms after the first` with n about 300000 (or a multiple of it, run later), then `0`, then `302` and the login redirect's URL (not served publicly until step B).
if not: n `0` = no second write yet: run this step again once; still `0` = runs are failing: the next command names the reason; a skipped count above 0 = `docker compose -f compose.yml -f compose.web.yml --profile web logs api 2>&1 | grep 'public snapshot skipped'` (`today: …`/`ladder: …` = a query; `write …` = the volume): write the lines in Results and report (no rollback: a skipped run keeps the old file); `401` = also not public: write it in Results; `200` = something serves the path without login: `docker compose -f compose.yml -f compose.web.yml --profile web stop web` and report at once.
**STOP**

## Rollback

30. Back to the pre-7b image, in the deploy's order: analyze and compact stop, the api is recreated while the scanner runs, the scanner alone is recreated (only if it is on the 7b image), then analyze and compact start; the volume and its file stay.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 analyze compact && docker tag cute-trading/rust:pre7b cute-trading/rust:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && sleep 10 && date +%s%3N > /root/7b_rollback_ms && docker compose --profile core up -d --no-build --no-deps --timeout 60 scanner && docker compose --profile core up -d --no-build --no-deps analyze && docker compose --profile core up -d --no-build --no-deps compact && sleep 20 && docker image inspect -f '{{.Id}}' cute-trading/rust:pre7b && docker inspect -f '{{.Name}} {{.Image}} {{.State.Status}} {{.State.StartedAt}}' cute-trading-api-1 cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1
```
→ expected: the scanner `Recreated` if it was on the 7b image (`Running` if it never left the old one), the `pre7b` ID, then four lines on that ID, each `running`; if the scanner was recreated, write its start time in Results as "premise 3 end".
if not: a container on another ID = `docker compose --profile core up -d --no-build --no-deps --force-recreate <service>` (the api: `docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps --force-recreate api`); a container not `running` = `docker compose -f compose.yml -f compose.web.yml --profile core --profile web logs --tail 30 <service>` and report.
Note: the pre-7b api ignores `PUBLIC_DIR` and never writes the mounted volume; a scanner recreated on the pre-7b image records sim premise 2 again.
Note: the pre-7b compose files, if wanted too (drops the api's mount): `cp -p /root/compose.yml.pre7b compose.yml && cp -p /root/compose.web.yml.pre7b compose.web.yml && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api`.
Note: to remove the volume as well (only if step B is not coming, and after the note above): `docker volume rm cute-trading_public-data`.

## Results

- Replay identity (step 5): frames, paper rows, hashes, verdict:
- PR CI (step 6):
- Image directory owner (step 12):
- Rollback tag ID, and any container on another ID (step 15):
- Quiet minute (step 19), UTC time, or went on after 15 minutes:
- File owner, mode, size, and ms after the api start (step 22):
- Reconnects between the api recreate and the scanner recreate (step 23):
- Premise 3 start = the scanner's start time, UTC (step 23):
- Sim premise and sha of the new run, exit reason of the old one (step 24):
- Whitelist check (step 27):
- Reconnects since the scanner recreate (step 28):
- Second write, skipped runs, HTTP code and redirect (step 29):
