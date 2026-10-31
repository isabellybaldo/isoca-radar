# isoca-infra and the cut-over Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A public isoca-infra repo that sets up the new KVM2 and serves cute-trading's frozen site at
radar.isoca.space, deployed from GitHub Actions, with isoca.space redirecting to isabellybaldo.github.io,
and the github.io site and the GitHub profile pointing at the project.

**Architecture:** Traefik (unprivileged, file provider, Let's Encrypt) in front of one read-only nginx that
serves `/srv/sites/<site>/current`. Actions builds each site, then a reusable workflow in isoca-infra
streams a tar.gz over SSH to a root-owned forced command (`site-deploy`) that unpacks a new release and
switches `current`. The owner runs every root step from two runbooks; Claude never holds root.

**Tech Stack:** Traefik v3 and nginx-unprivileged ≥ 1.30.6 (Docker, pinned by digest), bash, GitHub Actions,
Ubuntu (Hostinger KVM2), Astro (github.io).

**Spec:** `docs/superpowers/specs/2026-10-24-portfolio-port-design.md` §6, §8, §11 (the amendments win over
§6.4 and §7). Plan 3 of 3. Plan 2 left the `radar-site` artifact (`web/dist-portfolio/`) and its nginx
contract.

## Global Constraints

- Repos: isoca-infra at `C:\Users\Isabelly\PycharmProjects\isoca-infra` (clone of the empty public
  `https://github.com/isabellybaldo/isoca-infra`); cute-trading (repo `isabellybaldo/isoca-radar`);
  `C:\Users\Isabelly\PycharmProjects\isabellybaldo.github.io`; the profile repo `isabellybaldo/isabellybaldo`
  (clone it next to them). Claude commits locally; **the owner does every push and merge**; no
  `Co-Authored-By` trailer. Commits in isoca-infra use the GitHub noreply address (repo-local git config,
  already set), never the owner's personal email; shell scripts stay LF on this Windows PC
  (`.gitattributes`: `* text=auto eol=lf`).
- **isoca-infra is public and a portfolio piece.** Never commit: the box's IP, SSH port, user names other
  than the role names `deploy`/`claude`, key material or fingerprints, emails. Those live in the gitignored
  `local.env` (template `local.env.example`) and in GitHub encrypted secrets. gitleaks runs in CI.
- Domains: `radar.isoca.space` (the site), `new.isoca.space` (the same site, for testing before DNS moves;
  removed after cut-over), `isoca.space` and `www.isoca.space` → **302** to `https://isabellybaldo.github.io/`
  (path kept).
- nginx: `nginxinc/nginx-unprivileged:1.30.6-alpine` or newer stable, by digest. Never `alias`, `ssi`,
  `aio`, `map` with regex captures, `http3`/`quic`, `ssl_*`; `server_tokens off`. Per site:
  `root /srv/sites/<site>/current; try_files $uri $uri.html $uri/ =404;`.
- Traefik: v3, latest patch at implementation time, by digest; runs as uid 65532, entrypoints `:8000` (web)
  and `:8443` (websecure) published as host 80/443; file provider on a watched directory; ACME HTTP-01, no
  email; no dashboard/API; no Docker socket; HTTP/3 off.
- Containers: `read_only: true`, `cap_drop: [ALL]`, `security_opt: [no-new-privileges:true]`, cpu and
  memory limits, json-file logs capped (10m × 5). nginx sees only an internal network.
- radar headers: HSTS (1 year, no preload), frame deny, nosniff, referrer `same-origin`, CSP
  `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`.
  Rate limit per IP: average 10/s, burst 40, in-flight 16 (`ipStrategy.depth: 0`).
- nginx caching: `/site-assets/` and `/math-assets/` 1 year immutable; `*.html` and extensionless pages
  `no-cache`; `/frozen/` 1 day. gzip on for html, css, js, json, svg, wasm.
- Box layout: `/srv/sites/<site>/` and its `releases/`, `current` owned by `deploy`; `/srv/edge/dynamic/`
  (Traefik dynamic files) and `/srv/edge/.incoming/` (edge-config's staging, same filesystem) owned by `deploy`;
  `/srv/edge` itself stays root-owned, so `deploy` can't touch `/srv/edge/acme`; `/srv/isoca-infra` (a clone of this repo, owner-managed) holds compose
  and static config; `/usr/local/bin/site-deploy` root-owned 0755.
- Users: owner's admin (sudo, key only); `deploy` (no sudo, not in `docker`, key restricted to
  `site-deploy`); `claude` (no sudo, not in `docker`, in `systemd-journal`, own key). SSH: non-22 port,
  keys only, no root login. ufw: SSH port, 80, 443. fail2ban sshd. unattended-upgrades on.
- Actions: Node 24 majors pinned by full SHA with the tag in a comment (`actions/checkout` v7,
  `actions/cache` v6, `actions/setup-node` v7, `actions/upload-artifact` v7, `actions/download-artifact`
  latest major); `permissions` minimal; deploy jobs only on `main` (push or dispatch), in environment
  `production`.
- Runbooks: numbered steps, command → `→ expected:` → `if not:`, **STOP** where failure must halt,
  self-contained, no prose walls. Placeholders read from `local.env` (`$VPS_IP`, `$SSH_PORT`, `$ADMIN`).

## Review Focus

1. **A malicious or broken archive reaches `site-deploy`** (a symlink to `/etc`, `../` paths, a device
   file, 2 GB of zeros, an unknown site name): it is refused before anything under `/srv/sites` changes
   → Task 2 tests each case.
2. **A deploy dies halfway** (network drop mid-upload): `current` still points at the old release and the
   site keeps serving → Task 2 test (truncated stream) and Task 4 (switch only after a complete unpack).
3. **The IP, port or a key slips into the public repo**: gitleaks in CI plus a test that greps the tree for
   an IPv4 literal outside `local.env.example`'s placeholders → Task 1.
4. **A deep link `/math` or `/sweet`, and a missing page**: 200 with the right file, 404 for `/nope`, no
   directory listing → Task 3's nginx check runs these against the real `radar-site` contents.
5. **isoca.space visitors with a path** (`/foo`) land on `https://isabellybaldo.github.io/foo`, over HTTPS,
   with a 302 → Task 3 test of the redirect rule and Task 7's curl checks.

---

### Task 1: Repo skeleton, README, CI

**Files (isoca-infra):** `README.md`, `LICENSE` (MIT), `.gitattributes`, `.gitignore` (`local.env`, `*.pem`, `id_*`),
`local.env.example`, `.github/workflows/ci.yml`, `tests/no-secrets.sh`

**Interfaces:** Produces the repo layout later tasks fill: `box/`, `edge/`, `runbooks/`, `tests/`,
`.github/workflows/`.

- `local.env.example`: `VPS_IP=203.0.113.10` (documentation range), `SSH_PORT=2222`, `ADMIN=you`,
  `DEPLOY_KEY_PATH=~/.ssh/isoca_deploy`, `CLAUDE_KEY_PATH=~/.ssh/isoca_claude`, each with a one-line comment.
- README: what this repo is (one paragraph), an ASCII drawing (GitHub Actions → SSH forced command →
  `/srv/sites/<site>/current` ← nginx ← Traefik ← visitors; isoca.space → 302 → github.io), the directory
  map, "what is not in this repo and why" (the IP, port, keys), links to both runbooks and to cute-trading.
  Written for a reader judging the owner's work: plain, short, no marketing.
- `.gitattributes`: `* text=auto eol=lf`.
- `tests/no-secrets.sh`: fails if any tracked file except `local.env.example` contains an IPv4 literal
  other than `127.0.0.1`, `0.0.0.0` or 203.0.113.0/24, or a line matching
  `BEGIN (OPENSSH|RSA|EC) PRIVATE KEY`, or a 64-hex fingerprint after `SHA256:`.
- `ci.yml` (push, pull_request): gitleaks (pinned action), `shellcheck` on `box/*` and `tests/*.sh`,
  `tests/no-secrets.sh`, then the later tasks' test scripts (`tests/*.test.sh`, run in order).

- [ ] **Step 1:** Write `tests/no-secrets.sh` and a fixture run: a temp file with `10.1.2.3` → exit 1; the
  clean tree → exit 0.
- [ ] **Step 2:** Write README, LICENSE, `.gitignore`, `local.env.example`, `ci.yml`.
- [ ] **Step 3:** `bash tests/no-secrets.sh && shellcheck tests/*.sh` → exit 0.
- [ ] **Step 4:** Commit on `main` (first commit): `isoca-infra: the skeleton: what this repo runs, what it never holds, and CI that checks it`.

### Task 2: `site-deploy`, the forced command

**Files:** `box/site-deploy` (bash), `tests/site-deploy.test.sh`

**Interfaces:**
- Called by sshd with `SSH_ORIGINAL_COMMAND` set; env overrides for tests only: `SITES_ROOT` (default
  `/srv/sites`), `EDGE_DYNAMIC` (default `/srv/edge/dynamic`), `EDGE_INCOMING` (default `/srv/edge/.incoming`), `MAX_BYTES` (default 209715200), `KEEP`
  (default 5).
- Commands:
  - `deploy <site> <release>`: stdin is a tar.gz. `<site>` matches `^[a-z0-9-]{1,32}$` and
    `$SITES_ROOT/<site>` exists; `<release>` matches `^[0-9]{8}T[0-9]{6}Z-[0-9a-f]{7,40}$`. Reads at most
    `MAX_BYTES` (more → refuse). Lists the archive first and refuses any entry that is not a regular file
    or directory, any absolute path, any `..` component (a leading `./` and the bare `./` root entry are
    fine: Task 4 builds archives with `tar -C dir .`). `MAX_BYTES` caps the **uncompressed** size too (check
    while decompressing, before unpacking: a small gzip of 2 GB of zeros is refused). Names are listed
    with `--quoting-style=escape`, so a newline in a name can't spoof a listing line. Unpacks into `releases/<release>.tmp`, renames to
    `releases/<release>`, then points `current` at it atomically with a relative link
    (`ln -sfn releases/<release> current.new && mv -T current.new current`). Deletes all but the newest
    `KEEP` releases (never the one `current` names). Prints `deployed <site> <release>`.
  - `rollback <site>`: points `current` at the newest release older than the current one; prints
    `rolled back <site> to <release>`; refuses if there is none.
  - `edge-config`: stdin is a tar.gz of `*.yml` files only (no directories); same entry checks; replaces
    the contents of `EDGE_DYNAMIC` atomically (unpack under `EDGE_INCOMING`, then swap files); prints
    `edge config <n> files`.
  - Anything else: `refused: <reason>` on stderr, exit 2. Every refusal leaves the filesystem unchanged.
  - Unpacked files end up 0644 and directories 0755 (`--no-same-owner --no-same-permissions`, umask 022):
    nginx (uid 101) and Traefik (uid 65532) read them as "other". Modes are normalised explicitly after
    unpacking (`chmod`), because `--no-same-permissions` still honours a tar entry's 000.

- [ ] **Step 1: Write `tests/site-deploy.test.sh`** (plain bash, temp dirs, no root): deploys twice and
  checks `current` → second, then rollback → first; refuses: unknown site, bad site name, bad release id,
  an archive with a symlink, one with `../x`, one with an absolute path, one over `MAX_BYTES`, a small gzip that decompresses past `MAX_BYTES`, a hard link, a
  truncated archive, `edge-config` with a subdirectory, an unknown command — each with `current` and
  the release list unchanged; keeps exactly `KEEP` releases after `KEEP+2` deploys; `current` is a
  relative link.
- [ ] **Step 2:** `bash tests/site-deploy.test.sh` → FAIL (no script).
- [ ] **Step 3:** Implement `box/site-deploy` (`set -euo pipefail`, GNU tar listing with `tar -tvzf`).
- [ ] **Step 4:** `bash tests/site-deploy.test.sh && shellcheck box/site-deploy` → all pass.
- [ ] **Step 5:** Commit: `isoca-infra: site-deploy: the only thing the deploy key can run: checked archives, atomic releases, rollback`.

### Task 3: The edge: Traefik and nginx

**Files:** `edge/compose.yml`, `edge/traefik.yml` (static), `edge/dynamic/radar.yml`,
`edge/dynamic/isoca.yml`, `edge/dynamic/common.yml` (middlewares), `edge/nginx/sites.conf`,
`tests/edge.test.sh`

**Interfaces:**
- compose project `edge`: services `traefik` (ports `80:8000`, `443:8443`; mounts `traefik.yml` ro,
  `/srv/edge/dynamic` ro, `/srv/edge/acme` rw for `acme.json`: a bind mount owned by 65532, mode 0700, since a
  named volume would be root-owned) and `nginx` (mounts `sites.conf` ro,
  `/srv/sites` ro, tmpfs `/tmp`); networks `public` (traefik only, has egress for ACME) and `edge` (`internal: true`, traefik ↔ nginx).
- Routers: `radar` and `radar-test` (`Host(radar.isoca.space)`, `Host(new.isoca.space)`) → service
  `nginx` (`http://nginx:8080`) with middlewares `sec@file`, `csp-radar@file`, `rl@file`, `inflight@file`;
  `isoca` (`Host(isoca.space) || Host(www.isoca.space)`) → `noop@internal` with a `redirectRegex`
  `^https?://(?:www\.)?isoca\.space/(.*)` → `https://isabellybaldo.github.io/${1}` (`permanent: false`);
  an `http` catch-all redirecting to https.
- nginx `sites.conf`: one `server` for `radar.isoca.space new.isoca.space` on 8080 per the Global
  Constraints (root, `try_files`, caching, gzip, `autoindex off`); a default server returning 444.

- [ ] **Step 1: Write `tests/edge.test.sh`** (needs Docker; CI has it):
  - `docker compose -f edge/compose.yml config -q`;
  - the config text contains none of `alias`, `ssi`, `aio`, `http3`, `quic`, `ssl_`, `/var/run/docker.sock`, matched as
    nginx directives or whole words (`permissions` contains "ssi");
  - nginx: run the pinned image with `sites.conf` and a fixture `/srv/sites/radar/current` built from a
    small copy of the `radar-site` layout (index.html, math.html, sweet.html, site-assets/a.js,
    frozen/public.json); `nginx -t`; then curl with `Host: radar.isoca.space`: `/` → 200 index, `/math`
    → 200 math.html, `/sweet` → 200, `/site-assets/a.js` → 200 with `immutable`, `/nope` → 404,
    `/site-assets/` → 404 (no listing), `Server` header has no version;
  - traefik: the static and dynamic files pass `traefik` config parsing (run the pinned image with
    `--configFile` and a dry start that exits on error, or validate YAML against expected keys if no
    dry-run exists — say which in the report); the redirect regex maps `https://www.isoca.space/foo?x=1`
    to `https://isabellybaldo.github.io/foo?x=1` (test the regex with a small script).
- [ ] **Step 2:** Run it → FAIL.
- [ ] **Step 3:** Write the files; resolve and pin the two image digests (`docker buildx imagetools inspect`).
- [ ] **Step 4:** `bash tests/edge.test.sh` → pass.
- [ ] **Step 5:** Commit: `isoca-infra: the edge: unprivileged Traefik in front of a read-only nginx, radar.isoca.space, isoca.space to github.io`.

### Task 4: The deploy workflows

**Files:** `.github/workflows/deploy-static.yml` (reusable), `.github/workflows/rollback.yml`,
`.github/workflows/edge-config.yml`, `tests/workflows.test.sh`

**Interfaces:**
- `deploy-static.yml`: `on: workflow_call` with inputs `site` (string), `artifact` (string), `url`
  (string). Its job sets `environment: production` and reads `DEPLOY_SSH_KEY`, `DEPLOY_HOST`, `DEPLOY_PORT`,
  `DEPLOY_KNOWN_HOSTS` from that environment. GitHub doesn't allow `environment` on the job that calls a
  reusable workflow, and environment secrets don't pass through `secrets: inherit`, so the four are **not**
  declared under `on.workflow_call.secrets`; the first step fails with a clear message if any is empty.
  Steps: download the artifact (same run), tar.gz it, write the key and `known_hosts` (no
  `StrictHostKeyChecking=no` ever), `ssh deploy@$HOST -p $PORT "deploy <site> <UTC stamp>-<sha7>" < site.tgz`,
  expect `deployed`; then `curl -fsS -o /dev/null -w '%{http_code}' <url>` → 200 (retry 3× over 30 s).
- `rollback.yml`: `workflow_dispatch` with input `site`; environment `production`; runs `rollback <site>`.
- `edge-config.yml`: on push to `main` touching `edge/dynamic/**`, and dispatch; environment
  `production`; tars `edge/dynamic/*.yml` and runs `edge-config`.
- cute-trading calls `deploy-static.yml` (no `environment` on the calling job); the called job's
  `environment: production` resolves in the caller's repo, so isoca-radar's `production` environment holds
  the four secrets; isoca-infra's `production` environment holds the same four for its own two workflows.

- [ ] **Step 1: Write `tests/workflows.test.sh`** (text checks, like cute-trading's `workflow.test.ts`):
  every `uses:` pinned to 40-hex with a tag comment; no `StrictHostKeyChecking`; `permissions:` present
  and without `write` except `contents: read`; `rollback`, `edge-config` and `deploy-static`'s job use `environment: production`;
  `deploy-static.yml` declares no `on.workflow_call.secrets` and checks the four secrets are set.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Write the workflows. **Step 4:** Run → pass; also parse each
  YAML (python `yaml.safe_load` or `actionlint` if installable).
- [ ] **Step 5:** Commit: `isoca-infra: deploy, rollback and edge-config workflows over the site-deploy key`.

### Task 5: Runbook 1, the box

**Files:** `runbooks/01-box-setup.md`

Steps the owner runs (each command → expected → if not; STOP where marked):
0. **Before anything public.** Hostinger panel: the new box's IP differs from the old box's (the old IP and
   SSH port are in isoca-radar's git history). **STOP** if it is the same. Delete the old box (the backup is
   verified). `docker run` the pinned gitleaks image over a full clone of isoca-radar → no findings, or review
   each one. Then make `isoca-radar` public (needed for step 16: on GitHub Free, environment secrets and
   branch limits exist only in public repos).
1. PC (admin PowerShell): enable and start `ssh-agent`, `ssh-add` the owner key (one passphrase prompt).
2. PC: create `local.env` from the example with the real IP, chosen port, admin name.
3. PC: generate `isoca_deploy` and `isoca_claude` ed25519 keys (no passphrase on `isoca_deploy`: it lives
   only in GitHub secrets after step 16; delete the local copy then).
4. VPS as root (Hostinger's initial access): OS and version check; `apt update && apt full-upgrade`.
5. Admin user with sudo and the owner's public key; test login in a second window before going on. **STOP**
6. sshd: `Port $SSH_PORT`, `PasswordAuthentication no`, `PermitRootLogin no`, `KbdInteractiveAuthentication no`;
   `sshd -t`; restart; log in on the new port in a second window. **STOP**
7. ufw (SSH port, 80, 443), enable; check Hostinger's panel firewall matches.
8. fail2ban (sshd jail on the new port), unattended-upgrades; verify both active.
9. Docker Engine from Docker's apt repo; `/etc/docker/daemon.json` log limits; `docker run --rm hello-world`.
10. Users `deploy` and `claude`; `claude` into `systemd-journal`; neither in `sudo` or `docker` (check `id`).
11. `/srv/sites/radar` and `/srv/sites/radar/releases`, `/srv/edge/dynamic`, `/srv/edge/.incoming` owned by
    `deploy`; `/srv/sites` and `/srv/edge` root-owned and world-readable; `/srv/edge/acme` owned by 65532, mode 0700.
12. `git clone https://github.com/isabellybaldo/isoca-infra /srv/isoca-infra`; install `box/site-deploy` to
    `/usr/local/bin` (root, 0755); copy `edge/dynamic/*.yml` into `/srv/edge/dynamic`.
13. `deploy`'s `authorized_keys`: `restrict,command="/usr/local/bin/site-deploy" <isoca_deploy.pub>`;
    `claude`'s: its key, no restriction.
14. From the PC: `ssh -i isoca_deploy deploy@… "bogus"` → `refused`; `ssh -i isoca_deploy deploy@… ls` →
    `refused` (no shell); `ssh -i isoca_claude claude@… id` → no sudo/docker groups.
15. `docker compose -f /srv/isoca-infra/edge/compose.yml up -d`; `docker compose ps`: nginx `healthy`, traefik `Up`;
    `curl -sI http://localhost` → 308/301 to https.
16. GitHub (owner, browser): environment `production` in `isoca-radar` and `isoca-infra`, each with
    `DEPLOY_SSH_KEY` (isoca_deploy private), `DEPLOY_HOST`, `DEPLOY_PORT`, `DEPLOY_KNOWN_HOSTS`
    (`ssh-keyscan -p $SSH_PORT $VPS_IP` output, checked against the box's own
    `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`); restrict `production` to `main`. Then delete the
    local `isoca_deploy` private key.

- [ ] **Step 1:** Write the runbook. **Step 2:** Self-check: `grep -c '^→ expected:'` = number of steps,
  `tests/no-secrets.sh` passes, every placeholder comes from `local.env`.
- [ ] **Step 3:** Commit: `runbook: set up the new box: owner-run, hardened, two restricted users, the edge up`.

### Task 6: cute-trading deploys itself

**Files (cute-trading):** `.github/workflows/portfolio.yml`, `web/src/portfolio/workflow.test.ts`

- Bump the four actions to the Node 24 majors (Global Constraints), pinned by SHA.
- New job `deploy`: `needs: build`; `if: github.ref == 'refs/heads/main' && github.event_name != 'pull_request'`;
  `permissions: contents: read`; no `environment` here (GitHub refuses it on a calling job; the called job
  sets it); no `secrets:` block is needed;
  `uses: isabellybaldo/isoca-infra/.github/workflows/deploy-static.yml@<isoca-infra commit sha>` with
  `site: radar`, `artifact: radar-site`, `url: ${{ vars.DEPLOY_URL || 'https://radar.isoca.space/' }}` (runbook 2
  points the smoke check at new.isoca.space with a repo variable until DNS moves, then deletes the variable).
  (The SHA is isoca-infra's final local `main` commit, which the owner pushes unchanged. Run this task after
  Tasks 1–5 and 7. The branch is left for the owner to push and merge after the box is up.)

- [ ] **Step 1:** Extend `workflow.test.ts`: the deploy job exists, `needs: build`, the `if` above,
  no `environment` on the job, `uses:` pinned to a 40-hex isoca-infra SHA, `site: radar`,
  `artifact: radar-site`; every action is on v7/v6 majors per the tag comments.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Edit the workflow. **Step 4:** `npx vitest run src/portfolio/workflow.test.ts` → pass.
- [ ] **Step 5:** Commit on a branch: `ci: actions on Node 24, and deploy the portfolio to radar.isoca.space from main`.

### Task 7: Runbook 2, first deploy and DNS cut-over

**Files (isoca-infra):** `runbooks/02-cut-over.md`

1. Hostinger DNS: export/screenshot the zone; lower TTL to 300 on `@`, `www`, `radar`; add
   `A new.isoca.space → $VPS_IP`. Wait one old TTL.
2. `dig +short new.isoca.space` → `$VPS_IP`. **STOP**
3. `gh variable set DEPLOY_URL -R isabellybaldo/isoca-radar -b https://new.isoca.space/`. Owner merges the Task 6 PR → the `portfolio` run on `main` deploys; its `deploy` job ends with
   `deployed radar …`.
4. `curl -sI https://new.isoca.space/` → 200, valid certificate (`curl -v` shows Let's Encrypt), headers
   (HSTS, CSP, nosniff); `/math` and `/sweet` 200; `/frozen/public.json` 200 JSON; `/nope` 404. **STOP**
5. Owner looks at `https://new.isoca.space/` in Chrome (desktop and 360 px): pages, WASM loads, console
   clean (CSP).
6. DNS: `@`, `www`, `radar` A records → `$VPS_IP`; delete records for the dropped tenants (humor-do-dia,
   webappexample, apiexample, whoami) after listing them.
7. `dig` all three → `$VPS_IP`; `dig AAAA` for all three → empty (delete any AAAA record: Docker's userland
   proxy would hide IPv6 clients' addresses and merge them into one rate-limit bucket);
   from the PC `curl -s https://radar.isoca.space/ -o /dev/null`, then on the box the Traefik access log's
   `ClientHost` is the PC's public address, not a Docker gateway; **STOP** if not; `curl -sI https://radar.isoca.space/` → 200 with a valid cert;
   `curl -sI https://isoca.space/foo` → 302 `location: https://isabellybaldo.github.io/foo`; same for
   `www`.
8. `gh variable delete DEPLOY_URL -R isabellybaldo/isoca-radar`. Rollback drill: run `rollback.yml` with `site: radar` → `rolled back`; site still 200; re-run the last
   `portfolio` deploy → `deployed`.
9. Remove `radar-test` (new.isoca.space) from `edge/dynamic/radar.yml` in a commit (owner pushes → the
   `edge-config` workflow applies it) and the DNS record; raise TTLs back to 3600.

- [ ] **Step 1:** Write it. **Step 2:** Self-check as in Task 5. **Step 3:** Commit: `runbook: first deploy on new.isoca.space, then DNS: radar to the new box, isoca.space to github.io`.

### Task 8: github.io and the GitHub profile

**Files:** `isabellybaldo.github.io/src/data/projects.ts`, `src/pages/index.astro`, `public/thumbs/isoca-radar.svg`;
profile repo `README.md`

- `index.astro`: turn the `Projects` section back on (import and render; it was commented out "until a
  project goes public"). Check the other entries read right when shown, and report any that link to private repos.
- New `projects` entry: slug `isoca-radar`, title `isoca-radar`, tracks `Arbitrage on Polymarket, on paper.`,
  blurb `A read-only lab that watched every order book for baskets priced under a dollar, and paper-traded them at six latencies. It never placed an order; it did teach me what a CPU limit is.`,
  tech `['Rust', 'TypeScript', 'WebAssembly']`, accent `lavender`, thumb `/thumbs/isoca-radar.svg`,
  href `https://radar.isoca.space`, `comingSoon: false`. The thumb is a pixel-art SVG in the style of
  the existing two (look at them; same size and palette).
- Profile README: one line under the intro, in its voice:
  `🛰️ [isoca-radar](https://radar.isoca.space) — a Polymarket arbitrage lab, frozen after it outgrew its server. The math still runs in your browser.`
  and, under "About the repos", a line pointing to `isoca-infra` as the public repo that runs it.
- Wording of both approved by the owner on 2026-10-31 with the plan.

- [ ] **Step 1:** `npm run build && npm run check` in github.io → passes with the new entry.
- [ ] **Step 2:** Commit in each repo (local only): `content: isoca-radar is live` / `readme: isoca-radar and isoca-infra`.
