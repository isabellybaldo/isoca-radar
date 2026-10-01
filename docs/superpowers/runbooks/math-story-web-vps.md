# The /math story on the VPS: the web image only (the public page becomes a story, then a playground)

Note: this deploy changes one container: `web` (new image); `compose.yml` and `compose.web.yml` on the VPS already equal main's and must stay byte-for-byte the same (step 11 checks it); `oauth2-proxy`, the api, scanner, analyze, compact and postgres are not touched.
Note: nothing about routes, limits, headers or the snapshot changes: `/math`, `/math-assets/…` and `/math-data/public.json` stay public through `ct-math` (port 8081), everything else stays behind the GitHub login; the flood test is not repeated.
Note: the asset file names carry a hash and change with this build: every check derives them from `/math`, none is typed here.
Note: the new page is recognised by one string in its js bundle, `Try it yourself` (the playground's heading): steps 6, 13 and 15 expect it, the rollback (step 21) expects it gone.
Note: rollback material: the tag `cute-trading/web:pre-story` (from the running container's image ID; `web:pre7b-B`, `web:pre7b` and `web:pre7a` stay untouched) and `/root/compose.yml.pre-story`, `/root/compose.web.yml.pre-story`.
Note: the quiet minute (step 12) is advisory: at most 15 minutes of waiting, then go on and record the reconnects.
Note: nothing is installed on the VPS; never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; never add `--remove-orphans`.
Note: PC steps in Git Bash (step 1 in the `math-story` worktree, the others in the main checkout); VPS steps as root over SSH (`ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`), in `/home/isoca/projects/cute-trading`, run by the owner; browser steps in Chrome.

## Before merging

1. (On the PC, the `math-story` worktree) The branch is clean and pushed, and its PR's CI is green.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/math-story && git status --short --untracked-files=no && git log -1 --format='%h %s' && gh pr view --json number,state,headRefOid --jq '"#\(.number) \(.state) \(.headRefOid[0:7])"' && gh pr checks --json name,bucket --jq '.[] | "\(.name) \(.bucket)"' | sort
```
→ expected: no status lines, `<sha> <subject>`, `#<n> OPEN <the same sha>`, then `compose pass`, `rust pass`, `wasm pass`, `web pass`.
if not: status lines = commit the listed changes and push; `no pull requests found` = the PR is not open yet: tell Claude; another sha on the `#<n>` line = `git push`, then run this step again; `pending` = wait and run again; `fail` = read the job's log on GitHub, fix on the branch, redo this step.
**STOP**

2. (On the PC, the main checkout) The owner merges the PR on GitHub, then the main checkout is brought to the merge; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git checkout -q main && git pull -q --ff-only && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s' && grep -c 'Try it yourself' web/src/math/story/copy.ts && grep -c 'ct-math' compose.web.yml && grep -c 'public-data' compose.web.yml && grep -c -e '^\.claude' -e '^web/src/wasm/pkg' .dockerignore
```
→ expected: no status lines, `main`, `<sha> Merge pull request #<n> from isabellybaldo/<the PR's branch>` (`math-story` unless the branch was pushed under another name), then `1` (the story's copy is on `main`), `14`, `4`, `2`.
if not: `Not possible to fast-forward` = local commits on `main`: report, do not build; `No such file or directory` for `copy.ts`, or another subject = the PR is not merged yet (or `main` moved on): merge it, then run this step again; counts other than `14`, `4`, `2` = a compose file or `.dockerignore` changed on `main`: report, do not build (this deploy ships both compose files unchanged).
**STOP**

## On the PC: build and ship

3. Free the worktrees' build output, then the disk budget on the PC before building.
```bash
export PATH="$HOME/.cargo/bin:$PATH"; W=/c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees; for w in phase7b phase7b-B math-story; do if cd "$W/$w" 2>/dev/null; then echo "$w: $(cargo clean 2>&1 | tail -1)"; else echo "$w: no worktree"; fi; done; cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: three lines, one per worktree: `<name>: Removed <n> files, <x>GiB total` (`Removed 0 files` or `<name>: no worktree` are fine), a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `OVER BUDGET` = `docker builder prune -f --keep-storage 5GB`, then run this step again.

4. The release engine, the web tests with the engine required, both typechecks, both builds and the size check (needs the pinned `wasm-bindgen` CLI on the PC: 0.2.129, installed 2026-10-01; the first engine build prints nothing for a minute or two).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && ( set -o pipefail; sh scripts/build-wasm.sh 2>&1 | tail -5 ) && echo wasm-ok && cd web && npm ci --no-audit --no-fund >/dev/null && ( set -o pipefail; WASM_REQUIRED=1 npm test 2>&1 | grep -E '^ +(Test Files|Tests) ' ) && echo tests-ok && ( set -o pipefail; npm run build 2>&1 | grep -E 'B gzip|built in|^check-math-bundle|error' ) && echo build-ok
```
→ expected: ``Finished `release` profile [optimized] target(s) in <t>``, the four package files (`arb_wasm.d.ts`, `arb_wasm.js`, `arb_wasm_bg.wasm`, `arb_wasm_bg.wasm.d.ts`), `wasm-ok`, `Test Files  <n> passed (<n>)`, `Tests  <n> passed | 1 skipped (<n>)` with no `failed` (the skipped one is the real-snapshot test, which runs only with `PUBLIC_JSON`; the counts grow with the suite: at least 77 files on 2026-10-01), `tests-ok`, `✓ built in <t>` twice (both typechecks are silent), three `<file>: <n> B gzip (<n> B raw)` lines (one `.wasm`, one `.js`, one `.css`), `math-assets total: <n> B gzip (limit 350000)` (any value up to 350000 passes; write it in Results), then `build-ok`.
if not: `build-wasm: the wasm-bindgen CLI is …` = the CLI is missing or another version: report (install only with the owner's OK, with the `cargo install` command that line prints), then this step again; `check-math-bundle: over the 350 KB gzip budget` or any other `check-math-bundle:` line = report the lines, do not build the image; a `failed` test, a tsc `error` line or a missing `tests-ok`/`build-ok` = fix on a branch, do not build the image.
**STOP**

5. Build the web image only (its first stage compiles `arb-wasm`: several minutes when the cache is cold; later builds reuse it).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && ( set -o pipefail; docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web 2>&1 | tail -3 ) && docker image inspect -f '{{.Created}}' cute-trading/web:dev
```
→ expected: the build's last lines without `ERROR`, then a UTC time from the last few minutes (a fully cached re-run keeps the earlier time).
if not: read the first `ERROR` line (`docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web 2>&1 | grep -m1 -B5 ERROR`): `over the 350 KB gzip budget` or `check-math-bundle:` = as step 4; `expected exactly one wasm-bindgen in Cargo.lock` = the tree is not the merged commit: redo step 2; otherwise fix, run again.

6. The image carries the dashboard, the math page, one js, css and wasm with a `.gz` beside each, no build-machine path in the wasm, no `/api/` in the math files, the new page's heading in the js, and a valid nginx config.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint sh cute-trading/web:dev -c 'cd /usr/share/nginx/html && ls index.html math.html && ls math-assets && for f in math-assets/*.js math-assets/*.css math-assets/*.wasm; do [ -f "$f.gz" ] || echo "NO GZ $f"; done; grep -c -e Users -e /home/ math-assets/*.wasm; grep -l "/api/" math-assets/*.js math-assets/*.css math.html | wc -l; grep -c "Try it yourself" math-assets/*.js; nginx -t 2>&1 | tail -1'
```
→ expected: `index.html`, `math.html`, six names (`arb_wasm_bg-<hash>.wasm`, `arb_wasm_bg-<hash>.wasm.gz`, `math-<hash>.css`, `math-<hash>.css.gz`, `math-<hash>.js`, `math-<hash>.js.gz`), no `NO GZ` line, `0`, `0`, `1` (any number from 1 up), then `nginx: configuration file /etc/nginx/nginx.conf test is successful`.
if not: `No such file` for `math.html` or `math-assets`, or a third count of `0` (the image holds the old page) = the image was built from an old tree: redo steps 2 and 5; a `NO GZ` line, a first count above `0` (a path of the build machine in the public wasm) or a second count above `0` (the public bundle names the api) = report, do not ship; an nginx `[emerg]` line = report it (`docker/web/nginx.conf`), do not ship.
**STOP**

7. Bundle the image, both compose files and `.env.example`.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-story" && cd "$HOME/ct-story" && docker save cute-trading/web:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && tar -cf "$HOME/ct-story.tar" image.tar.gz compose.yml compose.web.yml .env.example && cd "$HOME" && rm -rf "$HOME/ct-story" && ls -l "$HOME/ct-story.tar" && tar -tvf "$HOME/ct-story.tar" && cd "$R"
```
→ expected: one `ls -l` line for `ct-story.tar` (roughly 15–45 MB; write the size in Results), then four members with their sizes: `image.tar.gz` (nearly the whole size), `compose.yml`, `compose.web.yml`, `.env.example`.
if not: an `image.tar.gz` of a few bytes = `docker save` failed (is Docker running, does step 5's image exist): run this step again.

8. Ship the bundle.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-story.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-story.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158; the bundle stays in `$HOME` until the copy succeeds.

## On the VPS: deploy

9. Rollback material, before anything changes: the tag `cute-trading/web:pre-story` from the running web container's image ID (only if the tag does not exist yet) and both compose files copied to `/root` (never overwritten on a re-run).
```bash
cd /home/isoca/projects/cute-trading && { docker image inspect cute-trading/web:pre-story >/dev/null 2>&1 || docker tag "$(docker inspect -f '{{.Image}}' cute-trading-web-1)" cute-trading/web:pre-story; } && { [ -e /root/compose.yml.pre-story ] || cp -p compose.yml /root/compose.yml.pre-story; } && { [ -e /root/compose.web.yml.pre-story ] || cp -p compose.web.yml /root/compose.web.yml.pre-story; } && docker image inspect -f '{{.Id}}' cute-trading/web:pre-story && docker inspect -f '{{.Image}}' cute-trading-web-1 && ls -l /root/compose.yml.pre-story /root/compose.web.yml.pre-story && grep -c 'public-data' /root/compose.web.yml.pre-story; grep -c 'ct-math' /root/compose.web.yml.pre-story
```
→ expected: two identical `sha256:…` lines (write the ID in Results), two `ls -l` lines, then `4` and `14` (the compose file of the 7b-B deploy: the web's mount and the public router).
if not: `No such object` = the web container is missing: `docker ps -a --format '{{.Names}} {{.Status}}'` and report; (a re-run after step 13) the second ID is the new web, so a different line is expected: leave `pre-story` as it is; counts other than `4` and `14` = the VPS does not run the 7b-B compose file: report, do not go on.
**STOP**

10. Load the bundle (rewrites `compose.yml`, `compose.web.yml` and `.env.example` with the same content; moves `web:dev`; `web:pre-story`, `web:pre7b-B`, `web:pre7b` and `web:pre7a` stay), then the disk budget.
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-story.tar && rm /root/ct-story.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls cute-trading/web --format '{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/web:dev`; `dev` on a new ID and `pre-story` on step 9's (the older tags `pre7b-B`, `pre7b`, `pre7a` are listed too); then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: `Cannot open: No such file` = the bundle was already loaded (a re-run) or never shipped: go on to step 11 if `dev` and `pre-story` differ in `docker image ls cute-trading/web --format '{{.Tag}} {{.ID}}'`, otherwise redo steps 7–8; a truncated tar = bundle and ship again (steps 7–8), then this step; `OVER BUDGET` = `docker image prune -f`, then `LIMIT_GB=75 sh scripts/disk-budget.sh` again.
**STOP**

11. The loaded files are the expected ones: compose parses, and `compose.yml` and `compose.web.yml` are both identical to the copies saved in step 9 (this deploy changes neither).
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok; cmp -s compose.yml /root/compose.yml.pre-story && echo compose-yml-same; cmp -s compose.web.yml /root/compose.web.yml.pre-story && echo compose-web-yml-same; diff --strip-trailing-cr /root/compose.yml.pre-story compose.yml | head -40; diff --strip-trailing-cr /root/compose.web.yml.pre-story compose.web.yml | head -40
```
→ expected: `config-ok`; `compose-yml-same`; `compose-web-yml-same`; then nothing (no diff line).
if not: no `config-ok` = read the error line, report, do not go on; a `-same` word missing and no diff line = only the line endings differ: go on; any diff line (`<`, `>`) = a compose file changed, which this deploy must not do: put the saved files and the `web:dev` tag back with `cp -p /root/compose.yml.pre-story compose.yml && cp -p /root/compose.web.yml.pre-story compose.web.yml && docker tag cute-trading/web:pre-story cute-trading/web:dev` (no container has changed yet), report the diff lines, do not go on.
**STOP**

12. A quiet minute (advisory): the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`; run step 13 within 2 minutes.
if not: `f` = a busy minute: run this step again every few minutes; after 15 minutes without a `t`, go on to step 13 anyway and write "no quiet minute in 15 min" in Results (step 14 records the reconnects); an empty line = no health rows in 24 h: report before going on.

13. Record the start time, then recreate `web` only: it must be up, on the new image and not on `pre-story`, with the volume read-only, the public router's labels, and the new page inside.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/story_web_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/story_scanner_started && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && sleep 5 && docker inspect -f '{{.State.Status}} {{.RestartCount}}' cute-trading-web-1 && { N=$(docker image inspect -f '{{.Id}}' cute-trading/web:dev); O=$(docker image inspect -f '{{.Id}}' cute-trading/web:pre-story); R=$(docker inspect -f '{{.Image}}' cute-trading-web-1); [ "$R" = "$N" ] && [ "$R" != "$O" ] && echo web-on-new-image; docker inspect -f '{{range .Mounts}}{{.Destination}} rw={{.RW}}{{"\n"}}{{end}}{{index .Config.Labels "traefik.http.routers.ct-math.service"}} {{index .Config.Labels "traefik.http.services.ct-math.loadbalancer.server.port"}}' cute-trading-web-1; docker exec cute-trading-web-1 sh -c 'id -u; head -c 20 /srv/ct-public-root/math-data/public.json; echo; touch /srv/ct-public-root/math-data/x 2>&1; grep -c "Try it yourself" /usr/share/nginx/html/math-assets/*.js'; }
```
→ expected: the scanner's start time; `Container cute-trading-web-1  Started` (a `Recreated` line before it may or may not be printed: `web-on-new-image` below is the proof), and no line for another container or for a volume; `running 0`; `web-on-new-image`; `/srv/ct-public-root/math-data rw=false`; `ct-math 8081`; `101`; `{"v":1,"written_ms":`; `touch: /srv/ct-public-root/math-data/x: Read-only file system`; `1` (any number from 1 up).
if not: not `running`, or a restart count above 0 = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web`, then roll back (step 21); `web-on-new-image` printed with a last count of `0` = the image itself holds the old page (step 6 was skipped), and a recreate cannot fix it: roll back (step 21), then redo steps 2 and 5; no `web-on-new-image` (with or without `Container cute-trading-web-1  Running`) = compose left the old container in place: `docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps --force-recreate web && sleep 5 && docker image inspect -f '{{.Id}}' cute-trading/web:dev && docker inspect -f '{{.Image}} {{.State.Status}}{{range .Mounts}} {{.Destination}} rw={{.RW}}{{end}}' cute-trading-web-1 && docker exec cute-trading-web-1 sh -c 'id -u; head -c 20 /srv/ct-public-root/math-data/public.json; echo; grep -c "Try it yourself" /usr/share/nginx/html/math-assets/*.js'` (expected: the `web:dev` ID, the same ID with `running /srv/ct-public-root/math-data rw=false`, `101`, `{"v":1,"written_ms":`, `1`; still another ID = roll back, step 21, and report); `rw=true`, no `ct-math 8081` line or `Permission denied` on `head` = roll back (step 21) and report.
**STOP**

14. The scanner is unaffected: its start time is unchanged; the reconnects since the recreate are recorded (not a gate).
```bash
cd /home/isoca/projects/cute-trading && sleep 180; docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/story_scanner_started && echo scanner-not-restarted; echo "web $(docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $(cat /root/story_web_ms)")"
```
→ expected: (after 3 minutes) `scanner-not-restarted`; `web <n>` (any number: write it in Results).
if not: a `diff` line instead of `scanner-not-restarted` = the scanner restarted: `docker compose --profile core logs --tail 50 scanner` and report; reconnects above 0: do NOT roll back, write the count and the UTC time in Results.

## Checks

15. (On the PC, no cookie) The public paths answer without login: the page with the math CSP and the security headers, the js, css and wasm gzipped and cached, the snapshot as fresh JSON, and the js is the new page's.
```bash
D=https://radar.isoca.space; H=$(curl -s --path-as-is "$D/math"); JS=$(printf '%s' "$H" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.js' | head -1); CSS=$(printf '%s' "$H" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.css' | head -1); WASM=$(curl -s --path-as-is "$D$JS" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.wasm' | head -1); echo "$JS $CSS $WASM"; h() { echo "== $1"; curl -s --path-as-is -o /dev/null -D - -H 'Accept-Encoding: gzip' "$D$1" | tr -d '\r' | grep -iE "^(HTTP|$2)"; }; h /math 'content-type|cache-control|content-security-policy|strict-transport-security|x-content-type-options|x-frame-options'; for a in "$JS" "$CSS" "$WASM" /math-data/public.json; do h "$a" 'content-type|content-encoding|cache-control'; done; curl -s --path-as-is --compressed "$D/math-data/public.json" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);console.log("v",j.v,"age_min",Math.round((Date.now()-j.written_ms)/60000),"keys",Object.keys(j).join(","),"leak",/account_id|"host"/.test(s))})'; if [ -n "$JS" ]; then echo "story $(curl -s --path-as-is --compressed "$D$JS" | grep -c 'Try it yourself')"; else echo no-js-url; fi
```
→ expected: `/math-assets/math-<hash>.js /math-assets/math-<hash>.css /math-assets/arb_wasm_bg-<hash>.wasm` (step 6's names); `== /math`: `HTTP/2 200` (the protocol may read `HTTP/1.1`), `content-type: text/html`, `cache-control: no-cache`, `content-security-policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`, `strict-transport-security: max-age=31536000`, `x-content-type-options: nosniff`, `x-frame-options: DENY`; the js, the css and the wasm: `HTTP/2 200`, `content-type: application/javascript`, `text/css` and `application/wasm`, each with `content-encoding: gzip` and `cache-control: public, max-age=31536000, immutable`; `== /math-data/public.json`: `HTTP/2 200`, `content-type: application/json`, `content-encoding: gzip`, `cache-control: public, max-age=60`; `v 1 age_min <0–10> keys v,written_ms,ladder,today leak false`; then `story 1` (any number from 1 up).
if not: `story 0`, or names other than step 6's = the old page is still served: on the VPS run step 13's `--force-recreate` command, then this step again; `no-js-url` or an empty first line = `/math` did not answer with the page: read its status under `== /math`; `/math` `302` = the public router is not picked up: on the VPS `docker inspect -f '{{json .Config.Labels}}' cute-trading-web-1 | grep -o 'ct-math[^,]*' | head -5`, report; `502` = the web container is not answering on 8081: on the VPS `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web`, roll back (step 21) and report; `404` on `public.json` = the mount or the file (step 13); the wasm without `application/wasm`, or a CSP other than the line above = report; only a missing `content-encoding` = write it in Results and go on; `leak true` or other keys = report (the api's whitelist, not this deploy: rolling the web back does not change the file); `429` = too many requests from this IP in a few seconds: wait 10 s and run again.
**STOP**

16. (On the PC, no cookie) Everything else still goes to the GitHub login.
```bash
D=https://radar.isoca.space; for p in / /index.html /assets/x.js /sweet /api/today /api/paper/ladder /math/ /math.html /math-assets /math-data/ /math-data/public.json.tmp /math-data/x.json; do printf '%s ' "$p"; curl -sS --path-as-is -o /dev/null -w '%{http_code} %{redirect_url}\n' "$D$p" | cut -d'?' -f1; done
```
→ expected: twelve lines, each `<path> 302 https://github.com/login/oauth/authorize` (the two `/api/` paths may answer `401` with no URL); never `200`.
if not: any `200` = a path is public that must not be: on the VPS roll back (step 21) and report; `/` or `/sweet` still `200` after the rollback = forward-auth is off: on the VPS `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web stop web` and report.
**STOP**

17. (On the PC, no cookie) Traversal probes: no crafted path under the public prefix reaches a dashboard file or the snapshot's tmp file.
```bash
D=https://radar.isoca.space; T=$(mktemp); for p in '/math-assets/..%2findex.html' '/math-assets/%2e%2e%2findex.html' '/math-assets/..%2fassets%2fx.js' '/math-assets/..%2fmath-data%2fpublic.json.tmp' '/math%2Dassets/..%2findex.html' '/math-assets/..%252findex.html' '/math-assets/../index.html' '/math-data/../public.json'; do C=$(curl -sS --path-as-is -o "$T" -w '%{http_code}' "$D$p"); printf '%s %s %s\n' "$p" "$C" "$(head -c 60 "$T" | tr -d '\r' | tr '\n' ' ')"; done; rm -f "$T"
```
→ expected: eight lines `<path> <status> <first 60 bytes of the body>`: the six encoded probes `404` (body `<html> <head><title>404 Not Found</title></head>…` or `404 page not found`; `400` is fine too); `/math-assets/../index.html` `302` and `/math-data/../public.json` `302` (Traefik cleans the path, so both are gated; body `<a href="https://github.com/login/oauth/authorize?…`; `400` or `404` are fine too); a `302` on an encoded probe is gated too; never `200`.
if not: any `200` = a crafted path reaches a file without login: on the VPS roll back NOW (step 21), then report the line and its body (the one exception: `/math-data/../public.json` `200` with the body `{"v":1,"written_ms":…` is the snapshot itself, the public file: no rollback, write it in Results and report); `429` = too many requests from this IP in a few seconds: wait 10 s and run again.
**STOP**

18. (Chrome, a private window: not logged in; 1440 px wide; DevTools open on Network and Console, cache disabled) open `https://radar.isoca.space/math`, read it top to bottom and press every switch; then open `https://radar.isoca.space/math#play` in a new tab.
→ expected: the header `isoca-radar · math`, then the hook (four sentences, starting `Sometimes the prices of every outcome`); seven chapters numbered 1 to 7, each a question with a short lead and one drawing: `What is being traded?`, `What happens with several outcomes?`, `When is that free money?`, `How much of it is there?`, `How long does it last?`, `What if you get only part of it?`, `So does it pay?`; chapters 1 to 6 each have one two-option switch, the first option selected on load; the second option changes the drawing, and in chapters 1, 2, 3, 5 and 6 the sentence under it; chapter 4 keeps its sentence while the Q* line moves (chapter 1: `a healthy book` / `a crossed book`, from `One book, two views.` to the crossed-book sentence and alert; 2: `every outcome is listed` / `one outcome is not listed`; 3: `prices add to 1.00` / `prices add to 0.97`: a price stack, the three prices laid end to end against a line labelled `one dollar`, and with the second option a narrow dashed-outline gap appears before the line (legend `the gap`) and the line under the drawing reads `net Π … on the table` with the engine's figure; no decision tree here; 4: `a deep book` / `a thin book`; 5: `it outlasts the order` / `it closes first`; 6: `every leg fills` / `one leg misses`, the verdict goes from `filled` to `caught`); chapter 3 ends on today's count: the strip `today (UTC) on the live scanner` with the NO and YES counts and `as of HH:MM UTC`; chapter 7 has no switch and shows the ladder headed `on the live scanner, as of HH:MM UTC` with its folds `which markets` and `the numbers`, then `Every figure here is on paper. No order was ever sent.`; the strip and the ladder each appear once on the page; then `Try it yourself`, a closed fold `the words used on this page` (opening it shows the word list: twelve entries, from `YES / NO share` to `on paper`), and sections a to f in order (`a · the book, twice` … `f · one leg misses`), each naming its chapter (`chapter 1` … `chapter 6`) and each drawing changing when a control is moved; the decision tree (a list of about fourteen tests) is only in section c; the footer `The live dashboard is private. The math is not.`; Network: every request `200`, one `public.json` per load (a second one after coming back to the tab more than five minutes later is normal), no `/api/` request; Console: no error, no CSP violation; the `#play` tab lands on `Try it yourself`.
if not: `live numbers unavailable` in chapters 3 and 7 = the snapshot did not load or did not parse: step 15's `v 1 …` line, report (no rollback needed: the rest of the page works); `The math engine did not load: the drawings need WebAssembly.` = hard-reload once (Ctrl+Shift+R); still there = report the Console error and roll back (step 21); the old page (it opens on sections a to f, no numbered chapters) = hard-reload once (Ctrl+Shift+R), then step 15's `story` line; a switch that changes nothing, a label stretched, cut or drawn over another = screenshot and report (a web fix on a branch; roll back only if the page is unreadable).

19. (Chrome, the same private window, DevTools device toolbar at 360 × 740) `/math` with every fold open (chapter 1's `every level`, chapter 7's `which markets` and `the numbers`, the playground's `the words used on this page`, and the folds in sections a to f); in the console:
```js
document.documentElement.scrollWidth <= window.innerWidth
```
→ expected: `true`; the chapters stay one column; a switch's two options may sit on two rows, and no label is cut; the switches work by touch; in chapter 3 the labels under the price stack sit under their segments and none is cut; in chapter 7's `the numbers` each rung name stays on one line (never broken mid-word).
if not: `false` = horizontal scroll: screenshot and report (a layout fix on a branch; no rollback needed); a cut label or a rung name broken over two lines = screenshot and report (no rollback needed).

20. (Chrome, a normal window, logged in as the owner) the dashboard itself: `https://radar.isoca.space/`, then `https://radar.isoca.space/paper`.
→ expected: both pages load with live numbers, as before the deploy.
if not: a page that does not load = on the VPS `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web`, then roll back (step 21) and report; a login page = log in once and write it in Results.
**STOP**

## Rollback

21. (On the VPS) Back to the web image that ran before this deploy; the compose files did not change, so there is nothing else to restore. At a quiet minute (step 12) when there is one.
```bash
cd /home/isoca/projects/cute-trading && docker tag cute-trading/web:pre-story cute-trading/web:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps --force-recreate web && sleep 5 && docker image inspect -f '{{.Id}}' cute-trading/web:pre-story && docker inspect -f '{{.Image}} {{.State.Status}}' cute-trading-web-1 && docker exec cute-trading-web-1 sh -c 'grep -c "Try it yourself" /usr/share/nginx/html/math-assets/*.js'; D=https://radar.isoca.space; printf '/math '; curl -sS --path-as-is -o /dev/null -w '%{http_code}\n' "$D/math"; JS=$(curl -s --path-as-is "$D/math" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.js' | head -1); if [ -n "$JS" ]; then echo "story $(curl -s --path-as-is --compressed "$D$JS" | grep -c 'Try it yourself')"; else echo no-js-url; fi
```
→ expected: `Container cute-trading-web-1  Started` (a `Recreated` line may come first), and no line for another container; the `pre-story` ID, the same ID with `running`; `0` (the old page is in the container); `/math 200`; `story 0`; then the owner reloads the dashboard in Chrome: it loads.
if not: `No such image` = the tag `pre-story` is missing (step 9 was skipped): `docker image ls cute-trading/web --format '{{.Tag}} {{.ID}}'` and report, change nothing else; another ID on the container = run this step again; the `pre-story` ID on the container with a first count above `0` = the tag itself holds the new page (step 9 first ran after step 13): report, change nothing else; `/math 502` or `/math 404` right after the recreate = Traefik has not picked the container up yet: run the PC check below after 10 s; `/math 000` or `no-js-url` = the VPS cannot reach its own name: run the same check from the PC, `D=https://radar.isoca.space; JS=$(curl -s --path-as-is "$D/math" | grep -o '/math-assets/[A-Za-z0-9_.-]*\.js' | head -1); curl -sS --path-as-is -o /dev/null -w '%{http_code}\n' "$D/math"; [ -n "$JS" ] && curl -s --path-as-is --compressed "$D$JS" | grep -c 'Try it yourself'` (expected `200`, then `0`); not `running` = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web` and report; `story 1` while the container's count is `0` = the answer was read during the restart: run the PC check above after 10 s.
Note: only the `web:dev` tag moves back; `oauth2-proxy`, the api, the volume and both compose files are as they were before step 9, and `/root/compose.yml.pre-story`, `/root/compose.web.yml.pre-story` stay as a record.

## Results

- PR CI and the merged commit (steps 1–2):
- Engine tests, and the gzip total of the math assets (step 4):
- Bundle size (step 7):
- Rollback tag ID = the running web's ID before the deploy; compose copies saved (step 9):
- Compose `cmp` (step 11): compose.yml same / compose.web.yml same:
- Quiet minute (step 12), UTC time, or "no quiet minute in 15 min":
- New web image ID, mount read-only, the new page in the container (step 13):
- Scanner not restarted; reconnects since the web recreate (step 14):
- Public paths (step 15): status of /math, js, css, wasm, public.json; snapshot age; the `story` line:
- Gated paths (step 16): the twelve statuses:
- Traversal probes (step 17): the eight statuses:
- Chrome 1440 (step 18): the seven chapters and their switches, today's count in chapter 3, the ladder in chapter 7, `Try it yourself` and sections a–f, console, cold `#play`:
- Chrome 360 (step 19): scroll check, switch labels, the price stack's labels in chapter 3, rung names in `the numbers`:
- Dashboard `/` and `/paper` (step 20):
- Rollback run (step 21): yes / no, and why:
