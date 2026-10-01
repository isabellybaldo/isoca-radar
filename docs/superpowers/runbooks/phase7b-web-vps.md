# Phase 7b step B on the VPS: the public /math page (web image, the web mount, the ct-math router; oauth2-proxy scope)

Note: this deploy changes two containers: `oauth2-proxy` (recreated for its scope, step 14) and `web` (new image, new labels, the read-only `public-data` mount, step 16); the api, scanner, analyze, compact and postgres are not touched.
Note: step A (the api writing `public.json` into `cute-trading_public-data`) must be live first, and PR #15 must not be merged before it: step 1 checks it, and `web` must never be the first container to mount that volume.
Note: the public paths (`/math`, `/math-assets/…`, `/math-data/public.json`) are served by nginx's second server on port 8081 through the Traefik service `ct-math`; the dashboard stays on 8080 through `ct-web`; neither port is published on the host.
Note: limits on the public paths, per client IP (no CDN in front: the TCP peer is the client): 5 requests/s on average, burst 20, 8 in flight.
Note: `/math` depends on `oauth2-proxy` too (the `ct-sec` middleware lives on its labels): while that container is down, `/math` fails closed.
Note: rollback material: the tag `cute-trading/web:pre7b-B` (from the running container's image ID; `web:pre7b` and `web:pre7a` stay untouched) and `/root/compose.yml.pre7b-B`, `/root/compose.web.yml.pre7b-B`.
Note: the quiet minute (step 15) is advisory: at most 15 minutes of waiting, then go on and record the reconnects.
Note: the flood steps (22–26) use one source IP: they show the per-IP limits and web's 32 MB cap, not a distributed flood or Traefik's TLS cost.
Note: nothing is installed on the VPS; never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; never add `--remove-orphans`.
Note: PC steps in Git Bash (step 2 in the 7b-B worktree, the others in the main checkout); VPS steps as root over SSH (`ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`), run by the owner; browser steps in Chrome.

## Before merging

1. (On the VPS, read-only) Step A is live: the volume exists, the api mounts it, its root is owned by 65532, and `public.json` is version 1 and under 10 minutes old.
```bash
V=cute-trading_public-data; C='stat -c "%u %a" "$P"; stat -c "%u %a %s" "$P/public.json"; head -c 20 "$P/public.json"; echo; grep -o "\"written_ms\":[0-9]*" "$P/public.json"'; docker volume inspect -f '{{.Name}}' "$V" && docker inspect -f '{{range .Mounts}}{{.Destination}} rw={{.RW}}{{"\n"}}{{end}}' cute-trading-api-1 | grep ct-public && { if docker image inspect alpine >/dev/null 2>&1; then echo via-alpine; O=$(docker run --rm -e P=/v -v "$V":/v:ro alpine sh -c "$C"); else echo via-host-path; O=$(P=/var/lib/docker/volumes/$V/_data sh -c "$C"); fi; echo "$O"; W=$(echo "$O" | grep -o '"written_ms":[0-9]*' | cut -d: -f2); echo "age $(( ($(date +%s%3N) - ${W:-0}) / 1000 )) s"; }
```
→ expected: `cute-trading_public-data`; `/var/lib/ct-public rw=true`; `via-alpine` (or `via-host-path`); `65532 755`; `65532 644 <bytes below 65536>`; `{"v":1,"written_ms":`; `"written_ms":<ms>`; `age <n> s` with n below 600.
if not: `no such volume`, no `/var/lib/ct-public` line or `No such file` = step A is not deployed: run the step A runbook (`phase7b-api-vps.md`) first, do not merge; an owner other than `65532`, or modes other than `755` and `644` = nginx (uid 101) could not read the file: report (a step A fix); an age of 600 s or more = the writer is behind: `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web logs --since 15m api 2>&1 | grep 'public snapshot'` and report.
**STOP**

2. (On the PC, the 7b-B worktree) The branch is clean and pushed, and PR #15's CI is green.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b-B && git status --short --untracked-files=no && git log -1 --format='%h %s' && gh pr view --json number,state,headRefOid --jq '"#\(.number) \(.state) \(.headRefOid[0:7])"' && gh pr checks --json name,bucket --jq '.[] | "\(.name) \(.bucket)"' | sort
```
→ expected: no status lines, `<sha> <subject>`, `#15 OPEN <the same sha>`, then `compose pass`, `rust pass`, `wasm pass`, `web pass`.
if not: status lines = commit the listed changes and push; another sha on the `#15` line = `git push`, then run this step again; `pending` = wait and run again; `fail` = read the job's log on GitHub, fix on the branch, redo this step.
**STOP**

3. (On the PC, the main checkout) The owner merges PR #15 on GitHub (only after step 1 passed), then the main checkout is brought to the merge; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git checkout -q main && git pull -q --ff-only && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s' && grep -c 'ct-math' compose.web.yml && grep -c 'public-data' compose.web.yml && grep -c -e '^\.claude' -e '^web/src/wasm/pkg' .dockerignore
```
→ expected: no status lines, `main`, `<sha> Merge pull request #15 from isabellybaldo/worktree-phase7b-B`, then `14`, `4`, `2`.
if not: `Not possible to fast-forward` = local commits on `main`: report, do not build; another subject or other counts = the PR is not merged yet (or `main` moved on): merge it, then run this step again.
**STOP**

## On the PC: build and ship

4. Free the worktrees' build output, then the disk budget on the PC before building.
```bash
export PATH="$HOME/.cargo/bin:$PATH"; W=/c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees; for w in phase7b phase7b-B; do if cd "$W/$w" 2>/dev/null; then echo "$w: $(cargo clean 2>&1 | tail -1)"; else echo "$w: no worktree"; fi; done; cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: `phase7b: Removed <n> files, <x>GiB total` and the same for `phase7b-B` (`Removed 0 files` or `no worktree` are fine), a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `OVER BUDGET` = `docker builder prune -f --keep-storage 5GB`, then run this step again.

5. The release engine, the web tests with the engine required, both typechecks, both builds and the size check (needs the pinned `wasm-bindgen` CLI on the PC: 0.2.129, installed 2026-10-01; the first engine build prints nothing for a minute or two).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && ( set -o pipefail; sh scripts/build-wasm.sh 2>&1 | tail -5 ) && echo wasm-ok && cd web && npm ci --no-audit --no-fund >/dev/null && ( set -o pipefail; WASM_REQUIRED=1 npm test 2>&1 | grep -E '^ +(Test Files|Tests) ' ) && echo tests-ok && ( set -o pipefail; npm run build 2>&1 | grep -E 'B gzip|built in|^check-math-bundle|error' ) && echo build-ok
```
→ expected: ``Finished `release` profile [optimized] target(s) in <t>``, the four package files (`arb_wasm.d.ts`, `arb_wasm.js`, `arb_wasm_bg.wasm`, `arb_wasm_bg.wasm.d.ts`), `wasm-ok`, `Test Files  <n> passed (<n>)`, `Tests  <n> passed (<n>)` (70 and 577 on 2026-10-01), `tests-ok`, `✓ built in <t>` twice (both typechecks are silent), three `<file>: <n> B gzip (<n> B raw)` lines (one `.wasm`, one `.js`, one `.css`), `math-assets total: <n> B gzip (limit 350000)` (about 214 KB; any value up to 350000 passes), then `build-ok`.
if not: `build-wasm: the wasm-bindgen CLI is …` = the CLI is missing or another version: report (install only with the owner's OK, with the `cargo install` command that line prints), then this step again; `check-math-bundle: over the 350 KB gzip budget` or any other `check-math-bundle:` line = report the lines, do not build the image; a `failed` test, a tsc `error` line or a missing `tests-ok`/`build-ok` = fix on a branch, do not build the image.
**STOP**

6. Build the web image only (its first stage compiles `arb-wasm` and installs `wasm-bindgen-cli`: several minutes the first time; later builds reuse it).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && ( set -o pipefail; docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web 2>&1 | tail -3 ) && docker image inspect -f '{{.Created}}' cute-trading/web:dev
```
→ expected: the build's last lines without `ERROR`, then a UTC time from the last few minutes (a fully cached re-run keeps the earlier time).
if not: read the first `ERROR` line (`docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web 2>&1 | grep -m1 -B5 ERROR`): `over the 350 KB gzip budget` or `check-math-bundle:` = as step 5; `expected exactly one wasm-bindgen in Cargo.lock` = the tree is not the merged commit: redo step 3; otherwise fix, run again.

7. The image carries the dashboard, the math page, one js, css and wasm with a `.gz` beside each, no build-machine path in the wasm, no `/api/` in the math files, and a valid nginx config.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint sh cute-trading/web:dev -c 'cd /usr/share/nginx/html && ls index.html math.html && ls math-assets && for f in math-assets/*.js math-assets/*.css math-assets/*.wasm; do [ -f "$f.gz" ] || echo "NO GZ $f"; done; grep -c -e Users -e /home/ math-assets/*.wasm; grep -l "/api/" math-assets/*.js math-assets/*.css math.html | wc -l; nginx -t 2>&1 | tail -1'
```
→ expected: `index.html`, `math.html`, six names (`arb_wasm_bg-<hash>.wasm`, `arb_wasm_bg-<hash>.wasm.gz`, `math-<hash>.css`, `math-<hash>.css.gz`, `math-<hash>.js`, `math-<hash>.js.gz`), no `NO GZ` line, `0`, `0`, then `nginx: configuration file /etc/nginx/nginx.conf test is successful`.
if not: `No such file` for `math.html` or `math-assets` = the image was built from an old tree: redo steps 3 and 6; a `NO GZ` line, a first count above `0` (a path of the build machine in the public wasm) or a second count above `0` (the public bundle names the api) = report, do not ship; an nginx `[emerg]` line = report it (`docker/web/nginx.conf`), do not ship.
**STOP**

8. Bundle the image, both compose files and `.env.example`.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-7bB" && cd "$HOME/ct-7bB" && docker save cute-trading/web:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && tar -cf "$HOME/ct-7bB.tar" image.tar.gz compose.yml compose.web.yml .env.example && cd "$HOME" && rm -rf "$HOME/ct-7bB" && ls -l "$HOME/ct-7bB.tar" && tar -tvf "$HOME/ct-7bB.tar" && cd "$R"
```
→ expected: one `ls -l` line for `ct-7bB.tar` (roughly 15–45 MB), then four members with their sizes: `image.tar.gz` (nearly the whole size), `compose.yml`, `compose.web.yml`, `.env.example`.
if not: an `image.tar.gz` of a few bytes = `docker save` failed (is Docker running, does step 6's image exist): run this step again.

9. Ship the bundle.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7bB.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-7bB.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158; the bundle stays in `$HOME` until the copy succeeds.

## On the VPS: deploy

10. Rollback material, before anything changes: the tag `cute-trading/web:pre7b-B` from the running web container's image ID (only if the tag does not exist yet) and both compose files copied to `/root` (never overwritten on a re-run).
```bash
cd /home/isoca/projects/cute-trading && { docker image inspect cute-trading/web:pre7b-B >/dev/null 2>&1 || docker tag "$(docker inspect -f '{{.Image}}' cute-trading-web-1)" cute-trading/web:pre7b-B; } && { [ -e /root/compose.yml.pre7b-B ] || cp -p compose.yml /root/compose.yml.pre7b-B; } && { [ -e /root/compose.web.yml.pre7b-B ] || cp -p compose.web.yml /root/compose.web.yml.pre7b-B; } && docker image inspect -f '{{.Id}}' cute-trading/web:pre7b-B && docker inspect -f '{{.Image}}' cute-trading-web-1 && ls -l /root/compose.yml.pre7b-B /root/compose.web.yml.pre7b-B && grep -c 'public-data' /root/compose.web.yml.pre7b-B; grep -c 'ct-math' /root/compose.web.yml.pre7b-B
```
→ expected: two identical `sha256:…` lines (write the ID in Results), two `ls -l` lines, then `3` and `0` (step A's compose file: the api's mount, no public router).
if not: `No such object` = the web container is missing: `docker ps -a --format '{{.Names}} {{.Status}}'` and report; (a re-run after step 16) the second ID is the new web, so a different line is expected: leave `pre7b-B` as it is; counts `4` and `14` = the copy was taken after step 12's load: report, do not go on (there is no pre-deploy compose file to roll back to); a first count of `0` = step A's compose file is not on the VPS: redo step 1.
**STOP**

11. The alpine image, pulled now and visibly (before anything is recreated), then the volume read once more through it: still step A's, still fresh.
```bash
docker pull -q alpine; V=cute-trading_public-data; C='stat -c "%u %a" "$P"; stat -c "%u %a %s" "$P/public.json"; head -c 20 "$P/public.json"; echo; grep -o "\"written_ms\":[0-9]*" "$P/public.json"'; docker volume inspect -f '{{.Name}}' "$V" && { if docker image inspect alpine >/dev/null 2>&1; then echo via-alpine; O=$(docker run --rm -e P=/v -v "$V":/v:ro alpine sh -c "$C"); else echo via-host-path; O=$(P=/var/lib/docker/volumes/$V/_data sh -c "$C"); fi; echo "$O"; W=$(echo "$O" | grep -o '"written_ms":[0-9]*' | cut -d: -f2); echo "age $(( ($(date +%s%3N) - ${W:-0}) / 1000 )) s"; }
```
→ expected: `docker.io/library/alpine:latest`; `cute-trading_public-data`; `via-alpine`; `65532 755`; `65532 644 <bytes below 65536>`; `{"v":1,"written_ms":`; `"written_ms":<ms>`; `age <n> s` with n below 600.
if not: a pull error and `via-host-path` = no alpine image: install nothing, the block has read the volume from its host path (`/var/lib/docker/volumes/cute-trading_public-data/_data`) and the same lines are expected; `no such volume`, `No such file`, another owner or other modes = do not go on (never let `web` create or be the first to mount the volume): report; an age of 600 s or more = the writer is behind: `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web logs --since 15m api 2>&1 | grep 'public snapshot'`, write it in Results and go on (the page will say the writer is behind).
**STOP**

12. Load the bundle (replaces `compose.yml`, `compose.web.yml` and `.env.example`; moves `web:dev`; `web:pre7b-B`, `web:pre7b` and `web:pre7a` stay), then the disk budget.
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-7bB.tar && rm /root/ct-7bB.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls cute-trading/web --format '{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/web:dev`; `dev` on a new ID and `pre7b-B` on step 10's (older tags may be listed too); then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: `Cannot open: No such file` = the bundle was already loaded (a re-run) or never shipped: go on to step 13 if `dev` and `pre7b-B` differ in `docker image ls cute-trading/web --format '{{.Tag}} {{.ID}}'`, otherwise redo steps 8–9; a truncated tar = bundle and ship again (steps 8–9), then this step; `OVER BUDGET` = `docker image prune -f`, then `LIMIT_GB=75 sh scripts/disk-budget.sh` again.
**STOP**

13. The loaded files are the expected ones: compose parses, `compose.yml` is unchanged, and `compose.web.yml` differs from the saved copy by exactly 32 added lines and 2 removed ones.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok; cmp -s compose.yml /root/compose.yml.pre7b-B && echo compose-yml-same; DIFF=$(diff --strip-trailing-cr /root/compose.web.yml.pre7b-B compose.web.yml); echo "$DIFF" | grep -c '^>'; echo "$DIFF" | grep -c '^<'; echo "$DIFF" | grep '^<'; echo "$DIFF" | grep '^>' | grep -v '^> *#' | cut -c1-100
```
→ expected: `config-ok`; `compose-yml-same`; `32`; `2`; the two removed lines, both comments (`< # Every router except oauth2-proxy's own /oauth2/* pages goes through forward-auth (spec §3).` and `<       # oauth2-proxy's own pages: the only route that answers without a session`); then the 16 added lines that are not comments: `>     volumes:`, `>       - public-data:/srv/ct-public-root/math-data:ro,nocopy`, `>       - traefik.http.routers.ct-web.service=ct-web`, and 13 label lines naming `ct-math` (the router's `rule`, `entrypoints`, `tls.certresolver`, `service=ct-math`, `services.ct-math.loadbalancer.server.port=8081`, `priority=1000`, `middlewares`; `ct-math-rl` `average=5`, `burst=20`, `sourcecriterion.ipstrategy.depth=0`; `ct-math-if` `inflightreq.amount=8` and its `depth=0`; `ct-math-csp`).
if not: no `config-ok` = read the error line, report, do not go on; no `compose-yml-same` = `diff --strip-trailing-cr /root/compose.yml.pre7b-B compose.yml`: no output = only the line endings differ: go on; any line = report, do not go on (7b-B does not change `compose.yml`); other counts or other lines = `diff --strip-trailing-cr /root/compose.web.yml.pre7b-B compose.web.yml`, report, do not go on (to put the files and the `web:dev` tag back: `cp -p /root/compose.yml.pre7b-B compose.yml && cp -p /root/compose.web.yml.pre7b-B compose.web.yml && docker tag cute-trading/web:pre7b-B cute-trading/web:dev`; no container has changed yet).
**STOP**

14. Recreate `oauth2-proxy` on its own, before the web (its scope becomes `user:email`; the dashboard answers errors for a few seconds while its middlewares are away); already on that scope, compose leaves it running and the check still runs.
```bash
cd /home/isoca/projects/cute-trading && S() { curl -s -o /dev/null -D - https://radar.isoca.space/oauth2/start | tr -d '\r' | grep -i '^location:' | grep -oE 'https://[^/]+|scope=[^&]*' | tr '\n' ' '; echo; } && echo "before: $(S)" && date +%s%3N > /root/7bB_oauth_ms && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps oauth2-proxy && sleep 10 && docker inspect -f '{{.State.Status}} {{.RestartCount}}' cute-trading-oauth2-proxy-1 && echo "after: $(S)"
```
→ expected: `before: https://github.com scope=user%3Aemail+read%3Aorg` (or already `scope=user%3Aemail`); `Container cute-trading-oauth2-proxy-1 … Recreated` then `Started` (or `Running`, already on the new scope), and no line for another container; `running 0`; `after: https://github.com scope=user%3Aemail` with no `read%3Aorg`; then the owner reloads the dashboard in Chrome and is still logged in (the cookie secret is unchanged).
if not: not `running`, or a restart count above 0 = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 oauth2-proxy` and report (the dashboard and `/math` stay closed until it runs); `after:` still with `read%3Aorg` = `docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps --force-recreate oauth2-proxy`, then `curl -s -o /dev/null -D - https://radar.isoca.space/oauth2/start | grep -io 'scope=[^&]*'`; an empty `after:` = the proxy is not answering yet: run that `curl` again after 10 s; `before:` and `after:` both empty while it is `running 0` = the VPS cannot reach its own name: run the same check from the PC, `curl -s -o /dev/null -D - https://radar.isoca.space/oauth2/start | grep -io 'scope=[^&]*'` (expected `scope=user%3Aemail`); the dashboard asks for a login = log in once and write it in Results.
**STOP**

15. A quiet minute (advisory): the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`; run step 16 within 2 minutes.
if not: `f` = a busy minute: run this step again every few minutes; after 15 minutes without a `t`, go on to step 16 anyway and write "no quiet minute in 15 min" in Results (step 17 records the reconnects); an empty line = no health rows in 24 h: report before going on.

16. Record the start time, then recreate `web` only: it must be up, on the new image, with the new labels and the volume read-only.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7bB_web_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/7bB_scanner_started && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && sleep 5 && docker inspect -f '{{.State.Status}} {{.RestartCount}}' cute-trading-web-1 && { N=$(docker image inspect -f '{{.Id}}' cute-trading/web:dev); O=$(docker image inspect -f '{{.Id}}' cute-trading/web:pre7b-B); R=$(docker inspect -f '{{.Image}}' cute-trading-web-1); [ "$R" = "$N" ] && [ "$R" != "$O" ] && echo web-on-new-image; docker inspect -f '{{range .Mounts}}{{.Destination}} rw={{.RW}}{{"\n"}}{{end}}{{index .Config.Labels "traefik.http.routers.ct-math.service"}} {{index .Config.Labels "traefik.http.services.ct-math.loadbalancer.server.port"}}' cute-trading-web-1; docker exec cute-trading-web-1 sh -c 'id -u; head -c 20 /srv/ct-public-root/math-data/public.json; echo; touch /srv/ct-public-root/math-data/x 2>&1'; }
```
→ expected: the scanner's start time; `Container cute-trading-web-1 … Recreated` then `Started`, and no line for another container or for a volume; `running 0`; `web-on-new-image`; `/srv/ct-public-root/math-data rw=false`; `ct-math 8081`; `101`; `{"v":1,"written_ms":`; `touch: /srv/ct-public-root/math-data/x: Read-only file system`.
if not: `Volume "cute-trading_public-data" Created` = the volume did not exist (steps 1 and 11 were skipped): roll back now (step 31) and report; not `running`, or a restart count above 0 = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web`, then roll back (step 31); no `web-on-new-image` = the container runs the old image under the new labels (`/math` then answers 502: it fails closed): `docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps --force-recreate web && sleep 5 && docker image inspect -f '{{.Id}}' cute-trading/web:dev && docker inspect -f '{{.Image}} {{.State.Status}}{{range .Mounts}} {{.Destination}} rw={{.RW}}{{end}}' cute-trading-web-1 && docker exec cute-trading-web-1 sh -c 'id -u; head -c 20 /srv/ct-public-root/math-data/public.json; echo'` (expected: the `web:dev` ID, the same ID with `running /srv/ct-public-root/math-data rw=false`, `101`, `{"v":1,"written_ms":`; still another ID = roll back, step 31, and report); `rw=true`, no `ct-math 8081` line or `Permission denied` on `head` = roll back (step 31) and report.
**STOP**

17. The scanner is unaffected: its start time is unchanged; the reconnects since each recreate are recorded (not a gate), and the memory baseline for the floods.
```bash
cd /home/isoca/projects/cute-trading && sleep 180; docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7bB_scanner_started && echo scanner-not-restarted; for f in oauth web; do echo "$f $(docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $(cat /root/7bB_${f}_ms)")"; done; docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' cute-trading-web-1 cute-trading-api-1 cute-trading-scanner-1 cute-trading-postgres-1
```
→ expected: `scanner-not-restarted`; `oauth <n>` and `web <n>` (any numbers: write them in Results); four `<name> <used> / <limit>` lines (write them in Results), `web` far below `32MiB`.
if not: a `diff` line instead of `scanner-not-restarted` = the scanner restarted: `docker compose --profile core logs --tail 50 scanner` and report; reconnects above 0: do NOT roll back, write the count and the UTC time in Results.

## Checks

18. (On the PC, no cookie) The public paths answer without login: the page with the math CSP and the security headers, the js, css and wasm gzipped and cached, and the snapshot as fresh JSON.
```bash
D=https://radar.isoca.space; H=$(curl -s --path-as-is "$D/math"); JS=$(printf '%s' "$H" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.js' | head -1); CSS=$(printf '%s' "$H" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.css' | head -1); WASM=$(curl -s --path-as-is "$D$JS" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.wasm' | head -1); echo "$JS $CSS $WASM"; h() { echo "== $1"; curl -s --path-as-is -o /dev/null -D - -H 'Accept-Encoding: gzip' "$D$1" | tr -d '\r' | grep -iE "^(HTTP|$2)"; }; h /math 'content-type|cache-control|content-security-policy|strict-transport-security|x-content-type-options|x-frame-options'; for a in "$JS" "$CSS" "$WASM" /math-data/public.json; do h "$a" 'content-type|content-encoding|cache-control'; done; curl -s --path-as-is --compressed "$D/math-data/public.json" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);console.log("v",j.v,"age_min",Math.round((Date.now()-j.written_ms)/60000),"keys",Object.keys(j).join(","),"leak",/account_id|"host"/.test(s))})'
```
→ expected: `/math-assets/math-<hash>.js /math-assets/math-<hash>.css /math-assets/arb_wasm_bg-<hash>.wasm` (step 7's names); `== /math`: `HTTP/2 200` (the protocol may read `HTTP/1.1`), `content-type: text/html`, `cache-control: no-cache`, `content-security-policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`, `strict-transport-security: max-age=31536000`, `x-content-type-options: nosniff`, `x-frame-options: DENY`; the js, the css and the wasm: `HTTP/2 200`, `content-type: application/javascript`, `text/css` and `application/wasm`, each with `content-encoding: gzip` and `cache-control: public, max-age=31536000, immutable`; `== /math-data/public.json`: `HTTP/2 200`, `content-type: application/json`, `content-encoding: gzip`, `cache-control: public, max-age=60`; then `v 1 age_min <0–10> keys v,written_ms,ladder,today leak false`.
if not: `/math` `302` = the public router is not picked up: on the VPS `docker inspect -f '{{json .Config.Labels}}' cute-trading-web-1 | grep -o 'ct-math[^,]*' | head -5`, report; `502` = the web runs the old image under the new labels: step 16's `--force-recreate`; `404` on `public.json` = the mount or the file (step 16); the wasm without `application/wasm`, or a CSP other than the line above = report; only a missing `content-encoding` = write it in Results and go on; `leak true` or other keys = on the VPS roll back (step 31) and report (a step A whitelist bug); `429` = too many requests from this IP in a few seconds: wait 10 s and run again.
**STOP**

19. (On the PC, no cookie) Everything else still goes to the GitHub login.
```bash
D=https://radar.isoca.space; for p in / /index.html /assets/x.js /sweet /api/today /api/paper/ladder /math/ /math.html /math-assets /math-data/ /math-data/public.json.tmp /math-data/x.json; do printf '%s ' "$p"; curl -sS --path-as-is -o /dev/null -w '%{http_code} %{redirect_url}\n' "$D$p" | cut -d'?' -f1; done
```
→ expected: twelve lines, each `<path> 302 https://github.com/login/oauth/authorize` (the two `/api/` paths may answer `401` with no URL); never `200`.
if not: any `200` = a path is public that must not be: on the VPS roll back (step 31) and report; `/` or `/sweet` still `200` after the rollback = forward-auth is off: on the VPS `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web stop web` and report.
**STOP**

20. (On the PC, no cookie) Traversal probes: no crafted path under the public prefix reaches a dashboard file or the snapshot's tmp file.
```bash
D=https://radar.isoca.space; T=$(mktemp); for p in '/math-assets/..%2findex.html' '/math-assets/%2e%2e%2findex.html' '/math-assets/..%2fassets%2fx.js' '/math-assets/..%2fmath-data%2fpublic.json.tmp' '/math%2Dassets/..%2findex.html' '/math-assets/..%252findex.html' '/math-assets/../index.html'; do C=$(curl -sS --path-as-is -o "$T" -w '%{http_code}' "$D$p"); printf '%s %s %s\n' "$p" "$C" "$(head -c 60 "$T" | tr -d '\r' | tr '\n' ' ')"; done; rm -f "$T"
```
→ expected: seven lines `<path> <status> <first 60 bytes of the body>`: the six encoded probes `404` (body `<html> <head><title>404 Not Found</title></head>…` or `404 page not found`) or `400`; `/math-assets/../index.html` `302` (Traefik cleans the path, so it is gated; body `<a href="https://github.com/login/oauth/authorize?…`) or `404` (`400` is fine too: Traefik rejected the path); a `302` on an encoded probe is gated too; never `200`.
if not: any `200` = a crafted path reaches a file without login: on the VPS roll back NOW (step 31), then report the line and its body; `429` = too many requests from this IP in a few seconds: wait 10 s and run again.
**STOP**

21. (On the PC) Plain HTTP redirects to https.
```bash
curl -sS --path-as-is -o /dev/null -w '%{http_code} %{redirect_url}\n' http://radar.isoca.space/math
```
→ expected: `301 https://radar.isoca.space/math` (or `308`).
if not: `200` over plain HTTP = report (the `ct-http` router on `oauth2-proxy`).

22. (On the VPS) Start the memory sampler for the floods: one `docker stats` round every few seconds for about 10 minutes, in the background (an earlier sampler from a previous run of this step is stopped first); run steps 23–25 right after.
```bash
P=$(cat /root/7bB_flood.pid 2>/dev/null); [ -n "$P" ] && grep -qa 'docker stats' "/proc/$P/cmdline" 2>/dev/null && kill "$P" && echo "stopped the earlier sampler $P"; sleep 3; : > /root/7bB_flood.txt; nohup sh -c 'for i in $(seq 1 90); do date -u +%T; docker stats --no-stream --format "{{.Name}} {{.MemUsage}} {{.CPUPerc}}" cute-trading-web-1 cute-trading-api-1 cute-trading-scanner-1 cute-trading-postgres-1; sleep 5; done' >> /root/7bB_flood.txt 2>&1 & echo $! > /root/7bB_flood.pid; sleep 10; cat /root/7bB_flood.txt
```
→ expected: a job line (`[1] <pid>`), then a UTC time and four `<name> <used> / <limit> <cpu>%` lines (the first round, before the flood); on a re-run within 10 minutes, `stopped the earlier sampler <pid>` first (a `Terminated` job notice may follow).
if not: an empty file = `docker stats` is slow: `cat /root/7bB_flood.txt` again after 10 s.

23. (On the PC) Flood the wasm from one IP (`oha` for 60 s if it is installed, otherwise 3,000 requests, 50 at a time), while the owner reloads the dashboard in Chrome (logged in).
```bash
D=https://radar.isoca.space; JS=$(curl -s --path-as-is "$D/math" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.js' | head -1); W=$(curl -s --path-as-is "$D$JS" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.wasm' | head -1); U="$D$W"; echo "$U"; date -u +%T; if [ -z "$JS" ] || [ -z "$W" ]; then echo no-wasm-url; elif command -v oha >/dev/null; then oha -z 60s -c 50 --no-tui "$U" | grep -A8 'Status code distribution'; else seq 1 3000 | xargs -P 50 -I{} curl -s --path-as-is -m 20 -o /dev/null -w '%{http_code}\n' "$U" | sort | uniq -c; fi; date -u +%T
```
→ expected: the wasm URL, a UTC time, then the status counts: mostly `429`, some `200` (a few `000` in the curl loop are dropped connections), then the end time; the dashboard still loads `/` during the run.
if not: `no-wasm-url` = `/math` did not answer (step 18); only `200` = the per-IP limit is not applied: report (the `ct-math-rl` and `ct-math-if` labels), no rollback; the dashboard does not load = write it in Results and look at step 26's numbers.

24. (On the PC) Flood the snapshot the same way (the file is served by nginx: the api never sees these requests).
```bash
U=https://radar.isoca.space/math-data/public.json; echo "$U"; date -u +%T; if command -v oha >/dev/null; then oha -z 60s -c 50 --no-tui "$U" | grep -A8 'Status code distribution'; else seq 1 3000 | xargs -P 50 -I{} curl -s --path-as-is -m 20 -o /dev/null -w '%{http_code}\n' "$U" | sort | uniq -c; fi; date -u +%T
```
→ expected: as step 23: mostly `429`, some `200`; the dashboard still loads.
if not: as step 23.

25. (On the PC) Flood `/math` the same way, while a phone on mobile data (another IP) opens `https://radar.isoca.space/math`.
```bash
U=https://radar.isoca.space/math; echo "$U"; date -u +%T; if command -v oha >/dev/null; then oha -z 60s -c 50 --no-tui "$U" | grep -A8 'Status code distribution'; else seq 1 3000 | xargs -P 50 -I{} curl -s --path-as-is -m 20 -o /dev/null -w '%{http_code}\n' "$U" | sort | uniq -c; fi; date -u +%T
```
→ expected: as step 23; during the run the phone loads the page with its drawings (the limits are per IP).
if not: the phone gets `429` = the limiter is not per IP: report (the `sourcecriterion.ipstrategy.depth=0` labels), no rollback.

26. (On the VPS) The sampler's numbers over the three floods: memory range and top CPU per container; the web was not killed, the scanner not restarted.
```bash
awk 'NF==5 && $3=="/" { v=$2+0; if ($2 ~ /GiB/) v*=1024; else if ($2 ~ /KiB/) v/=1024; else if ($2 !~ /MiB/) v/=1048576; c=$5+0; if (!($1 in n) || v<mn[$1]) mn[$1]=v; if (v>mx[$1]) mx[$1]=v; if (c>cx[$1]) cx[$1]=c; n[$1]++ } END { for (k in n) printf "%s samples %d mem min %.1f max %.1f MiB cpu max %.1f%%\n", k, n[k], mn[k], mx[k], cx[k] }' /root/7bB_flood.txt | sort; docker inspect -f '{{.State.OOMKilled}} {{.RestartCount}}' cute-trading-web-1; docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7bB_scanner_started && echo scanner-not-restarted
```
→ expected: four lines `cute-trading-<service>-1 samples <n> mem min <x> max <y> MiB cpu max <z>%` (write them in Results): `web` max at or under 32 MiB, `api`, `scanner` and `postgres` max within a few MiB of step 17's baseline and the api's CPU flat; `false 0`; `scanner-not-restarted`.
if not: `true`, or a restart count above 0 = the web hit its 32 MB cap under the flood: roll back (step 31) and report (a separate nginx for the public page is the owner's call); the api, scanner or postgres moved with the floods = report the lines; fewer samples than the floods lasted = the sampler ended early: redo steps 22–25.
**STOP**

27. (Chrome, a private window: not logged in; DevTools open on Network, cache disabled) open `https://radar.isoca.space/math` once.
→ expected: every request `200`, none `429`: five same-origin requests (`math`, `math-<hash>.js`, `math-<hash>.css`, `arb_wasm_bg-<hash>.wasm` with type `wasm`, exactly one `public.json`), plus Google Fonts (`fonts.googleapis.com`, `fonts.gstatic.com`); no `/api/` request; Console: no CSP violation, no error.
if not: a `429` on a single load = report (the in-flight limit is 8, the burst 20), wait a minute after the floods and try once more; a CSP violation = report the directive it names.

28. (Chrome, window 1440 px wide, not logged in) `https://radar.isoca.space/math`, top to bottom; then open `https://radar.isoca.space/math#d` in a new tab; then once with JavaScript disabled (DevTools, Run command, `Disable JavaScript`, then reload).
→ expected: the header `isoca-radar · math`; the live strip (`today on the live scanner`, the NO and YES counts, the `edge on the table today` figure, the bars by hour, `as of HH:MM UTC`); sections a to f in order, each with its drawing, and each drawing changes when a control is moved; drawing a's price axis is zoomed onto the book, so its mirror lines are clearly separate lines; drawing d's stairs fill most of the chart's height, and with a fourth leg added its labels stay readable; section f starts on `filled, +$2.50 on paper`, `the usual burst` turns it into `caught, and lost $6.00 on paper`, its three legs' labels are legible, and under it the ladder headed `on the live scanner, as of HH:MM UTC` with its `which markets` fold; the footer `The live dashboard is private. The math is not.`; `/math#d` on a cold load scrolls to section d; with JavaScript disabled the page shows only `This page draws with WebAssembly and needs JavaScript.`
if not: `live numbers unavailable` instead of the strip = the snapshot did not load or did not parse: step 18's last line, report; a label stretched, cut or drawn over another = screenshot and report (a web fix on a branch; no rollback needed); `The math engine did not load: the drawings need WebAssembly.` = report the Console error.
Note: if the api's writer stops, the page says `the live writer is behind` once the snapshot is over 30 minutes old and the tab is revisited; nothing to do here.

29. (Chrome, the same window at 1440 px, then DevTools device toolbar at 360 × 740) `/math` with every fold open; in the console at each width:
```js
document.documentElement.scrollWidth <= window.innerWidth
```
→ expected: `true` at both widths; at 360 px the sections stay one column, the controls work by touch, no label is cut, and the axis labels of the live ladder under f do not collide.
if not: `false` = horizontal scroll: screenshot and report (a layout fix on a branch; no rollback needed).

30. (Chrome, logged in as the owner) the dashboard itself: `https://radar.isoca.space/`, then `/paper`, then `/episodes`: click an episode's title (the link in the event column; `/episodes/<id>` opens), click the `EXPLAIN ▸` tab, then the question `Would we have caught it?`.
→ expected: the three pages and the episode detail load with live numbers; the explain panel opens; under `At what latency would this pay?` (first on every page) the axis label `extras` is whole (never `extra` / `s`); under `Would we have caught it?` (on an episode's detail page only) the six rung labels `in time` / `too late` each sit on two lines without touching (PR #12's label fixes ride in this image).
if not: a page that does not load = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web` on the VPS (in `/home/isoca/projects/cute-trading`) and report; a broken label = screenshot and report (no rollback needed).
**STOP**

## Rollback

31. Back to the web image and the `compose.web.yml` that ran before this deploy (the api and the volume stay on step A; `oauth2-proxy` stays on the `user:email` scope). At a quiet minute (step 15) when there is one.
```bash
cd /home/isoca/projects/cute-trading && cp -p /root/compose.web.yml.pre7b-B compose.web.yml && docker tag cute-trading/web:pre7b-B cute-trading/web:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && sleep 5 && docker image inspect -f '{{.Id}}' cute-trading/web:pre7b-B && docker inspect -f '{{.Image}} {{.State.Status}}' cute-trading-web-1 && grep -c 'ct-math' compose.web.yml; for p in /math /math-data/public.json /; do printf '%s ' "$p"; curl -sS --path-as-is -o /dev/null -w '%{http_code}\n' "https://radar.isoca.space$p"; done
```
→ expected: `Container cute-trading-web-1 … Recreated` then `Started`; the `pre7b-B` ID, the same ID with `running`; `0`; `/math 302`, `/math-data/public.json 302`, `/ 302` (nothing public any more; `404` is fine too, never `200`); then the owner reloads the dashboard in Chrome: it loads.
if not: another ID on the container = `docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps --force-recreate web && sleep 5 && docker image inspect -f '{{.Id}}' cute-trading/web:pre7b-B && docker inspect -f '{{.Image}} {{.State.Status}}' cute-trading-web-1 && curl -sS -o /dev/null -w '%{http_code}\n' https://radar.isoca.space/math` (expected: the `pre7b-B` ID, the same ID with `running`, `302`); `000` on the three paths = the VPS cannot reach its own name: run the same check from the PC, `for p in /math /math-data/public.json /; do printf '%s ' "$p"; curl -sS --path-as-is -o /dev/null -w '%{http_code}\n' "https://radar.isoca.space$p"; done` (expected `302` three times); not `running` = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web` and report; `/math 200` = the old compose file is not in place: `grep -c 'ct-math' compose.web.yml` must print `0`, then this step again.
Note: the compose file is restored first: the old image under the new labels would send `/math` to port 8081, where the old nginx does not listen (502: it fails closed), and only the old labels put the routes back exactly as before.
Note: `oauth2-proxy` needs no rollback (the saved compose file carries the same scope line); to put the provider's default scope back anyway: `sed -i '/OAUTH2_PROXY_SCOPE:/d' compose.web.yml && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps oauth2-proxy`.

## Results

- Step A at the start (step 1): owner, modes, snapshot age:
- PR CI and the merged commit (steps 2–3):
- Engine tests, and the gzip total of the math assets (step 5):
- Rollback tag ID = the running web's ID before the deploy; compose copies saved (step 10):
- Diff of compose.web.yml: added / removed lines (step 13; expected 32 / 2):
- oauth2-proxy scope before and after; still logged in (step 14):
- Quiet minute (step 15), UTC time, or "no quiet minute in 15 min":
- New web image ID, mount read-only (step 16):
- Reconnects since the oauth2-proxy recreate / since the web recreate; baseline memory web / api / scanner / postgres (step 17):
- Public paths (step 18): status and headers of /math, js, css, wasm, public.json; snapshot age:
- Gated paths (step 19): the twelve statuses:
- Traversal probes (step 20): the seven statuses:
- Plain HTTP (step 21):
- Flood wasm (step 23): status counts, 429 share, dashboard loaded:
- Flood public.json (step 24): status counts, 429 share:
- Flood /math (step 25): status counts, 429 share, phone loaded:
- Memory and CPU over the floods (step 26): web max MiB, api / scanner / postgres range, OOM and restarts:
- Page load in Chrome (step 27): requests, any 429:
- Chrome 1440 / 360 (steps 28–29): mirror lines in a, stairs and leg labels in d, leg labels in f, the live ladder's axis at 360 px, cold `#d`, scroll check, JavaScript off:
- Dashboard and the explain panel's labels (step 30): `extras`, `in time` / `too late`:
