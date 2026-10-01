# Phase 7a step B on the VPS: the latency answer on the dashboard (web only)

Note: one image change, `cute-trading/web:dev`: the explain panel with the latency ladder, the Sweet spots grid, and the Paper epoch line, `earlier epochs` and `reset paper money` folds. The api, scanner, analyze, compact and postgres are not touched; step A is live since 2026-09-29 19:58 UTC.
Note: recreate `web` only, at a quiet minute: a `web` recreate once coincided with the scanner's websockets dropping (2026-09-28 02:46 UTC, mechanism unverified); later web recreates did not.
Note: this deploy's web rollback tag is `cute-trading/web:pre7b` (step 7); `cute-trading/web:pre7a` stays untouched for step A's rollback.
Note: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker.
Note: PC steps in Git Bash from the main checkout on the merged commit; VPS steps as root over SSH (`ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`); browser steps in Chrome, logged in as the owner.

## On the PC

1. The repo on the commit being deployed; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s'
```
→ expected: no status lines, `main`, then `<sha> <subject>` of the merged 7a-B commit.
if not: commit or stash the listed changes, then run this step again.

2. Web tests, types and the production build pass (each stage stops the chain on a failure).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm ci --no-audit --no-fund >/dev/null && ( set -o pipefail; npm test 2>&1 | grep -E '^ +(Test Files|Tests) ' ) && echo tests-ok && npx tsc --noEmit && echo tsc-ok && ( set -o pipefail; npm run build 2>&1 | tail -1 ) && echo build-ok
```
→ expected: `Test Files  <n> passed (<n>)`, `Tests  <n> passed (<n>)`, `tests-ok`, `tsc-ok`, `✓ built in <t>`, then `build-ok`.
if not: any `failed`, a missing `tests-ok`/`tsc-ok`/`build-ok`, or a tsc error: fix on a branch; do not build the image.
**STOP**

3. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `docker builder prune -f --keep-storage 5GB`, then run this step again.

4. Build the web image only.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web 2>&1 | tail -3
```
→ expected: the build's last lines without `ERROR`.
if not: read the first `ERROR` line (a type error means step 2 was skipped), fix, run again.

5. The image carries the new bundle.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint sh cute-trading/web:dev -c 'grep -l "At what latency would this pay" /usr/share/nginx/html/assets/*.js | wc -l; grep -l "reset-paper" /usr/share/nginx/html/assets/*.js | wc -l'
```
→ expected: `1`, then `1`.
if not: `0` = the image was built from an old tree: redo steps 1 and 4.
**STOP**

6. Save and ship the image.
```bash
docker save cute-trading/web:dev | gzip -1 > "$HOME/ct-7b-web.tar.gz" && ls -l "$HOME/ct-7b-web.tar.gz" && scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7b-web.tar.gz" root@31.97.23.123:/root/ && rm "$HOME/ct-7b-web.tar.gz"
```
→ expected: one `ls -l` line (roughly 15–40 MB), the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

7. Rollback tag `web:pre7b` on the image the web container runs now (by ID: on 2026-09-30 the VPS had no `web:dev` tag), only if `web:pre7b` does not exist yet; `web:pre7a` is only compared, never retagged.
```bash
cd /home/isoca/projects/cute-trading && { docker image inspect cute-trading/web:pre7b >/dev/null 2>&1 || docker tag "$(docker inspect -f '{{.Image}}' cute-trading-web-1)" cute-trading/web:pre7b; } && docker image inspect -f '{{.Id}}' cute-trading/web:pre7b cute-trading/web:pre7a; docker inspect -f '{{.Image}}' cute-trading-web-1
```
→ expected: three identical `sha256:…` lines (pre7b, pre7a, the running web).
if not (first run, before step 8 only): the running ID differs from the first line = `docker tag <the running sha256 ID> cute-trading/web:pre7b` and run the last two commands again until the first and third lines match; only `pre7a` differs or is missing = write it in Results and go on (never retag `pre7a`).
if not (a re-run after step 10): the running web is the NEW image, so a different third line is expected: leave `pre7b` as it is and check that the first line equals the second (`pre7a`); if `pre7b` equals the NEW running web = `docker tag <the pre7a sha256 ID> cute-trading/web:pre7b` and report.
**STOP**

8. Load the new image (moves `web:dev`; `web:pre7b` and `web:pre7a` stay).
```bash
cd /home/isoca/projects/cute-trading && gunzip -c /root/ct-7b-web.tar.gz | docker load && rm /root/ct-7b-web.tar.gz && docker image ls 'cute-trading/web' --format '{{.Repository}}:{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/web:dev`; `web:dev` on a new ID, `web:pre7b` and `web:pre7a` on the old one; then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: `OVER BUDGET` = `docker image prune -f`, then `LIMIT_GB=75 sh scripts/disk-budget.sh` again.

9. A quiet minute: the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`.
if not: `f` = a busy minute: wait 15 minutes and run this step again; never go on during a burst; an empty line = no health rows: **STOP**, report.
Note: run step 10 within 2 minutes of the `t`; later, run this step again first.
**STOP**

10. Scanner websockets baseline, then recreate `web` only.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7b_web_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/7b_scanner_started && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs FROM health ORDER BY minute_ms DESC LIMIT 3" && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && docker compose -f compose.yml -f compose.web.yml --profile web ps web --format '{{.Service}} {{.Status}}'
```
→ expected: the scanner's start time, three `HH:MI|<shards>|0|0|<msgs>` rows, `Container cute-trading-web-1 …` lines only (no api, no oauth2-proxy, no network), then `web Up …`.
if not: `web` not `Up` = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web`, then the rollback (step 19).

11. After the recreate: no websocket disconnect.
```bash
cd /home/isoca/projects/cute-trading && sleep 180; T0=$(cat /root/7b_web_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7b_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"
```
→ expected: `scanner-not-restarted`, then `0`.
if not: reconnects > 0: do NOT roll back; write the count and the UTC time in Results.

## Checks

12. Unauthenticated requests never reach the dashboard or the api.
```bash
for p in / /sweet /api/paper/ladder; do curl -sS -o /dev/null -w "$p %{http_code}\n" "https://radar.isoca.space$p"; done
```
→ expected: `/ 302`, `/sweet 302`, `/api/paper/ladder 302` (or `401`).
if not: `/` or `/sweet` `200` = forward-auth is off (a web rollback does not fix it): on the VPS stop web with `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web stop web` and report; `/api/paper/ladder` `200` = on the VPS also stop the api with `cd /home/isoca/projects/cute-trading && docker compose -f compose.yml -f compose.web.yml --profile web stop api` and report.
**STOP**

13. (Chrome, window 1440 px wide) open `https://radar.isoca.space/`, DevTools Network filtered on `ladder`, click the `EXPLAIN ▸` tab on the right edge.
→ expected: a drawer opens left of the tab (the tab stays visible, 1440 < 1500 so no dock); exactly one `/api/paper/ladder` request; `At what latency would this pay?` is open with the big VT323 line (`≈ N ms`, `none`, `pays at every rung`, `too few trades yet` or `a rung has no account yet`), six columns `0 25 52 100 200 300` plus the extras slot, counts `n / 30` while thin (`no account` under a missing rung), `as of HH:MM UTC`, the pink us band from just right of the 200 rung (measured + 50, ≈205 ms at measured ≈155) to just right of the 300 rung (measured + 150, ≈305 ms at ≈155), never into the extras slot, labelled `us (measured ≈N + matching)`, the `sp-idle-nodelay (… ms, secondsDelay ignored)` and `sp-burst (… ms idle, more in bursts)` lines, and the folds `which markets` (a grid, or `recomputing after the ladder` / `no markets data`) and `the numbers`.
if not: a label drawn inside the SVG or stretched = report (spec §7.2); more than one request = the fetch rule broke: report.

14. (Chrome, same tab) keep the panel open 2 minutes, then press Esc and wait 2 more minutes.
→ expected: one more `/api/paper/ladder` per minute while open; Esc closes it with the focus on the `EXPLAIN ▸` tab; no request while closed.
if not: requests while closed or more than one per minute = report (no rollback needed).

15. (Chrome) the panel on each page: `/` (How can this make money?), `/episodes` (click a row, then What is an episode?), an episode detail (Would we have caught it?), `/paper` (open the `awaiting resolution` fold, click an awaiting row, then Why is money locked?), `/sweet`, `/papers`, `/health` (the hero only).
→ expected: each answer is one drawing plus at most two sentences; `as selected: …` after a row click, gone after `back to the page` and after leaving the page; no word `earn`, `earned`, `made`, `income` or `profitable` anywhere in the panel.
if not: a forbidden word = screenshot and report (a copy fix on a branch; no rollback needed).

16. (Chrome) `/sweet`, then `/paper`.
→ expected: `/sweet` shows the grid with six NO columns and two YES columns, a tap on a cell writes one detail line under the grid, the old per-scenario chart and its chips are gone, every number is still in `all numbers`; `/paper` shows `since new policies 2026-09-29 19:58 UTC · epoch 1` (or `since reset … · epoch N`) above ACCOUNTS, `earlier epochs` lists epoch 0 with the P0 finals once opened, and `reset paper money` shows the current epoch and a RESET button that stays disabled until `reset` is typed.
if not: screenshot and report.
Note: do not press RESET for this check (one per UTC day).

17. (Chrome, window 1600 px wide) `/` with the panel open, then closed.
→ expected: open, the panel docks beside the app and the pair is centred; the `EXPLAIN ▸` tab sits flush on the panel's left border like a folder tab, never over the panel's edge or its scrollbar, and one click on it closes the panel; closed, the tab is back on the window's right edge.
if not: the tab covers the panel's edge or does not respond to a click = screenshot and report (D-06; a layout fix on a branch; no rollback needed).

18. (Chrome, window 1600 px wide, then DevTools device toolbar 360 × 740, then a window ~2560 px wide) `/`, `/sweet`, `/paper`, each with the panel closed and open; in the console on each:
```js
document.documentElement.scrollWidth <= window.innerWidth
```
→ expected: at 1600 px docked as in step 17; at 360 px it is a drawer; `true` on every page and width, open and closed; the Sweet grid fits without scrolling; at ~2560 px, closed, the tab sits on the window's right edge, well right of the centred app: write its position in Results.
if not: `false` = horizontal scroll: screenshot and report (a layout fix on a branch; no rollback needed).
**STOP**

## Rollback

19. Back to the web that ran before this deploy (`web:pre7b`; the api stays on 7a; the old web never calls the new routes). At a quiet minute (step 9).
```bash
cd /home/isoca/projects/cute-trading && docker tag cute-trading/web:pre7b cute-trading/web:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && docker image inspect -f '{{.Id}}' cute-trading/web:pre7b && docker inspect -f '{{.Image}}' cute-trading-web-1
```
→ expected: the same `sha256:…` twice.
if not: `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web` and report.
Note: `web:pre7a` is not used here; it stays for step A's full rollback.

## Results

- Rollback tags (step 7): pre7b = pre7a = running, or what differed:
- Quiet minute (step 9), UTC time:
- Reconnects after the web recreate (step 11):
- Hero at deploy (step 13): big line, markets status:
- Ladder requests while open / closed (step 14):
- Docked tab at 1600 px (step 17):
- 1600 / 360 px scroll check and the ~2560 px closed tab position (step 18):
