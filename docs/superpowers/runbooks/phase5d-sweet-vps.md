# Phase 5d on the VPS: sweet spots (migrate 0007, SWEET card, no new service)

Mechanics follow `docs/superpowers/runbooks/phase5a-cards-vps.md` (build, bundle, ship, load) —
read that runbook's header once for the Rules that still apply verbatim: never print `.env`, never
print a container's environment, `docker compose config` only as
`docker compose --env-file .env.example … config -q`, never restart Docker, and the disk budget
(30 GB PC / 50 GB VPS, `LIMIT_GB=50 sh scripts/disk-budget.sh` on the VPS). Only what differs from
5a is spelled out below.

**What ships** (one Rust image, one web image, same tags `cute-trading/rust:dev` and
`cute-trading/web:dev`):
- The Rust image: scanner now records `events.tags` from Gamma (go/no-go: GO, spec §3); analyze
  gains the `SWEET` card family; api is unchanged.
- The web image: the **Sweet spots** box on the Paper page, directly below Edge capture.
- Migration 0007: `events.tags TEXT[] NULL` (no rewrite, no long lock), plus `GRANT SELECT` to
  `analyze_rw` on `events`, `episode_legs`, `paper_intents`, `paper_execs`, `paper_ledger`,
  `paper_markouts`. No new indexes (spec §2) — every paper table's primary key already starts with
  `(run_id, account_id, episode_id)`.
- No new compose service, no new `.env` key, no new published port. `analyze_rw`'s write set is
  unchanged: INSERT/UPDATE/DELETE on `card_cells`, `card_results`, `card_meta` only.

**The order:**

| Where | Step |
| --- | --- |
| PC | §1 benchmark |
| PC | §2 build (5a §3, unchanged) |
| PC | §3 bundle and ship (5a §4 and §7, unchanged) |
| VPS | §4 pre-flight |
| VPS | §5 stop the scanner and analyze, migrate 0007, start again |
| VPS | §6 post-deploy checks |
| VPS | §7 exit |

---

## 1. Benchmark (on the PC, Git Bash)

