# Old VPS backup and frozen export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Before the old VPS expires on 2026-11-13, copy all of isoca-radar's data to the owner's PC
(verified), and commit the four frozen JSON files the portfolio site needs.

**Architecture:** One owner-run runbook: the PC side in **Windows PowerShell 5.1** with Windows'
built-in OpenSSH 9.5 (no WSL, no Git Bash), the VPS side in a root shell. The VPS stages everything
small (the `pg_dump`, the frozen JSON, configs, sha256 manifests) in `/root/ct-backup`; the PC pulls it
and the two big volumes with resumable `sftp reget -r`, checks every file against the manifests with
`Get-FileHash`, test-restores the dump and queries the archive with DuckDB. Claude then commits the JSON
files under `web/frozen/` with a test that pins their shape.

**Tech Stack:** PowerShell 5.1, Windows OpenSSH 9.5 (`ssh`, `sftp`), bash on the VPS, Docker Desktop
(postgres:17 for the test restore), DuckDB (Python package in a throwaway venv), Vitest.

**Spec:** `docs/superpowers/specs/2026-10-24-portfolio-port-design.md` (§5.1, §8 steps 1–2, §9).
This is plan 1 of 3; plan 2 (the frozen site) reads `web/frozen/`, plan 3 is isoca-infra.

## Global Constraints

- Old VPS: root over SSH on port 50158 at 31.97.23.123, **password login**, project folder
  `/home/isoca/projects/cute-trading`. The PC reaches it as the alias `isoca-old` in
  `C:\Users\Isabelly\.ssh\config` (step 1). Windows OpenSSH can't share a connection, so the runbook
  groups the work into **5 connections = 5 password prompts** (steps 1, 6, 8, 10, 14). The owner runs
  every step; Claude never runs them.
- **PowerShell 5.1's `>` writes UTF-16**: no binary or JSON file is ever produced by a PowerShell
  redirect. Every file is written on the VPS by bash and downloaded with `sftp`.
- Volumes: `cute-trading_pgdata` (dumped, not copied), `cute-trading_archive` (Parquet, one folder
  per UTC day, each with `manifest.json`), `cute-trading_data` (raw capture), `cute-trading_public-data`
  (`public.json`). Database `cute_trading`, owner role `ct_owner`.
- VPS rules: never print `.env` or a container's environment; never restart Docker; install nothing on
  the VPS; never start scanner, analyze or compact.
- Order: Postgres dump → Parquet archive → raw capture (raw is the only part that may be dropped if
  time runs out) → JSON export.
- Backup destination: owner's choice, default `E:\isoca-radar-backup` (E: has ~110 GB free); never
  inside a git checkout.
- Runbook format: numbered steps, one command block → `→ expected:` → `if not:` line, **STOP** after any
  step whose failure must halt; no prose walls; self-contained (no "as in another runbook").
- The owner does every push. Claude commits locally only. No `Co-Authored-By` trailer.
- Frozen JSON is committed to a repo that will be public: it must not contain a host name, IP,
  account id or anything from `.env`.

## Review Focus

1. **A transfer cut off halfway** (laptop sleeps, Wi-Fi drops): re-running the same sftp step resumes
   (`reget -r` appends to partial files) and the following `Get-FileHash` check catches anything
   corrupt or missing (runbook steps 6–11).
2. **The PC disk fills up mid-copy**: the measured total is checked against E:'s free space before the
   first byte moves (runbook step 3).
3. **The VPS disk fills up while staging the dump**: step 2 checks `/` has more free space than the
   pgdata volume (a `-Fc -Z 6` dump is far smaller); if not, step 5's fallback streams the dump through
   `cmd /c` (binary-safe redirect) instead of staging it.
4. **The frozen cards are in a non-final state** (`warming`, `thin`, `under_review`) for a shipped
   window: the frozen test fails and names the window (Task 2, `frozen.test.ts`).
5. **A secret or the VPS identity leaks into the committed JSON**: the frozen test scans every file for
   the IP, `host`, `account_id` and `isoca-radar-scanner` (Task 2).

---

### Task 1: The backup runbook

**Files:**
- Create: `docs/superpowers/runbooks/old-vps-backup.md`

**Interfaces:**
- Produces: on the PC, `E:\isoca-radar-backup` (`$B`) holding `ct-backup\` (`cute_trading.dump`,
  `archive.sha256`, `data.sha256`, `ct-backup.sha256`, `config\`, `frozen\{public.json,cards-7d.json,
  cards-30d.json,cards-all.json,health-now.json}`), `archive\`, `capture\` and `verify.ps1`. Task 2
  reads `$B\ct-backup\frozen\`.

Header notes (one line each): what it does; the deadline 2026-11-13; the VPS rules; the UTF-16 rule;
where each step runs (PC = PowerShell, VPS = the root shell from step 1); every PC step starts with
`$B='E:\isoca-radar-backup'`; the 5 password prompts.

- [ ] **Step 1: Write the runbook with these steps, each as command → expected → if not**

1. **PC** — set `$B`, create `$B\archive`, `$B\capture`; append to `$HOME\.ssh\config` (only if
   `Select-String -Quiet '^Host isoca-old$'` finds nothing; `Add-Content -Encoding ascii`) the block
   `Host isoca-old` / `HostName 31.97.23.123` / `Port 50158` / `User root` / `ServerAliveInterval 30`;
   write `$B\verify.ps1` (below); then `ssh isoca-old` → password 1 → a root prompt. Steps 2, 4, 5 run there.
2. **VPS** — pre-flight: `docker ps --format '{{.Names}} {{.Status}}' | grep cute-trading`;
   `du -sh /var/lib/docker/volumes/cute-trading_{pgdata,archive,data,public-data}/_data`; `df -h /`.
   → `postgres`, `api`, `web`, `oauth2-proxy` Up, no scanner/analyze/compact Up; four sizes; free on `/`
   > the pgdata size (else step 5 uses its fallback). Write the four sizes down.
3. **PC** (a second PowerShell window) — `[int]((Get-PSDrive E).Free/1GB)` → ≥ (pgdata + archive + data)
   × 1.2. **STOP** if not: pick another `$B`.
4. **VPS** — the frozen JSON and configs, inside `bash -euo pipefail <<'SH'` … `SH` so a failure stops
   it without closing the shell:
   ```bash
   D=/root/ct-backup; mkdir -p $D/frozen $D/config; cd /home/isoca/projects/cute-trading
   cp -r compose.yml compose.web.yml config $D/config/
   docker exec cute-trading-web-1 cat /srv/ct-public-root/math-data/public.json > $D/frozen/public.json
   for w in 7d 30d all; do docker exec cute-trading-web-1 wget -qO- "http://api:8081/api/cards?win=$w" > $D/frozen/cards-$w.json; done
   docker exec cute-trading-web-1 wget -qO- http://api:8081/api/health/now > $D/frozen/health-now.json
   wc -c $D/frozen/*.json; grep -c '"card":"SWEET"' $D/frozen/cards-*.json
   ```
   → five non-zero sizes, `public.json` < 65536; each cards file counts `1`. (`api` resolves from the web
   container: both are on `ct_radar`.) **STOP** on any error.
5. **VPS** — the dump and the manifests (same heredoc form; slow):
   ```bash
   D=/root/ct-backup
   docker exec cute-trading-postgres-1 pg_dump -U ct_owner -d cute_trading -Fc -Z 6 > $D/cute_trading.dump
   for v in archive data; do (cd /var/lib/docker/volumes/cute-trading_$v/_data && find . -type f -print0 | sort -z | xargs -0 -r nice -n 19 sha256sum) > $D/$v.sha256; done
   (cd $D && find . -type f ! -name '*.sha256' -print0 | sort -z | xargs -0 sha256sum) > $D/ct-backup.sha256
   ls -l $D/cute_trading.dump; wc -l $D/*.sha256; df -h /
   ```
   → a dump size, three line counts > 0 (`data.sha256` may be small), `/` not full; then `exit`.
   Fallback when step 2 said `/` is too small: drop the `pg_dump` line here and, on the PC after step 6,
   run `cmd /c "ssh isoca-old ""docker exec cute-trading-postgres-1 pg_dump -U ct_owner -d cute_trading -Fc -Z 6"" > E:\isoca-radar-backup\ct-backup\cute_trading.dump"`
   (one more password; cmd's redirect is binary-safe). **STOP** on any error.
6. **PC** — `sftp isoca-old` (password 2), then type `lcd E:/isoca-radar-backup`, `reget -r /root/ct-backup`,
   `bye` → the transfer list ends without errors. Interrupted: run the same three commands again.
7. **PC** — `& "$B\verify.ps1" "$B\ct-backup\ct-backup.sha256" "$B\ct-backup"` → `ok <n> files`.
8. **PC** — `sftp isoca-old` (password 3): `lcd E:/isoca-radar-backup`,
   `reget -r /var/lib/docker/volumes/cute-trading_archive/_data archive`, `bye`. Re-run after any cut.
9. **PC** — `& "$B\verify.ps1" "$B\ct-backup\archive.sha256" "$B\archive"` → `ok <n> files` with
   `<n>` = the line count of `archive.sha256` from step 5. `FAIL <path>` lines: redo step 8, then this.
10. **PC** — step 8 with `cute-trading_data/_data capture` (password 4). Skippable only if time runs
    out (write that down).
11. **PC** — step 9 with `data.sha256` and `$B\capture` → `ok <n> files`.
12. **PC** — test restore: `docker run -d --name ct-restore -e POSTGRES_PASSWORD=restore -v "${B}\ct-backup:/b" postgres:17`;
    loop `docker exec ct-restore pg_isready -U postgres` until ready;
    `docker exec ct-restore sh -c 'pg_restore -l /b/cute_trading.dump | grep -c "TABLE DATA"'` → `26`;
    `docker exec ct-restore createdb -U postgres cute_trading`;
    `docker exec ct-restore pg_restore -U postgres -d cute_trading --no-owner --no-privileges /b/cute_trading.dump`;
    `docker exec ct-restore psql -U postgres -d cute_trading -Atc "select (select count(*) from episodes),(select count(*) from card_results),(select max(minute_ms) from health)"`
    → three numbers > 0, the third is the frozen moment (write it down); then `docker rm -f ct-restore`.
13. **PC** — DuckDB: `py -3.11 -m venv "$env:TEMP\ddb"; & "$env:TEMP\ddb\Scripts\pip.exe" -q install duckdb`,
    then the venv's `python.exe -c` running
    `duckdb.sql("select count(*) from read_parquet('E:/isoca-radar-backup/archive/*/*.parquet', union_by_name=true)").fetchone()`
    → one tuple with a count > 0.
