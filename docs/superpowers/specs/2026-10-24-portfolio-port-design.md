# Portfolio port: cute-trading frozen, on a new VPS run by isoca-infra

Status: approved by the owner (2026-10-24).

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
 OLD VPS (expires 2026-11-13)           OWNER'S PC
 ┌──────────────────────────┐  rsync  ┌──────────────────────┐
 │ postgres · parquet · raw │ ──────► │ full backup (~30 GB) │
 │ public.json · /api/cards │ ──┐     └──────────────────────┘
 └──────────────────────────┘   │ frozen JSON, committed
   dies with humor-do-dia,      ▼
   whoami, web/apiexample    GITHUB                                 NEW VPS (KVM2)
                             ┌──────────────────────────────┐       ┌─────────────────────────────┐
                             │ cute-trading: push to main   │       │ Traefik (HTTPS, headers, RL)│
                             │  └ Actions: build web + wasm │ rsync │  radar.isoca… ─┐            │
                             │     └ isoca-infra's deploy   │ ────► │  isoca.space  ─┤ nginx, ro  │
                             │       workflow (ssh: deploy) │  ssh  │       /srv/sites/<site>     │
                             └──────────────────────────────┘       └─────────────────────────────┘
```

## 2. Goals

1. **Capture everything from the old VPS before it expires**: the Postgres database, the Parquet
   archive and whatever raw capture is left (~30 GB in all), onto the owner's PC, verified.
2. **cute-trading, frozen, public, static**, at radar.isoca.space, with exactly two features:
   - **/math**: the 7-chapter story and playground (Rust via WASM), as today, reading a frozen
     `public.json`.
   - **Sweet spots**: the page as today (side × dimension grid, folds) but **ALL window only, no window
     switch** (owner, 2026-10-25: 7D and 30D held the same three days and were still `warming`), reading
     frozen JSON instead of `/api/cards`.
   - plus a short **"what it took to run live"** section (§5.3).
3. **isoca-infra**: a new repo that sets up and hardens the KVM2 and deploys static sites to it.
4. **DNS cut-over**: isoca.space, www.isoca.space and radar.isoca.space point at the new box.
5. **A first, simple isoca.space home** linking to cute-trading (content kept minimal, §7).
6. **Deploys from GitHub Actions**, not from the owner's PC (§6.4).
7. **The owner's GitHub profile README** (their existing landing) updated to point at cute-trading,
   isoca-infra and isoca.space (§7).

## 3. Non-goals (this phase)

- No live data, no scanner, analyze, compact, Postgres or API on the new box.
- Episodes, Paper, Papers, Health, Radar and the explain side panel are **not deployed**. Their code stays
  in the repo; deleting it is not part of this work.
- No login anywhere yet. isoca-infra only leaves room for one (§6.5).
- The games: separate projects, brainstormed later.
- humor-do-dia (and its db), webappexample, apiexample, whoami: dropped, nothing copied.
- The old Python `frontend/` and `backend/` and `legacy/`: untouched, not deployed.

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
| `cards-all.json` | `GET /api/cards?win=all` | Sweet spots |

The old box's login works again (the owner applied the oauth `read:org` fix there). The export can go
through it, or from inside the box (e.g. a throwaway curl container on the api's internal network), or,
as a fallback, by running `api` on the PC against the restored Postgres dump. Builder's choice. Also record the timestamp of the last capture
(the "frozen on" date shown on the site) and the export's provenance in a short README next to the files.

### 5.2 The site

- One static build (`web/`, Vite) containing: `/` (a small landing), `/math`, `/sweet`.
- **Landing `/`** — *Claude's call*: one or two sentences on what isoca-radar was (a read-only
  Polymarket arbitrage lab, paper only), two cards linking to /math and /sweet, and the cost section.
- A minimal header with only those links. No live dot, no fake toggle, no other nav entries.
- **Sweet spots** reuses `Sweet.tsx` and its components, without the window switch; it reads
  `cards-all.json`. No polling tickers.
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

Repo: https://github.com/isabellybaldo/isoca-infra (exists). **The owner does every push** to every
repo (and merges PRs); Claude commits locally and stops there.

isoca-infra is a **public** GitHub repo and is treated as part of the portfolio: clean history, a README
that explains the setup (with a drawing), readable runbooks. Nothing that helps an attacker or names the
box is committed: no IP, SSH port, user names, key fingerprints, `.env` contents, or the real hostnames
beyond the public domains. Those live in one gitignored local file (e.g. `local.env`, with a committed
`local.env.example` of placeholders) that the runbooks read, and in GitHub encrypted secrets for the
workflows (§6.4). A secret scan (e.g.
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
  sudo, not in the `docker` group** (docker group is root-equivalent) — *Claude's call*. It can read
  `/srv/sites` and system state (disk, memory, logs readable without root) to check deploys; it does not
  deploy (§6.4).
- A `deploy` user for GitHub Actions only — *Claude's call*: no shell beyond what deploying needs, no
  sudo, not in `docker`; owns `/srv/sites/*` and the Traefik dynamic-config folder. Its key is restricted
  in `authorized_keys` (`restrict`, a forced command that only accepts an upload into a site's release
  folder and switches its `current` link).
- ufw: allow only the new SSH port, 80, 443. Check Hostinger's panel firewall agrees.
- fail2ban on SSH; unattended security upgrades.
- Docker Engine from Docker's official repo with json-file log limits (it runs Traefik and nginx, §6.3).

### 6.3 Reverse proxy

**Traefik** (owner's choice), in Docker, as on the old box. Traefik doesn't serve files itself, so one
small **nginx-unprivileged** container (old box: 32 MB limit) serves `/srv/sites/` bind-mounted
**read-only**, one server block per site — *Claude's call* (one shared file server, not one per site).

- Routing from Traefik's **file provider** (dynamic config committed in isoca-infra, IP-free), not Docker
  labels, so adding a site is a config file, not a container change — *Claude's call*.
- Let's Encrypt certificates per domain (HTTP challenge); http → https, www → apex.
- The security headers the old box set (`ct-sec`: HSTS, frame deny, nosniff, referrer; `ct-csp`; the
  /math CSP with `'wasm-unsafe-eval'`), carried over from `compose.web.yml`.
- **Rate limit and in-flight limit** per IP on every public router (project rule), with the old /math
  numbers as the starting point (5 req/s, burst 20, in-flight 8).
- `radar.isoca.space`: the cute-trading build; `isoca.space`: the home (§7).
- No Docker socket mounted into Traefik (the file provider doesn't need it).
- Traefik and nginx stay hardened as before: read-only, `cap_drop: ALL`, `no-new-privileges`, cpu and
  memory limits.

The compose file and the Traefik config live in isoca-infra; changing them is an owner step (Docker
access is root-equivalent, §6.2).

### 6.4 Deploys (GitHub Actions)

Owner's choice: deploys run in GitHub Actions, not on the owner's PC. Never build Rust/WASM on the KVM2.

- **One reusable deploy workflow lives in isoca-infra** (`workflow_call`): it takes a site name and a
  built artifact, rsyncs it over SSH as `deploy` into a new release folder, switches the site's relative
  `current` symlink atomically (relative, so it resolves inside nginx's read-only mount too), keeps the
  previous releases for rollback, and checks the site answers over HTTPS afterwards.
- **cute-trading** gets a workflow: on push to `main` (the owner merges PRs) and on manual dispatch, build
  `web/` (WASM included, reusing what `ci.yml` already does), upload `dist/` as an artifact, then call
  isoca-infra's deploy workflow, pinned to a commit SHA.
- **isoca-infra** deploys its own static parts the same way: the isoca.space home (§7), and the Traefik
  **dynamic config** (Traefik's file provider reloads on change, so no Docker access is needed). The compose
  file and Traefik's static config stay owner steps (Docker access is root-equivalent).
- A manual **rollback** dispatch switches a site's `current` back to the previous release.
- Secrets (the `deploy` private key, host, SSH port, pinned `known_hosts`) live in GitHub encrypted
  secrets, in a `production` environment on each repo that deploys; never in a repo. Third-party actions
  are pinned by SHA.
- The link between repos is the reusable workflow plus the per-site entry (domain, nginx server block,
  router) in isoca-infra.

### 6.5 Room for a login (not built)

The Traefik config allows putting a `forwardAuth` middleware in front of a single site later
(oauth2-proxy in Docker, GitHub provider, as on the old box). The games will be its first user; who may log in is decided
then.

## 7. isoca.space home

A first, minimal page: name, one line, a link card to cute-trading (radar.isoca.space) and links to the
owner's GitHub. Its source lives
in a folder inside isoca-infra for now (no third repo this phase) — *Claude's call*. Its real content
and design are a separate brainstorm; this phase only replaces the whoami placeholder so the domain isn't
dead after cut-over.

The owner's **GitHub profile README** (already a small landing page) gets the matching update in the same
step: cute-trading (frozen portfolio piece, with its link), isoca-infra, isoca.space. Its wording is
shown to the owner before it's pushed.

## 8. Order (driven by the 2026-11-13 expiry)

Steps 1 and 2 happen first and soon (target: the week of 2026-10-26), well before anything else, leaving
margin for a slow transfer. Everything after them no longer needs the old box.

1. **Backup first**: full ~30 GB capture from the old VPS to the PC, checksummed (§9).
2. Export the frozen JSON (§5.1), commit it.
3. isoca-infra: repo, hardening runbook, owner runs it; Traefik and nginx up.
4. cute-trading portfolio build (§5.2–5.3); deploy to the new box under a test subdomain
   (`new.isoca.space`, added to the zone so Traefik gets a real certificate before cut-over) — *Claude's call*.
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
- Final rate-limit numbers per router (start from the old /math ones).
- Release retention count; how the forced command and the reusable workflow split the work.

## 11. Amendments (2026-10-25, owner decisions and findings while building plans 1–2)

- **isoca.space → isabellybaldo.github.io.** isoca.space and www.isoca.space redirect (302, so it can change
  later) to the owner's existing Astro portfolio, isabellybaldo.github.io, instead of a placeholder page in
  isoca-infra (§7 superseded). That site gets a live isoca-radar project card linking to radar.isoca.space;
  the GitHub profile README gets the matching line. Both repos are the owner's; the owner pushes.
- **The old box is off** (owner, 2026-10-25): every container stopped with restart=no after the verified
  backup. radar.isoca.space and isoca.space are dark until the DNS cut-over.
- **nginx ≥ 1.30.6** (the 2026-10-14 security release: CVE-2026-15176, -93479, -97205, -95101, -97629), the
  unprivileged image pinned by digest. The config never uses `alias`, SSI, `aio`, `map` regex captures,
  HTTP/3 or TLS (Traefik is the edge); `server_tokens off`.
- **Traefik runs unprivileged:** container entrypoints on 8000/8443, published as host 80/443, so it needs
  no `NET_BIND_SERVICE`; no Docker socket (file provider); no dashboard; HTTP/3 not enabled.
- **Deploy mechanics:** the reusable workflow streams a tar.gz over SSH to a forced command
  (`site-deploy`) owned by root on the box, which validates the site name and archive (no links, no device
  files, size cap), unpacks into a new release folder and switches `current`; `rollback` and
  `edge-config` (the Traefik dynamic files) go through the same command. This replaces "rsync" in §6.4: a
  forced command can't safely allow general rsync.
- **CI hygiene:** GitHub actions on their Node 24 majors (checkout v7, cache v6, setup-node v7,
  upload-artifact v7, download-artifact current), pinned by SHA. The owner's PC moves off Node 23 to 24 LTS
  (not part of any plan).
- **Owner's SSH key has a passphrase:** the setup runbook enables the Windows `ssh-agent` service first.
