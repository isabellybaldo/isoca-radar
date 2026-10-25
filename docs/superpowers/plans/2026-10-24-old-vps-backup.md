# Old VPS backup and frozen export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Before the old VPS expires on 2026-11-13, copy all of isoca-radar's data to the owner's PC
(verified), and commit the four frozen JSON files the portfolio site needs.

**Architecture:** One owner-run runbook (old VPS as root, PC in Git Bash + WSL Ubuntu for rsync) that
streams a `pg_dump` and rsyncs the archive and raw capture volumes to the PC, checks them with sha256
manifests, a test restore and a DuckDB query, and exports `public.json` plus `/api/cards` for three
windows. Claude then commits those JSON files under `web/frozen/` with a test that pins their shape.

**Tech Stack:** bash, ssh, rsync (WSL Ubuntu, already installed on the PC), Docker Desktop
(postgres:17 for the test restore), DuckDB (Python package in a throwaway venv), Vitest.

**Spec:** `docs/superpowers/specs/2026-10-24-portfolio-port-design.md` (§5.1, §8 steps 1–2, §9).
This is plan 1 of 3; plan 2 (the frozen site) reads `web/frozen/`, plan 3 is isoca-infra.

## Global Constraints

- Old VPS: `ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`, project folder
  `/home/isoca/projects/cute-trading`. The owner runs every VPS step; Claude never runs them.
- Volumes: `cute-trading_pgdata` (dumped, not copied), `cute-trading_archive` (Parquet, one folder
  per UTC day, each with `manifest.json`), `cute-trading_data` (raw capture), `cute-trading_public-data`
  (`public.json`). Database `cute_trading`, owner role `ct_owner`.
- VPS rules: never print `.env` or a container's environment; never restart Docker; install nothing on
  the VPS; never start scanner, analyze or compact.
- Order: Postgres dump → Parquet archive → raw capture (raw is the only part that may be dropped if
  time runs out) → JSON export.
- Backup destination: owner's choice, default `/mnt/e/isoca-radar-backup` (E: has ~110 GB free); never
  inside a git checkout.
- Runbook format: numbered steps, one command block → `→ expected:` → `if not:` line, **STOP** after any
  step whose failure must halt; no prose walls; self-contained (no "as in another runbook").
- The owner does every push. Claude commits locally only. No `Co-Authored-By` trailer.
- Frozen JSON is committed to a repo that will be public: it must not contain a host name, IP,
  account id or anything from `.env`.

## Review Focus

1. **A transfer cut off halfway** (laptop sleeps, Wi-Fi drops): re-running the same step resumes and
   ends verified, never silently leaves a partial file counted as done → rsync `--partial
   --append-verify` and the sha256 check run after every rsync step (runbook steps 7–10).
2. **The PC disk fills up mid-copy**: the measured total is checked against the destination's free
   space before the first byte moves (runbook step 3).
3. **The VPS disk fills up**: nothing is staged on the VPS; the dump streams straight to the PC
   (runbook step 5) and manifests are written to `/root/ct-backup` (a few MB, checked in step 2).
4. **The frozen cards are in a non-final state** (`warming`, `thin`, `under_review`) for a shipped
   window: the frozen test fails and names the window (Task 2, `frozen.test.ts`).
5. **A secret or the VPS identity leaks into the committed JSON**: the frozen test scans every file for
   the IP, `host`, `account_id` and `isoca-radar-scanner` (Task 2).

---

### Task 1: The backup runbook

**Files:**
- Create: `docs/superpowers/runbooks/old-vps-backup.md`

**Interfaces:**
- Produces: on the PC, `$B` (the backup folder) holding `cute_trading.dump`, `archive/`, `capture/`,
  `manifests/{archive,capture}.sha256`, `config/`, and `frozen/{public.json,cards-7d.json,
  cards-30d.json,cards-all.json,health-now.json}`. Task 2 reads `$B/frozen/`.

Header notes (one line each): what it does; the deadline 2026-11-13; the VPS rules above; where each
step runs (VPS as root over SSH, PC in Git Bash, PC in WSL Ubuntu); `$B` is set in step 1 and every PC
step starts by setting it again.

- [ ] **Step 1: Write the runbook with these steps, each as command → expected → if not**