14. **PC** — `ssh isoca-old "rm -r /root/ct-backup && echo cleaned"` (password 5) → `cleaned`.
15. Tell Claude: step 12's three numbers, whether step 10 was skipped, and that `$B\ct-backup\frozen` is ready.

`verify.ps1` (written by step 1; PowerShell 5.1, ~15 lines): `param([string]$Manifest, [string]$Root)`;
for each manifest line `^([0-9a-f]{64}) [ *]\./(.+)$`, `Get-FileHash -Algorithm SHA256` of
`Join-Path $Root $path` (`/` → `\`); a missing file or a different hash prints `FAIL <path>`; the end
prints `ok <n> files` when nothing failed, otherwise `<k> FAILED of <n>` and exits 1.

- [ ] **Step 2: Check the runbook against the format**

Run: `grep -c '^→ expected:' docs/superpowers/runbooks/old-vps-backup.md; grep -c '^\*\*STOP\*\*' docs/superpowers/runbooks/old-vps-backup.md; grep -ciE 'as in|see .*runbook|wsl|rsync' docs/superpowers/runbooks/old-vps-backup.md`
Expected: `14` (step 15 has none), at least `3` (steps 3, 4, 5), `0`.
Also: save `verify.ps1`'s body to the scratch folder and run it in PowerShell on a two-file sample
manifest (one good, one tampered): expect `FAIL <the tampered one>` and `1 FAILED of 2`.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/runbooks/old-vps-backup.md
git commit -m "runbook: back up the old VPS before 11-13: dump, archive, raw capture, verified, and the frozen JSON"
```

Then hand the runbook to the owner. Task 2 waits for runbook step 15.

### Task 2: Commit the frozen data

> Done 2026-10-25 with one change from below (owner): only `public.json` and `cards-all.json` are
> committed. 7D and 30D were `warming` over the same three days, so only ALL ships. The test checks the
> IP, `srv1125993`, `account_id` and `isoca-radar-scanner` (not `"host"`: the EDGE card's
> `"host":"vps-kvm2"` is a plan label, not an identity), reads the files with `?raw` imports, and
> `.gitattributes` marks `web/frozen/*.json -text`.

**Files:**
- Create: `web/frozen/public.json`, `web/frozen/cards-7d.json`, `web/frozen/cards-30d.json`,
  `web/frozen/cards-all.json` (copied from `E:\isoca-radar-backup\ct-backup\frozen\`; `health-now.json` is **not** committed: it
  names the host)
- Create: `web/frozen/README.md`
- Test: `web/src/frozen.test.ts`

**Interfaces:**
- Consumes: `E:\isoca-radar-backup\ct-backup\frozen\*.json` from Task 1; `CardsResponse`, `CardBody`, `CardStatus` from
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

- [ ] **Step 3: Copy the four files from `E:\isoca-radar-backup\ct-backup\frozen\` into `web/frozen/` byte for byte, and write `web/frozen/README.md`**

README (short, no host names): what each file is and which page reads it; exported 2026-10-xx from the
old VPS (runbook `old-vps-backup.md` step 4); `FROZEN_MS = <step 12's third number>` and its UTC time;
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