Debug build (spec §9). Write both totals in Results:
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p cards --test bench_sweet -- --ignored --nocapture
cargo test -p cards --test bench_tick -- --ignored --nocapture
```
`bench_sweet` (SWEET alone < 75 ms, held < 16 MB): a SWEET tick over a 30-day history prints
`sweet tick: N ms` and `sweet held: N KB`, then `test result: ok. 1 passed`.

`bench_tick` (30- and 90-day ticks incl. SWEET < 300 ms; the 90-day tick that renders `all` < 400
ms): prints `tick total at 30 days: tick1 N ms, tick2 N ms …` and the same for `90 days`, then
`test result: ok. 1 passed`.

These are debug-build regression guards only (Phase 5d ruling 2026-09-27): the real check is the
VPS `docker stats` step in §6.

## 2. Build (on the PC, Git Bash)

Unchanged from 5a runbook §3: same `.env` keys (`ANALYZE_RW_DB_PASSWORD`, `SCANNER_HOST`), no new
key for 5d. Run 5a §3 as written, including the disk-budget check, the `git status` check and the
`analyze` binary smoke line. Write `GIT_SHA` in Results.

## 3. Bundle and ship (on the PC / VPS)

Unchanged from 5a runbook §4 (bundle, prune, ship) and §7 (load on the VPS, `config -q` check,
disk budget). Run them as written. `compose.yml` and `compose.web.yml` do not gain new keys in 5d,
so §6's `.env`-key step from 5a does not apply here — there is nothing new to add.

---

## 4. Pre-flight (on the VPS)

Record every 5a card's `(card, win, premise_key)` before touching anything, so the post-deploy
check can prove none of them restarted (a new `premise_key` would mean a family's key moved, since
Rust reruns backfill under a changed key):
```bash
cd /home/isoca/projects/cute-trading
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT card, win, premise_key FROM card_results ORDER BY 1, 2" > ~/pre5d_keys.txt; wc -l ~/pre5d_keys.txt
```
Back up the database as in 5a runbook §5 (the pre-flight record of migrations and running run,
plus tagging the current image `cute-trading/rust:<old-sha>` so `docker load` does not leave it
`<none>`, plus `cp .env .env.bak-5d`). Also take the safety dump as in 5a §9, once the scanner is
stopped in §5 below.

**Clock and kernel, before the deploy.** Chrony was installed on 2026-09-27 after a host freeze
left the clock 2 s behind. The same `apt` run left a kernel upgrade pending. Reboot first so
capture is interrupted once, then confirm the clock and the containers:
```bash
sudo reboot
# after it is back:
chronyc tracking | grep -E 'System time|Last offset'     # System time: a few ms or less
docker compose ps                                        # postgres/scanner/analyze/api/web up
```
Do not start §5 while `System time` is over about 250 ms. The scanner measures its host latency
band in its first 10 minutes, and a skewed clock would put those minutes out of the band.

## 5. Stop the scanner and analyze, migrate 0007, start again (on the VPS)

`scanner`, `analyze` and `migrate` live in `compose.yml` under the `core` profile, so every command
touching them needs `--profile core` (5a runbook §9, §10, §12). `api` and `web` live in
`compose.web.yml` under the `web` profile, so restarting them needs both `-f` files and
`--profile web` (5a runbook §12) — they do not exist in a plain `docker compose` invocation against
`compose.yml` alone:
```bash
cd /home/isoca/projects/cute-trading
docker compose --profile core stop --timeout 60 scanner analyze
docker compose --profile core run --rm migrate; echo "migrate exit=$?"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT max(version) FROM _sqlx_migrations WHERE success"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT has_table_privilege('analyze_rw', 'paper_ledger', 'SELECT'), has_table_privilege('analyze_rw', 'paper_ledger', 'INSERT')"
docker compose --profile core up -d --no-build --no-deps scanner analyze
docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web
```
Expect:
- `migrate exit=0`;
- `7` (0007 applied and successful, on top of 0001–0006 already on the VPS from Phase 5a);
- `t|f`: `analyze_rw` can now read `paper_ledger` but still cannot write it (0007 grants SELECT
  only; INSERT/UPDATE/DELETE stay on `card_cells`, `card_results`, `card_meta`).

If `migrate` fails, stop and read its log
(`docker compose --profile core logs --tail 50 migrate`) before deciding whether to roll back —
0007 is additive (a nullable column plus grants), so a clean failure leaves nothing half-applied.

Then tail all four restarted services' logs (5a runbook §12 style) before moving to §6:
```bash
docker compose --profile core logs --tail 30 scanner
docker compose --profile core logs --tail 20 analyze
docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 20 api web
```
Expect the scanner and analyze lines 5a runbook §12 describes (no new-image startup error), and no
`panicked`/`error` lines from `api` or `web`.

## 6. Post-deploy checks (on the VPS, then in the browser)

**Clock offset is measured** (new in this deploy: the scanner samples CLOB `/time` every 7.3 s):
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), clock_offset_ms, lag_p50_ms FROM health ORDER BY minute_ms DESC LIMIT 5"
```
Expect `clock_offset_ms` filled (not empty) and within ±250 ms on every row, and `lag_p50_ms`
positive (about 75). The Health page shows the same offset, and the Radar keeps all its hearts.

**(GO) Tags arrive**, within about 10 minutes of the first universe refresh:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FILTER (WHERE tags IS NOT NULL), count(*) FROM events WHERE gone_ms IS NULL"
```
Expect most listed events tagged (the go/no-go sample was 100/100, spec §3 Results).

**SWEET results**, after the first tick and catch-up:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT win, status FROM card_results WHERE card = 'SWEET' ORDER BY 1"
```
Expect `24h`, `7d`, `30d` and `all`, each `under_review` while catch-up runs, then `live` or
`warming`.

**The 30-cluster minimum, re-checked against real volumes (spec §12 D5):** the review suggested a
50-cluster minimum instead of 30; the plan deferred that until real volumes were known and put this
re-check in the deploy step. Once the `30d` window has caught up (same wait as the checks below),
compare how many rows would clear each threshold:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FILTER (WHERE (r->>'clusters')::int >= 30) AS ge30, count(*) FILTER (WHERE (r->>'clusters')::int >= 50) AS ge50, count(*) AS total FROM card_results, jsonb_array_elements(body->'series'->'sweet'->'rows') r WHERE card = 'SWEET' AND win = '30d'"
```
Record `ge30`, `ge50`, `total` and today's date (UTC) in Results. This is a data point, not a
pass/fail gate: a `ge50` far below `ge30` means 50 would have thinned out most rows this early and
30 is still the right call for now; if they're close, raise it with the owner before the next
deploy rather than changing it here.

**No 5a card restarted:**
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT card, win, premise_key FROM card_results WHERE card <> 'SWEET' ORDER BY 1, 2" | diff - ~/pre5d_keys.txt && echo keys-unchanged
```
Expect `keys-unchanged`. A diff in `premise_key` only would mean a 5a family's key moved: **stop
and roll back** (§8).