| # | Where | What | Key command(s) | Expected |
|---|---|---|---|---|
| 1 | WSL | Set `$B`, create it, copy the SSH key into WSL with mode 600 (keys on `/mnt/c` are 777 and ssh refuses them) | `B=/mnt/e/isoca-radar-backup; mkdir -p "$B"/{manifests,config,frozen}; install -m 600 /mnt/c/Users/Isabelly/.ssh/id_ed25519 ~/.ssh/isoca_old; ssh -p 50158 -i ~/.ssh/isoca_old root@31.97.23.123 'hostname; which rsync'` | the old host's name and `/usr/bin/rsync`; if rsync is missing: STOP, tell Claude (fallback is `scp -r` per day folder) |
| 2 | VPS | Pre-flight: services, sizes, free disk | `cd /home/isoca/projects/cute-trading && docker ps --format '{{.Names}} {{.Status}}' \| grep cute-trading; du -sh /var/lib/docker/volumes/cute-trading_{pgdata,archive,data,public-data}/_data; df -h /` | `postgres`, `api`, `web`, `oauth2-proxy` Up; no scanner/analyze/compact Up; four sizes; `/` has ≥ 1 GB free |
| 3 | WSL | The destination fits | `df -BG "$B"` | free ≥ (pgdata + archive + data sizes from step 2) × 1.2; **STOP** if not, pick another `$B` |
| 4 | VPS | Manifests (nice'd, relative paths) | `mkdir -p /root/ct-backup && for v in archive data; do (cd /var/lib/docker/volumes/cute-trading_$v/_data && nice -n 19 find . -type f -print0 \| sort -z \| xargs -0 nice -n 19 sha256sum) > /root/ct-backup/$v.sha256; wc -l /root/ct-backup/$v.sha256; done` | two line counts > 0 (raw capture may be small) |
| 5 | WSL | Stream the dump to the PC | `ssh … root@31.97.23.123 'docker exec cute-trading-postgres-1 pg_dump -U ct_owner -d cute_trading -Fc -Z 6' > "$B/cute_trading.dump" && ls -l "$B/cute_trading.dump"` | a non-zero size, no error text; **STOP** on any error |
| 6 | WSL | The dump is readable | `docker run --rm -v "$(wslpath -w "$B")":/b postgres:17 pg_restore -l /b/cute_trading.dump \| grep -c 'TABLE DATA'` (run from Git Bash with `-v "$(cygpath -w "$B")":/b` if `docker` isn't in WSL) | `26` (one per table in `crates/store/migrations`) |
| 7 | WSL | Archive | `rsync -a --partial --append-verify --info=progress2 -e "ssh -p 50158 -i ~/.ssh/isoca_old" root@31.97.23.123:/var/lib/docker/volumes/cute-trading_archive/_data/ "$B/archive/"` | ends with `100%`; re-run the same command after any interruption |
| 8 | WSL | Archive verified | `scp -P 50158 -i ~/.ssh/isoca_old root@31.97.23.123:/root/ct-backup/archive.sha256 "$B/manifests/" && (cd "$B/archive" && sha256sum -c --quiet ../manifests/archive.sha256) && echo archive-ok` | `archive-ok`; failing lines = re-run step 7 then this step |
| 9 | WSL | Raw capture | step 7's command with `cute-trading_data/_data/` → `"$B/capture/"` | ends with `100%`; skippable only if time runs out (write that down) |
| 10 | WSL | Raw capture verified | step 8 with `data.sha256` and `capture` | `capture-ok` |
| 11 | WSL | Configs (no `.env`) | `scp -P 50158 -i ~/.ssh/isoca_old -r root@31.97.23.123:'/home/isoca/projects/cute-trading/{compose.yml,compose.web.yml,config}' "$B/config/"` | the three paths exist in `$B/config` |
| 12 | WSL | Frozen JSON: `public.json` and cards via the web container's busybox `wget` on the api's network | `ssh … 'docker exec cute-trading-web-1 cat /srv/ct-public-root/math-data/public.json' > "$B/frozen/public.json"; for w in 7d 30d all; do ssh … "docker exec cute-trading-web-1 wget -qO- 'http://api:8081/api/cards?win=$w'" > "$B/frozen/cards-$w.json"; done; ssh … "docker exec cute-trading-web-1 wget -qO- http://api:8081/api/health/now" > "$B/frozen/health-now.json"; wc -c "$B"/frozen/*.json; grep -c '"card":"SWEET"' "$B"/frozen/cards-*.json` | five non-zero sizes; `public.json` < 65536; each cards file counts `1` |
| 13 | Git Bash | Test restore into a throwaway Postgres | `docker run -d --name ct-restore -e POSTGRES_PASSWORD=restore -v "$(cygpath -w "$B")":/b postgres:17` → wait for `pg_isready` → `createdb -U postgres cute_trading` → `pg_restore -U postgres -d cute_trading --no-owner --no-privileges /b/cute_trading.dump` → `psql -U postgres -d cute_trading -Atc "select (select count(*) from episodes),(select count(*) from card_results),(select max(minute_ms) from health)"` → `docker rm -f ct-restore` | three numbers, all > 0; the third is the frozen moment (ms), written down for Task 2 |
| 14 | WSL | DuckDB reads the archive | `python3 -m venv /tmp/ddb && /tmp/ddb/bin/pip -q install duckdb && /tmp/ddb/bin/python -c "import duckdb,sys; print(duckdb.sql(\"select count(*) from read_parquet('$B/archive/*/*.parquet', union_by_name=true)\").fetchone())"` | one tuple with a count > 0 |
| 15 | VPS | Clean up the manifests | `rm -r /root/ct-backup && echo cleaned` | `cleaned` |
| 16 | — | Tell Claude: step 13's three numbers, whether step 9 was skipped, and that `$B/frozen` is ready | — | — |

The `ssh …` in the table is `ssh -p 50158 -i ~/.ssh/isoca_old root@31.97.23.123`; the runbook writes it
out in full in every step (self-contained). The `api` hostname resolves from the web container because
both share the `ct_radar` network (`compose.web.yml`).

- [ ] **Step 2: Check the runbook against the format**

Run: `grep -c '^→ expected:' docs/superpowers/runbooks/old-vps-backup.md; grep -c '^\*\*STOP\*\*' docs/superpowers/runbooks/old-vps-backup.md; grep -ciE 'as in|see .*runbook' docs/superpowers/runbooks/old-vps-backup.md`
Expected: `16` (or 15 if step 16 has none), at least `3` (steps 3, 5, and 1's rsync-missing case), `0`.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/runbooks/old-vps-backup.md
git commit -m "runbook: back up the old VPS before 11-13: dump, archive, raw capture, verified, and the frozen JSON"
```

Then hand the runbook to the owner. Task 2 waits for runbook step 16.

### Task 2: Commit the frozen data

**Files:**
- Create: `web/frozen/public.json`, `web/frozen/cards-7d.json`, `web/frozen/cards-30d.json`,
  `web/frozen/cards-all.json` (copied from `$B/frozen/`; `health-now.json` is **not** committed: it
  names the host)
- Create: `web/frozen/README.md`
- Test: `web/src/frozen.test.ts`

**Interfaces:**
- Consumes: `$B/frozen/*.json` from Task 1; `CardsResponse`, `CardBody`, `CardStatus` from
  `web/src/api/types.ts`; the existing `realSnapshot.test.ts` (run with `PUBLIC_JSON`).
- Produces for plan 2: `web/frozen/public.json` (a `PublicSnapshot`), `web/frozen/cards-{7d,30d,all}.json`
  (each a `CardsResponse`), and `FROZEN_MS` documented in the README (the last capture minute, ms, from
  runbook step 13).

- [ ] **Step 1: Write the failing test `web/src/frozen.test.ts`**

```ts
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import type { CardsResponse } from './api/types';

const dir = new URL('../frozen/', import.meta.url);
const read = (f: string) => readFileSync(new URL(f, dir), 'utf8');
const WINS = ['7d', '30d', 'all'] as const;

describe('frozen data', () => {
  it.each(WINS)('cards-%s.json is a CardsResponse with a final SWEET card', (w) => {
    const r = JSON.parse(read(`cards-${w}.json`)) as CardsResponse;
    expect(r.win).toBe(w);
    const sweet = r.cards.find((c) => c.card === 'SWEET');
    expect(sweet?.series.sweet).toBeDefined();
    expect(sweet?.status, `SWEET ${w}`).toBe('live');
  });
  it('public.json is a v1 snapshot under 64 KB', () => {
    const s = read('public.json');
    expect(s.length).toBeLessThan(65536);
    expect(JSON.parse(s).v).toBe(1);
  });
  it.each(['public.json', ...WINS.map((w) => `cards-${w}.json`)])('%s names no host, IP or account', (f) => {
    const s = read(f);
    for (const bad of ['31.97.23.123', '"host"', 'account_id', 'isoca-radar-scanner']) expect(s).not.toContain(bad);
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd web && npx vitest run src/frozen.test.ts`
Expected: FAIL with `ENOENT` for `frozen/cards-7d.json`.

- [ ] **Step 3: Copy the four files from `$B/frozen/` into `web/frozen/` byte for byte, and write `web/frozen/README.md`**

README (short, no host names): what each file is and which page reads it; exported 2026-10-xx from the
old VPS (runbook `old-vps-backup.md` step 12); `FROZEN_MS = <step 13's third number>` and its UTC time;
capture stopped 2026-10-19; "never edit by hand: re-export or leave".

- [ ] **Step 4: Run the frozen test and the real-snapshot test**

Run: `cd web && npx vitest run src/frozen.test.ts && PUBLIC_JSON=frozen/public.json npx vitest run src/math/realSnapshot.test.ts`
Expected: both PASS. A SWEET status other than `live` for one window: STOP and report it to the owner
(plan 2 must then decide whether that window ships); do not edit the JSON.

- [ ] **Step 5: Run the whole web suite**

Run: `cd web && npx vitest run`
Expected: all test files pass (the previous floor was 77 files; now 78).

- [ ] **Step 6: Commit**

```bash
git add web/frozen web/src/frozen.test.ts
git commit -m "web: the frozen data: the last public.json and the SWEET cards for 7D, 30D and ALL, pinned by a test"
```
