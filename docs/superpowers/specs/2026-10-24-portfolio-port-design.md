# Portfolio port: cute-trading frozen, on a new VPS run by isoca-infra

Status: design, awaiting owner approval (2026-10-24).

## 1. Intent

The Hostinger VPS that runs isoca-radar (radar.isoca.space) expires on **2026-11-13**; the owner still has
root SSH to it. On 2026-10-19
Hostinger CPU-limited it: the scanner sat at ~65 % of a core and `compact` used 2 cores, so capture was
stopped and the site has served frozen data since. Running everything live is impractical on the
replacement box (a Hostinger **KVM2**, already bought, empty, no Docker).

So cute-trading becomes a **portfolio piece**: a frozen, public, static site that shows the two best
things it made, plus an honest "what it took to run this live" story. The scanner code stays in the repo
to be read; nothing runs.

A new repo, **isoca-infra**, owns the new VPS: its hardening, the reverse proxy, the domains and the
deploys. cute-trading is the first site it serves; the isoca.space portfolio home and, later, ad-free
sudoku and mahjong games (behind a login) follow.

```
 OLD VPS (expires Nov 2026)                 OWNER'S PC                      NEW VPS (KVM2)
 ┌──────────────────────────┐   rsync   ┌────────────────────────┐       ┌─────────────────────────────┐
 │ postgres · parquet · raw │ ────────► │ full backup (~30 GB)   │       │ Caddy (HTTPS, headers, RL)  │
 │ public.json · /api/cards │ ────────► │ frozen JSON → repo     │       │  isoca.space  → home        │
 └──────────────────────────┘           │ npm run build → dist/  │ rsync │  radar.isoca… → /srv/sites/ │
   dies with humor-do-dia,              │ isoca-infra deploy     │ ────► │                 radar       │
   whoami, web/apiexample               └────────────────────────┘       │ (Docker installed, idle)    │
                                                                         └─────────────────────────────┘
```

## 2. Goals

1. **Capture everything from the old VPS before it expires**: the Postgres database, the Parquet
   archive and whatever raw capture is left (~30 GB in all), onto the owner's PC, verified.
2. **cute-trading, frozen, public, static**, at radar.isoca.space, with exactly two features:
   - **/math**: the 7-chapter story and playground (Rust via WASM), as today, reading a frozen
     `public.json`.
   - **Sweet spots**: the page as today (side × dimension grid, 7D / 30D / ALL windows, folds), reading
     frozen JSON instead of `/api/cards`.
   - plus a short **"what it took to run live"** section (§5.3).
3. **isoca-infra**: a new repo that sets up and hardens the KVM2 and deploys static sites to it.
4. **DNS cut-over**: isoca.space, www.isoca.space and radar.isoca.space point at the new box.
5. **A first, simple isoca.space home** linking to cute-trading (content kept minimal, §7).

## 3. Non-goals (this phase)

- No live data, no scanner, analyze, compact, Postgres or API on the new box.
- Episodes, Paper, Papers, Health, Radar and the explain side panel are **not deployed**. Their code stays
  in the repo; deleting it is not part of this work.
- No login anywhere yet. isoca-infra only leaves room for one (§6.5).
- The games: separate projects, brainstormed later.
- humor-do-dia (and its db), webappexample, apiexample, whoami: dropped, nothing copied.
- The old Python `frontend/` and `backend/` and `legacy/`: untouched, not deployed.
- The pending oauth `read:org` fix on the old box: moot, not pursued.

## 4. Anti-goals (fails even if it "works")

- The frozen site looks **broken**: "stale", "live unavailable", "warming", "recomputing", spinners or
  error states because data is old. Frozen must read as intentional ("frozen on <date>").
- The new box ends up using meaningful CPU, memory or network for cute-trading.
- Claude holds root, or can change firewall/SSH/users on the new box.
- The ~30 GB backup is lost or unverified when the old box dies.
- Any public path that isn't static, or isn't rate-limited (project rule).

## 5. cute-trading (this repo)

### 5.1 Frozen data

Exported once from the old VPS, **committed to the repo** (small JSON, so the build is reproducible
without the old box) — *Claude's call*:

| File | Source | Used by |
|---|---|---|
| `public.json` | the api's last snapshot (`/var/lib/ct-public/public.json`, ≤ 64 KB) | /math |
| `cards-7d.json`, `cards-30d.json`, `cards-all.json` | `GET /api/cards?win=…` | Sweet spots |