**Memory (docker stats) and CPU (the tick's own time, spec §9):** wait until an `analyze` tick line
shows `0 catch-up hours`, then:
```bash
docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' | grep '^cute-trading-analyze'
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT value FROM card_meta WHERE key = 'tick'"
```
Record the memory number and the second query's `tick_ms` in Results. Pass: memory below 96 MB
(75% of the 128 MB cap). Run the memory read twice, a minute apart — `analyze` idles between its
15-minute ticks.

`docker stats`' `CPUPerc` samples one instant of a container that is idle 14 of every 15 minutes:
it reads near 0% whether the tick that just ran took 50 ms or 50 s, so it is dropped as a CPU check
(Phase 5d ruling 2026-09-27). The real CPU check is the release-build tick time the second query
just read, against the 0.25-CPU cap's per-tick budget (900 s × 0.25 = 225 s): pass is `tick_ms`
well under that — a healthy tick prints **`tick_ms` under 5000** (5 s). A tick anywhere near 225 s
means the cap will throttle it; stop and investigate rather than deploying past that line.

**In the browser (signed in):**
- the Paper page shows **SWEET SPOTS** directly below Edge capture, `30d` selected by default,
  with the §7.6 caveat, two headline sentences and the §7.2 footnote;
- switching the window switch's side to YES shows `NOT TESTED` or `DEAD ON PAPER` on the
  long-dated rows;
- at 360 px (DevTools → device toolbar → 360 × 740, run
  `document.documentElement.scrollWidth` in the console) the Paper page does not scroll
  sideways — it must print `360`; the sweet table scrolls inside its own box.

## 7. Exit (spec §1)

On the VPS: the Sweet spots box shows 7d / 30d / all, every row carries n, C (clusters), a label
and the card hash, and `analyze` stays inside its memory cap (§6's `docker stats`) and its CPU cap
(§6's `tick_ms`, well under 225 s). Record the date (UTC) in Results.

## 8. Rollback

Use 5a runbook's Rollback section for the mechanics (keep the previous `rust:dev` and `web:dev`
image tags recorded in pre-flight; `docker compose --profile core stop analyze scanner`, retag,
`docker compose --profile core up -d --no-build --no-deps scanner`). What's different for 5d:
- Migration 0007 stays applied even on rollback: its column (`events.tags`) is nullable and
  unused by the old code, and its grants are read-only (`GRANT SELECT`, no write access). Nothing
  needs undoing.
- SWEET rows left in `card_results` are ignored by the old web build: it filters cards by paper
  id and never asks for `SWEET`.
- **Only analyze** (SWEET failing, the other cards keep their last results):
  `docker compose --profile core stop analyze`.
- **The scanner, back to the previous image** (tags stop recording, everything else untouched):
  follow 5a Rollback's scanner section, substituting the 5d image tags.

## 9. Results

(Fill in:
- `GIT_SHA` (§2);
- the §1 benchmark totals: `bench_sweet`'s tick ms and held KB, `bench_tick`'s totals at 30 and 90
  days;
- chrony `System time` after the reboot (§4) and the first five `clock_offset_ms` values (§6);
- the tag counts (§6, GO check);
- the SWEET statuses per window (§6);
- `keys-unchanged` (§6);
- `analyze`'s `docker stats` memory after catch-up (§6), taken twice;
- `analyze`'s release-build `tick_ms` from `card_meta` after catch-up (§6; pass under 5000);
- the 30-cluster minimum re-check against the first 30 days: `ge30`, `ge50`, `total` and its date (§6, D5);
- the browser checks: SWEET SPOTS box present, YES-side `NOT TESTED`/`DEAD ON PAPER`, 360 px
  `scrollWidth === 360`;
- the exit date (UTC).)