The old box's login is broken (oauth 403), so the export goes **around** it: from inside the box (e.g. a
throwaway curl container on the api's internal network) or, as a fallback, by running `api` on the PC
against the restored Postgres dump. Builder's choice. Also record the timestamp of the last capture
(the "frozen on" date shown on the site) and the export's provenance in a short README next to the files.

### 5.2 The site

- One static build (`web/`, Vite) containing: `/` (a small landing), `/math`, `/sweet`.
- **Landing `/`** — *Claude's call*: one or two sentences on what isoca-radar was (a read-only
  Polymarket arbitrage lab, paper only), two cards linking to /math and /sweet, and the cost section.
- A minimal header with only those links. No live dot, no fake toggle, no other nav entries.
- **Sweet spots** reuses `Sweet.tsx` and its components; the data client reads the static JSON for the
  chosen window. No polling tickers.
- **/math** keeps its current behavior and bundle; the "live" parts (strip, ladder) render from the
  frozen `public.json` and are labeled as frozen, never as stale or unavailable.
- Every page shows one quiet line: "Frozen snapshot from <date>. Capture stopped on 2026-10-19."
- Every "stale / warming / recomputing / unavailable / live" state that would trigger because of the
  data's age must be suppressed or reworded (anti-goal §4). Tests should pin this with the real frozen
  files.
- Everything is same-origin static files: CSP stays `connect-src 'self'`, `'wasm-unsafe-eval'` for /math.
- The dashboard build for the undeployed pages may stay in the repo; it's not shipped.

### 5.3 "What it took to run live"

One short section, drawn rather than written (owner prefers schematics to prose), with measured numbers
from the repo's own docs:

- Polymarket market feed: ~123 GB/day raw (~1.4 MB/s in, all day), ~33 GB/day after zstd.
- Scanner: ~65 % of a core sustained; compact: 2 cores while compacting; together they triggered
  Hostinger's CPU limit on 2026-10-19.
- Disk budget: ≤ 30 GB raw capture (~24 h) + ≤ 40 GB Parquet archive.
- Service memory limits (api 128 MB, analyze 128 MB, compact 512 MB, web 32 MB, oauth2-proxy 64 MB,
  plus Postgres and the scanner).

The builder verifies each number against `spikes/probe/RESULTS.md`, `PROJECT.md`, the compose files and
the 10-19 notes before it ships; anything unverifiable is dropped, not guessed. Placement: on the landing
(§5.2), linked from /math's last chapter — *Claude's call*.

### 5.4 What stays in the repo

All Rust crates and bins (scanner included) and the undeployed pages, unchanged. `README.md` and
`PROJECT.md` get a short note: the project is frozen, where it's served, how to rebuild the site.

## 6. isoca-infra (new repo)

### 6.0 The repo is public, and a portfolio piece

isoca-infra is a **public** GitHub repo and is treated as part of the portfolio: clean history, a README
that explains the setup (with a drawing), readable runbooks. Nothing that helps an attacker or names the
box is committed: no IP, SSH port, user names, key fingerprints, `.env` contents, or the real hostnames
beyond the public domains. Those live in one gitignored local file (e.g. `local.env`, with a committed
`local.env.example` of placeholders) that the runbooks and the deploy script read. A secret scan (e.g.
gitleaks) runs before every push, and every commit is reviewed for these leaks — *the scan tool is
Claude's call*.

### 6.1 Box

Hostinger KVM2 (2 vCPU, 8 GB RAM, 100 GB NVMe per Hostinger's plan; builder confirms from the panel and
the installed OS, assumed Ubuntu 24.04 LTS).

### 6.2 Initial hardening (a runbook in the owner's format: command → expected, no prose)

The owner runs every root step; Claude never holds root.

- An admin user for the owner (sudo), SSH key only.
- SSH moved off port 22; `PasswordAuthentication no`, `PermitRootLogin no`.
- A `claude` user with its own SSH key (generated on the owner's PC, used only by Claude Code): **no
  sudo, not in the `docker` group** (docker group is root-equivalent) — *Claude's call*. It owns
  `/srv/sites/*` and can deploy static files there; that's all this phase needs.
- ufw: allow only the new SSH port, 80, 443. Check Hostinger's panel firewall agrees.
- fail2ban on SSH; unattended security upgrades.
- Docker Engine from Docker's official repo with json-file log limits, installed but idle this phase
  (for oauth2-proxy and future projects).

### 6.3 Reverse proxy

**Caddy**, installed natively (systemd) — *Claude's call* (lighter than Traefik; static file serving
needs no container). It:

- serves `/srv/sites/<site>/` with `file_server`, automatic HTTPS for each domain;
- sets the security headers the old Traefik set (HSTS, frame deny, nosniff, referrer, the CSPs above);
- **rate-limits** per IP (project rule), via the `caddy-ratelimit` module (Caddy built/added with it);
- redirects www → apex, http → https;
- `radar.isoca.space`: the cute-trading build; `isoca.space`: the home (§7).

The Caddyfile lives in isoca-infra; changing it is an owner step (needs root to reload).

### 6.4 Deploys

Build on the PC, ship files — *Claude's call* (matches "PC builds, VPS runs"; never build Rust/WASM on the
KVM2). isoca-infra has a deploy script that takes a site name and a built folder (e.g. cute-trading's
`web/dist`), rsyncs it as `claude` to a new release folder, and switches a `current` symlink atomically,
keeping the previous release for rollback. cute-trading only has to produce `dist/`; the link between
repos is that script plus a per-site entry (domain, source repo, build command) in isoca-infra.

### 6.5 Room for a login (not built)

The Caddyfile structure allows putting `forward_auth` in front of a single site later (oauth2-proxy in
Docker, GitHub provider, as on the old box). The games will be its first user; who may log in is decided
then.

## 7. isoca.space home

A first, minimal page: name, one line, a link card to cute-trading (radar.isoca.space). Its source lives
in a folder inside isoca-infra for now (no third repo this phase) — *Claude's call*. Its real content
and design are a separate brainstorm; this phase only replaces the whoami placeholder so the domain isn't
dead after cut-over.

## 8. Order (driven by the 2026-11-13 expiry)

Steps 1 and 2 happen first and soon (target: the week of 2026-10-26), well before anything else, leaving
margin for a slow transfer. Everything after them no longer needs the old box.

1. **Backup first**: full ~30 GB capture from the old VPS to the PC, checksummed (§9).
2. Export the frozen JSON (§5.1), commit it.
3. isoca-infra: repo, hardening runbook, owner runs it; Caddy up.
4. cute-trading portfolio build (§5.2–5.3); deploy to the new box under a test subdomain
   (`new.isoca.space`, added to the zone so Caddy gets a real certificate before cut-over) — *Claude's call*.
5. DNS (managed in Hostinger's panel; the owner uses no email on isoca.space): first export the whole zone and keep every record (MX, TXT, anything unknown) untouched except
   the A/AAAA/CNAME records for isoca.space, www and radar, which move to the new IP, and the dropped
   tenants' subdomains, which are removed — *Claude's call*. Lower TTL ahead of time; verify HTTPS after.
6. isoca.space minimal home.
7. The old box is left to expire; nothing else to migrate.

## 9. Backup details

- Postgres: `pg_dump -Fc` from inside the container, copied out.
- Parquet archive and raw capture volumes: `rsync` over SSH to the PC (resumable). The owner puts the
  total at ~30 GB; the disk budget allows up to ~70 GB, so the runbook measures each volume (`du`) first.
  Order: Postgres dump, then the Parquet archive, then raw capture last; raw capture is the one part that
  may be dropped if time runs out (the archive already holds the compacted hours) — *Claude's call*.
- Verification: file counts and sha256 manifests compared on both ends; a test restore of the dump into
  a local Postgres; one DuckDB query over the archive.
- Also copy the old box's `compose*.yml` and `config/` (not `.env`: its secrets aren't reused).
- The backup's destination on the PC is the owner's choice and stays out of git.

## 10. Left to the builder

- Exact export method around the broken login (§5.1).
- How the Sweet data client switches to static JSON (build flag, separate entry, or a fixtures client).
- The landing's layout and the cost drawing's form, within the owner's "drawings over text" preference.
- Caddy rate-limit numbers (old /math: 5 req/s, burst 20; in-flight 8 has no Caddy equivalent; pick
  something equivalent for static pages).
- Release retention count, deploy script language.
