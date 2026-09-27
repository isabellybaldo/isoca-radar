# Phase 5a on the VPS: one combined deploy (S3c + capture fix + Phase 4 dashboard + live cards)

**Name:** the project is **isoca-radar**. Internal names keep `cute-trading` on purpose (compose
project, volumes, database `cute_trading`, image tags `cute-trading/*:dev`, the folders). See the
Phase 4 runbook's header.

This is the owner's option A (2026-09-27): **one** deploy after Phase 5a that ships everything the
VPS does not run yet, in one ordered pass. Read it through once before starting. Do the steps in order.

**What ships** (all from the branch head, one Rust image and one web image):
- The Rust image `cute-trading/rust:dev`, with three binaries, each picked by its entrypoint.
  - scanner: S3c paper trading, the capture housekeeping fix `cc51942`, run premises at start, the
    `host` premise after 10 minutes (`SCANNER_HOST`), and CLOB round trips in the health row.
  - api: the Phase 4 dashboard plus `/api/cards`.
  - analyze: the new `core` service.
- The web image `cute-trading/web:dev`: the Phase 4 dashboard plus the Papers page, the Edge
  capture box, IBM Plex Mono and the (?) help.
- Migrations 0004 (S3c paper tables), 0005 (paper state sha) and 0006 (cards, `analyze_rw`, the
  run 7 backfill).
- Compose: `analyze` in the `core` profile, `SCANNER_HOST`, and the dashboard (`compose.web.yml`,
  first time live).

**The VPS before this deploy:**
- Soak run 3 (`run_id` 7) is still running on image `2c5b84d`.
- The database has migrations 0001–0003 only. `2c5b84d` shipped `crates/store/migrations` 0001–0003.
- No dashboard is running.
- Two host trim loops (`find /data/capture …`) keep capture small, because `2c5b84d`'s housekeeping
  never ran in release.
- Phase 4 runbook §0–§2 are done: DNS, the GitHub OAuth App, the `.env` keys `RADAR_DOMAIN` and
  `OAUTH2_PROXY_*`, and the `ct_radar` network with Traefik connected. The entrypoints are verified
  as `web` / `websecure`.

**Rules, all the way through.** The VPS is public and shared with other sites.
- **Never print `.env`.** No `cat`, no `grep` that shows values, no `echo` of a password.
- **Never print a container's environment.** No bare `docker inspect <container>`, and no
  `docker inspect -f '{{.Config.Env}}'`: the `DATABASE_URL`s carry passwords. Use only the
  `-f` templates given here.
- **`docker compose config` only as `docker compose --env-file .env.example … config -q`.** Never
  drop the `-q`, and never run it without `--env-file .env.example`. Without them, compose prints the
  fully resolved file, with every real value from `.env` substituted in: the database passwords, the
  OAuth client secret and the cookie secret. With both, it only validates the placeholders and exits
  0 or 1. Never use `config --services` either.
- **Never restart Docker.** Other sites run on this host.
- **Disk budget.** 50 GB for the VPS project. `sh scripts/disk-budget.sh` checks against the
  stricter 30 GB by default; run `LIMIT_GB=50 sh scripts/disk-budget.sh` for the VPS allowance. On
  the PC the budget is 30 GB (`scripts/disk-budget.ps1`). A year of card cells is a few hundred MB.

**The order:**

| Where | Step |
| --- | --- |
| PC | §1 benchmark |
| PC | §2 pin digests |
| PC | §3 build |
| PC | §4 bundle and ship |
| VPS | §5 pre-flight and rollback image |
| VPS | §6 `.env` keys |
| VPS | §7 load |
| VPS | §8 stop the trim loops |
| VPS | §9 stop the scanner |
| VPS | §10 migrate 0004–0006 |
| VPS | §11 activate `analyze_rw` |
| VPS | §12 start |
| VPS and PC | §13 checks |

The `.env` keys (§6) come **before** the first `docker compose` command that uses the new
`compose.yml`. Compose interpolates every `${…:?}` in the file, whatever the profile, and `analyze`
requires `ANALYZE_RW_DB_PASSWORD`. So every later command (`stop`, `migrate`, `exec`) would refuse
to run without that key.

---

## 1. Benchmark (on the PC, Git Bash)

Debug build (spec §9). Write both totals in Results:
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p cards --test bench_tick -- --ignored --nocapture 2>&1 | grep -E 'tick total|test result'
```
Pass: `tick total at 30 days: N ms` and `… at 90 days: N ms`, then `test result: ok. 1 passed`.

The 30-day total must be under 200 ms. The 90-day `all` render may run 239–266 ms in debug (ruling:
it happens once per clock hour, and release is several times faster).

## 2. Pin the web base-image digests (on the PC, once)

This is Phase 4 runbook §3's pin step, not done yet:
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading
for i in node:22-alpine nginxinc/nginx-unprivileged:1.29-alpine quay.io/oauth2-proxy/oauth2-proxy:v7.12.0; do
  docker pull -q "$i" && docker inspect --format '{{index .RepoDigests 0}}' "$i"
done
```
Replace each tag with `name:tag@sha256:…` (keep the tag too) in two places:
- the two `FROM` lines of `docker/web.Dockerfile`;
- the `oauth2-proxy` `image:` in `compose.web.yml`.

Then check and commit:
```bash
docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok
git add docker/web.Dockerfile compose.web.yml && git commit -m "deploy: pin web image digests"
```
Commit **before** building: `GIT_SHA` (below) becomes the scanner's and analyze's `SCANNER_VERSION`.
Every run records it in `runs.version`.

## 3. Build (on the PC, Git Bash)

The PC `.env` must have the two Phase 5a keys, or compose refuses to parse `compose.yml`. The
command below prints a count, never a value. Expect `2`:
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading
grep -c -E '^(ANALYZE_RW_DB_PASSWORD|SCANNER_HOST)=' .env
```
If it prints less than `2`, add the keys without printing them:
```bash
(grep -q '^ANALYZE_RW_DB_PASSWORD=' .env || printf 'ANALYZE_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env) && (grep -q '^SCANNER_HOST=' .env || echo 'SCANNER_HOST=pc-dev' >> .env)
```

Check the disk, then build. `git status` must show no changes to tracked files: the image is the commit.
```bash
powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
git status --short --untracked-files=no
export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA"
docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate                        # cute-trading/rust:dev (scanner + api + analyze)
docker compose -f compose.yml -f compose.web.yml --profile web build web          # cute-trading/web:dev
docker run --rm --entrypoint /usr/local/bin/analyze cute-trading/rust:dev 2>&1 | head -1
```
The last line proves that `analyze` is in the image. With no `DATABASE_URL` it exits at once with
`analyze: DATABASE_URL is not set (the analyze_rw URL)`. Write `GIT_SHA` in Results.

## 4. Bundle, prune and ship (on the PC)

The bundle holds the images, the compose files, `.env.example` (for the `config -q` check), the roles
script (used on fresh volumes only) and the disk-budget script:
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading
mkdir -p "$HOME/ct-5a/docker/postgres/init" "$HOME/ct-5a/scripts" && cd "$HOME/ct-5a"
docker save cute-trading/rust:dev cute-trading/web:dev | gzip -1 > image.tar.gz
cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" .
cp "$R/docker/postgres/init/01-roles.sh" docker/postgres/init/
cp "$R/scripts/disk-budget.sh" scripts/
tar -cf "$HOME/ct-5a.tar" image.tar.gz compose.yml compose.web.yml .env.example docker scripts
cd "$HOME" && rm -rf "$HOME/ct-5a" && ls -l "$HOME/ct-5a.tar"
docker builder prune -f --keep-storage 5GB
powershell -ExecutionPolicy Bypass -File "$R/scripts/disk-budget.ps1"
```
The disk budget must pass after the prune. Ship the bundle from PowerShell (the key has a passphrase):
```powershell
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" $HOME\ct-5a.tar root@31.97.23.123:/root/
```
Once §7 has loaded it on the VPS, delete `$HOME\ct-5a.tar` on the PC.

---

## 5. Pre-flight: record, back up, keep the old image (on the VPS)

Everything here uses the **old** `compose.yml`, still in place. Nothing is stopped yet.
```bash
cd /home/isoca/projects/cute-trading
LIMIT_GB=50 sh scripts/disk-budget.sh
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT version, success FROM _sqlx_migrations ORDER BY version"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT run_id, started_ms, version, mode, exit_reason FROM runs WHERE run_id >= 7 AND service = 'scanner' AND mode = 'live' ORDER BY run_id"
```
Expect three things:
- migrations `1|t`, `2|t`, `3|t` and nothing above;
- one row, `7 | 1790384263621 | 2c5b84d | live |` with an empty `exit_reason` (still running);
- the disk within budget.

That row is the backfill list 0006 will take (approved 2026-09-26). Write it in Results. If more live
runs show up, stop and ask before migrating: 0006 backfills every live scanner run from 7 on.

Keep the running image under its own tag. `docker load` (§7) moves `cute-trading/rust:dev` to the
new image, and without this tag the old one would become an untagged `<none>`. Also keep the old
compose files:
```bash
OLD="$(docker inspect -f '{{.Image}}' "$(docker compose --profile core ps -q scanner)")" && docker tag "$OLD" cute-trading/rust:2c5b84d && unset OLD
docker image ls cute-trading/rust
mkdir -p /root/ct-rollback && cp compose.yml /root/ct-rollback/ && { [ ! -f compose.web.yml ] || cp compose.web.yml /root/ct-rollback/; }
cp .env .env.bak-5a && chmod 600 .env.bak-5a
```
Expect `cute-trading/rust:2c5b84d` and `cute-trading/rust:dev` on the same IMAGE ID.

## 6. The new `.env` keys (on the VPS; values never printed)

These are appends only. The running scanner never reads `.env` again, and the old `compose.yml`
does not use these keys. The password uses the same `openssl` pattern as Phase 4 §2. Hex only,
which §11 relies on.
```bash
cd /home/isoca/projects/cute-trading
grep -q '^SCANNER_HOST=' .env || echo 'SCANNER_HOST=vps-kvm2' >> .env
grep -q '^ANALYZE_RW_DB_PASSWORD=' .env || printf 'ANALYZE_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env
chmod 600 .env
grep -c -E '^(SCANNER_HOST=vps-kvm2|ANALYZE_RW_DB_PASSWORD=.+)$' .env
grep -c -E '^(POSTGRES_PASSWORD|SCANNER_DB_PASSWORD|ANALYZE_DB_PASSWORD|RADAR_DOMAIN|OAUTH2_PROXY_CLIENT_ID|OAUTH2_PROXY_CLIENT_SECRET|OAUTH2_PROXY_COOKIE_SECRET)=.+$' .env
```
Expect `2`, then `7`. These are counts; nothing prints a value.

If the first count is `1` and an old `SCANNER_HOST` line exists with another value, fix that line
with `nano .env`. `SCANNER_HOST` must be exactly `vps-kvm2`, because it becomes part of Edge
capture's premise key.

## 7. Load the bundle (on the VPS)

```bash
cd /home/isoca/projects/cute-trading
tar -xf /root/ct-5a.tar && rm /root/ct-5a.tar
gunzip -c image.tar.gz | docker load && rm image.tar.gz
docker image ls cute-trading/rust && docker image ls cute-trading/web
docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok
grep -n -A1 -E '^  analyze:$' compose.yml && grep -n -E 'SCANNER_HOST|ANALYZE_RW_DB_PASSWORD' compose.yml
LIMIT_GB=50 sh scripts/disk-budget.sh
```
**Never drop `-q` from that `config` line, and never run it without `--env-file .env.example`.**
(See Rules at the top.)

Expect three things:
- `cute-trading/rust:dev` now on a new IMAGE ID, with `:2c5b84d` still listed;
- `config-ok`;
- the `analyze` service and the two keys found in `compose.yml`.

The running scanner is untouched: a container keeps the image it was started from.

## 8. Stop the host trim loops (on the VPS)

The new scanner (`cc51942`) runs its own housekeeping from the first message: the 10 GB cap
(`max_capture_gb`), 48 h retention and the disk-pause guard. So the host loops go now. Preview
first, and check that every line is one of the trim loops (a `sh -c 'while …find /data/capture…'`
loop, or a `find` / `docker run … find /data/capture …` it started):
```bash
pgrep -af 'find /data/capture'
```
Then stop them and check again a few seconds later:
```bash
pkill -f 'find /data/capture'; sleep 3; pgrep -af 'find /data/capture' || echo trim-loops-gone
docker ps --format '{{.Names}} {{.Image}} {{.Command}}' | grep -F 'find /data/capture' || echo no-trim-containers
```
Expect `trim-loops-gone` and `no-trim-containers`. If a loop respawned, run the `pkill` once more.
Capture can grow for the few minutes until §12: about 2 GB an hour, well inside the budget.

## 9. Stop the scanner (on the VPS)

This ends run 7 cleanly (the stop grace is 30 s: it flushes, closes episodes and writes
`exit_reason`). From here until §12 nothing writes to the database, so 0006's index builds and its
one-time `UPDATE episodes` have no concurrent writer (T2 ruling).
```bash
cd /home/isoca/projects/cute-trading
docker compose --profile core stop scanner
docker compose --profile core ps -a scanner
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT run_id, ended_ms, exit_reason FROM runs WHERE run_id = 7"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM pg_stat_activity WHERE usename = 'scanner_rw'"
```
Expect the scanner `Exited`, run 7 with `ended_ms` and an `exit_reason`, and `0` scanner sessions.
Write run 7's end in Results.

Take a safety dump while nothing writes. It is a last resort only (see Rollback); the database is
small:
```bash
docker compose exec -T postgres pg_dump -U ct_owner -d cute_trading -Fc > /root/ct-pre-5a.dump && ls -lh /root/ct-pre-5a.dump
```

## 10. Migrate 0004–0006 (on the VPS)

The new `compose.yml` adds `ANALYZE_RW_DB_PASSWORD` to Postgres's environment. It is only read by
`01-roles.sh` on an empty volume, so this data volume ignores it. But it changes the service's
config, so the next `up` without `--no-deps` would **recreate Postgres**. Do that on purpose now,
while nothing is connected, and wait for it to be healthy (60 s to shut down cleanly):
```bash
cd /home/isoca/projects/cute-trading
docker compose up -d --no-build --timeout 60 postgres
until [ "$(docker inspect -f '{{.State.Health.Status}}' "$(docker compose ps -q postgres)")" = healthy ]; do sleep 2; done; echo pg-healthy
docker compose --profile core run --rm migrate; echo "migrate exit=$?"
```
Expect `pg-healthy`, then `migrate exit=0`. Check what was applied:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT version, success FROM _sqlx_migrations ORDER BY version"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_regclass('paper_state') IS NOT NULL, to_regclass('run_premises') IS NOT NULL, to_regclass('card_results') IS NOT NULL"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, count(*) FROM run_premises GROUP BY 1 ORDER BY 1"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'analyze_rw'"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FILTER (WHERE updated_ms = 0) FROM episodes"
```
Expect five things:
- `1|t` through `6|t`: 0004, 0005 and 0006 are applied and successful;
- `t|t|t`;
- `7|6`: run 7 with the six Gebele areas, as approved in §5;
- `f`: `analyze_rw` exists but cannot log in yet;
- `0`: every episode has its `updated_ms`.

If `migrate` fails, stop here and read its log (`docker compose --profile core logs --tail 50
migrate`). 0006 runs in one transaction, so a failure leaves nothing half-applied. See Rollback.

## 11. Activate `analyze_rw` once (on the VPS; the password is never printed)

Migration 0006 creates `analyze_rw` NOLOGIN, because a migration can't carry a secret. This gives it
the password from `.env`. The shell reads the password into a variable. The bash builtin `printf`
feeds it to psql on stdin as a psql variable, so it never appears on a command line, in `ps` or on
screen. `:'pw'` quotes it as a literal.
```bash
cd /home/isoca/projects/cute-trading
PW="$(grep -E '^ANALYZE_RW_DB_PASSWORD=' .env | cut -d= -f2-)"
case "$PW" in ''|*[!0-9a-f]*) echo "pw-bad: not an openssl rand -hex value; stop"; PW= ;; *) echo pw-read ;; esac
[ -n "$PW" ] && printf '\\set pw %s\nALTER ROLE analyze_rw LOGIN PASSWORD :'"'"'pw'"'"';\n' "$PW" | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -q -U ct_owner -d cute_trading && echo role-set
unset PW
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'analyze_rw'"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT setconfig FROM pg_db_role_setting s JOIN pg_roles r ON r.oid = s.setrole WHERE r.rolname = 'analyze_rw'"
```
Expect `pw-read`, `role-set`, `t`, then `{statement_timeout=10s}`.

Do not run this with `-e`, `-a` or `\echo`; any of those would show the statement. If `pw-bad`
appears, the `.env` line was not written by §6. Replace it with an `openssl rand -hex 24` value
(never printed) and run this step again.

## 12. Start: the scanner, analyze and the dashboard (on the VPS)

`--no-deps`: Postgres is already up on its new config and `migrate` has already run. Without it,
compose would run `migrate` again (a harmless no-op) and could touch Postgres. Only `compose.yml`
starts the `core` services; the dashboard needs both files.
```bash
cd /home/isoca/projects/cute-trading
docker compose --profile core up -d --no-build --no-deps scanner analyze
docker compose -f compose.yml -f compose.web.yml --profile web pull oauth2-proxy
docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web oauth2-proxy
docker compose -f compose.yml -f compose.web.yml --profile core --profile web ps
```
All five are `running`, and Postgres is `healthy`. Then read the logs:
```bash
docker compose --profile core logs --tail 30 scanner
docker compose --profile core logs --tail 20 analyze
docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 20 api web oauth2-proxy
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, version, started_ms FROM runs WHERE service = 'scanner' AND mode = 'live' ORDER BY run_id DESC LIMIT 1"
```
Expect these log lines:
- **scanner:** `paper: policies changed (saved none != current <sha>); all accounts start fresh`.
  This is expected: it is the first S3c start, with no saved state. No `SCANNER_HOST must be …`
  error.
- **analyze:** `analyze: every 900 s (writes card tables only, paper only)`, then within about a
  minute one `analyze: tick … ms, … dirty hours, … catch-up hours, 16 results` line. That is G1–G4
  × 24h/7d/30d/all, from run 7's backfilled premises. EDGE has no key until the new run's `host` row
  (ruling F2).
- **api:** `api: listening on 0.0.0.0:8081 (read-only, paper only)`.

The last query shows the new live run. Its `version` is `GIT_SHA` from §3. Write it down as `NEW`.

If Traefik serves "TRAEFIK DEFAULT CERT", see Phase 4 runbook §4 (Traefik's own ACME challenge;
restarting `web` does not help).

---

## 13. Post-deploy checks

### 13.1 Dashboard smoke test (Phase 4 §5)

From the PC (Git Bash), without a session:
```bash
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/api/health/now
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/api/stream
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' 'https://radar.isoca.space/api/cards?win=7d'
curl -sSI http://radar.isoca.space/ | head -1
curl -sSI https://radar.isoca.space/oauth2/sign_in | grep -iE 'strict-transport|x-frame-options|x-content-type-options|referrer-policy|content-security-policy'
```
Pass:
- The first four are `302` to `https://github.com/login/oauth/authorize?…` (or to `/oauth2/…`,
  which then goes to GitHub). Never `200`.
- The fifth is `301` or `308`.
- The last shows all five headers.

With a session: open `https://radar.isoca.space/` and sign in with GitHub. Then check:
- `/api/health/now` is JSON; `/api/cards/meta` shows `tick` and `last_error: null`.
- DevTools → Console: no CSP violations. The fonts come from fonts.googleapis.com and
  fonts.gstatic.com.
- Network: `/api/stream` is `200 text/event-stream`, and the header dot says LIVE.
- DevTools → Application → Cookies: `_ct_radar` is `Secure`, `HttpOnly`, `SameSite=Lax`.
- A private window signed in as a different GitHub account gets `403`.

### 13.2 No published ports, container limits, networks (on the VPS)

```bash
cd /home/isoca/projects/cute-trading
docker ps --format '{{.Names}} {{.Ports}}' | grep '^cute-trading-' | grep -E '0\.0\.0\.0:|\[::\]:|:::' || echo no-published-ports
docker compose -f compose.yml -f compose.web.yml port postgres 5432; echo "exit=$?"
for s in scanner analyze api web oauth2-proxy; do
  printf '%-13s ' "$s"
  docker inspect -f 'user={{.Config.User}} ro={{.HostConfig.ReadonlyRootfs}} cpus={{.HostConfig.NanoCpus}} mem={{.HostConfig.Memory}} capdrop={{.HostConfig.CapDrop}} sec={{.HostConfig.SecurityOpt}} restart={{.HostConfig.RestartPolicy.Name}}:{{.HostConfig.RestartPolicy.MaximumRetryCount}} nets={{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' "cute-trading-$s-1"
done
```
Pass: `no-published-ports`, and `port postgres` prints nothing with a non-zero exit. The inspect
lines should read as follows (`user=` empty means the image's own user):

| Service | Expected |
| --- | --- |
| analyze | `user=65532:65532 ro=true cpus=250000000 mem=134217728 capdrop=[ALL] sec=[no-new-privileges:true] restart=on-failure:5 nets=cute-trading_internal` (only the internal network: never `ct_radar`, never `traefik_public`) |
| api | `cpus=250000000 mem=134217728`, nets `cute-trading_internal ct_radar` |
| web | `cpus=100000000 mem=33554432`, nets `ct_radar` |
| oauth2-proxy | `cpus=100000000 mem=67108864`, nets `ct_radar` |
| scanner | `restart=on-failure:5`, nets `cute-trading_egress cute-trading_internal` |

Check the database sessions per role:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT usename, count(*) FROM pg_stat_activity WHERE usename IN ('analyze_ro', 'analyze_rw', 'scanner_rw') GROUP BY 1 ORDER BY 1"
```
Pass: `analyze_ro` tops out around 4 (the api, one of them the shared LISTEN), and `analyze_rw` at
2 or fewer (analyze's pool).

### 13.3 The new run wrote its premises; the host row after about 10 minutes

```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT area, version FROM run_premises WHERE run_id = <NEW> ORDER BY area"
```
Right after the start: seven rows. They are the six Gebele areas (`censoring`, `depth`,
`detector_no`, `detector_yes`, `episode_def`, `fees`), each with the same version as run 7's, plus
`sim`.

About 10 health minutes later, run the same query again. An eighth row, `host`, reads
`1:vps-kvm2|<band>`, where band is `lt25`, `25-100`, `100-250`, `gt250` or `unmeasured`. Write the
band in Results. `unmeasured` means no minute had both a lag and a REST round trip; check
`SELECT count(*) FROM health WHERE run_id = <NEW> AND rest_rtt_p50_ms IS NOT NULL`.

### 13.4 analyze: 16 results, then 21 (ruling F2)

```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM card_results"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT card, win, status FROM card_results ORDER BY 1, 2"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT key, value FROM card_meta WHERE key IN ('tick', 'last_error')"
docker compose --profile core logs --since 30m analyze | grep -E 'tick|failed|refus'
```
- **First tick:** `16`, which is G1–G4 × `24h`, `7d`, `30d`, `all`. `last_error` is `null`.
- **After the host row:** the first tick after 13.3's `host` row (up to 15 minutes after it) makes it
  `21`. That adds EDGE × `24h`, `7d`, `30d`, `all`, `today`. The new EDGE rows start as
  `under_review` or `warming`; that is expected for a new key.
- **Later tick lines** may report fewer results, because 30d/all re-render at most hourly. That is
  not a failure. The `card_results` count is the check.

`tick failed`, a refusal, or the container restarting (`docker compose --profile core ps analyze`)
is a failure. `restart: on-failure:5` stops after five tries; read the log, then see Rollback.

### 13.5 Paper trading is live (S3c, short)

After about 3 minutes, run these (the full check is `s3-paper.md` §3):
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM paper_accounts WHERE run_id = <NEW>"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) marks, count(*) FILTER (WHERE m.cash + (m.equity - m.cash) <> a.bankroll + m.realized + m.unrealized) broken FROM paper_marks m JOIN paper_accounts a USING (run_id, account_id) WHERE m.run_id = <NEW>"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT coalesce(sum(drops), 0) FROM health WHERE run_id = <NEW>"
docker compose --profile core logs --since 5m scanner | grep "engine: frames" | tail -3
```
Pass: `15` accounts, marks `> 0` with `broken = 0`, and `0` drops. Write the `on_frame_us` p50/p99
next to soak run 3's in Results.

### 13.6 analyze's CPU and memory after catch-up

Wait until an analyze tick line shows `0 catch-up hours`. Then:
```bash
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}' | grep '^cute-trading-'
```
Pass:
- analyze at or under 25 % CPU (its cap is 0.25 of one CPU), and under 128 MiB;
- api under 128 MiB, web under 32 MiB, oauth2-proxy under 64 MiB;
- the scanner's CPU and memory in soak run 3's range (about 1.5 GiB).

Run it twice, a minute apart. analyze idles between its 15-minute ticks.

### 13.7 The capture cap holds (`cc51942`)

With the trim loops gone, the scanner's own housekeeping must keep capture at or under the 10 GB cap
and 48 h retention. Check about 6 hours after the start (by then about 2 GB an hour has reached the
cap), and again at 24 hours:
```bash
docker run --rm -v cute-trading_data:/data alpine du -sh /data/capture
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT kind, count(*) FROM integrity_events WHERE run_id = <NEW> AND kind IN ('capture_budget_delete', 'capture_drop') GROUP BY 1"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT bool_or(capture_paused) FROM health WHERE run_id = <NEW>"
```
Pass:
- capture at or under about 10 GB, and not growing past it;
- `capture_budget_delete` rows once the cap is reached;
- no `capture_drop`;
- `paused` false.

If capture keeps growing past 10 GB, start a trim loop again (see Rollback) and report it.

### 13.8 Disk budget and cleanup

```bash
cd /home/isoca/projects/cute-trading
LIMIT_GB=50 sh scripts/disk-budget.sh
docker image ls cute-trading/rust && docker image ls cute-trading/web
```
Pass: within budget. The script counts `cute-trading/rust:dev` only; the web image (about 0.1 GB)
and the `:2c5b84d` rollback tag come on top.

After 24 hours with every check in §13 passed, delete the rollback material:
```bash
rm -f /root/ct-pre-5a.dump && docker image rm cute-trading/rust:2c5b84d && rm -rf /root/ct-rollback
```

### 13.9 In the browser (signed in)

- **Papers:** G1–G4, each with a status tag, three columns, n, and a short hash (the full hash on
  hover). Saguillo shows "coming later: …". The Not replicated box is there, and so is "updated n
  min ago".
- **Paper:** the Edge capture box at the top, with the decay chart and "measured: vps-kvm2 ≈ … ms".
  It fills once EDGE has results (13.4).
- **Radar:** "Edge on the table today: +$X · captured at 145 ms: Y%." in the today box, once
  today has data.
- **Help:** every section has a (?) that opens and closes by mouse and by keyboard (Tab to it,
  Enter, then Escape).
- **360 px:** DevTools → device toolbar → 360 × 740. On each page (Radar, Episodes, one episode
  detail, Paper, Papers, Health), run `document.documentElement.scrollWidth` in the console. It
  must print `360` (no horizontal page scroll); wide tables scroll inside their own box.

## 14. Exits

**Phase 4 exit** (after a week of capture): Phase 4 runbook §6.

**Phase 5a exit** (spec §1):
- On the VPS, the Papers page shows G1–G4.
- The Edge capture box shows live figures for 24h, 7d, 30d and all.
- Every card carries a status and a hash.
- analyze stays inside its CPU cap (the §1 benchmark and 13.6).

After the first 7 days, check the thin thresholds (30 and 10) against the real 7-day volumes
(spec §11) and note them in Results.

## Rollback

Choose by what failed. Migrations 0004–0006 are additive and stay: `2c5b84d`'s scanner writes
episodes and health without the new columns, which default or stay NULL. Its runs write no
`run_premises`, so no card ever counts them.

**Only the dashboard.** The scanner and analyze are untouched:
```bash
docker compose -f compose.yml -f compose.web.yml --profile web stop api web oauth2-proxy
```
To also close the door, revoke the OAuth App's client secret on GitHub.

**Only analyze.** The cards keep their last results, and the page shows their age:
```bash
docker compose --profile core stop analyze
```
To lock the role again as well:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "ALTER ROLE analyze_rw NOLOGIN"
```

**The scanner, back to `2c5b84d`.** Use `--no-deps`: the old image's `migrate` does not know
0004–0006 and would refuse to run, which would block the scanner through `depends_on`.
```bash
cd /home/isoca/projects/cute-trading
docker compose --profile core stop analyze scanner
docker tag cute-trading/rust:dev cute-trading/rust:5a      # keep the new image for a retry
docker tag cute-trading/rust:2c5b84d cute-trading/rust:dev
docker compose --profile core up -d --no-build --no-deps scanner
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, version FROM runs WHERE service = 'scanner' AND mode = 'live' ORDER BY run_id DESC LIMIT 1"
```
Expect the newest run on version `2c5b84d`. Then:
- **The dashboard:** `api`, `web` and `oauth2-proxy` keep running on the images they started
  from. Do not recreate `api` while `:dev` points at `2c5b84d`.
- **The trim loop:** `2c5b84d`'s capture housekeeping never runs in release, so start a trim loop
  again at once. Hour files older than 120 minutes:
  ```bash
  nohup sh -c 'while :; do docker run --rm -v cute-trading_data:/data alpine find /data/capture -name "*.jsonl.zst" -mmin +120 -delete; sleep 600; done' >/dev/null 2>&1 &
  pgrep -af 'find /data/capture'
  ```
- **The old compose files** are in `/root/ct-rollback/`. They are not needed to run the old image:
  the new `compose.yml` runs it too, and `SCANNER_HOST` is simply ignored by it.

**The database (last resort).** Only if 0004–0006 left it unusable, which should not happen: 0006
is one transaction. Stop every writer and reader, then restore the §9 dump over a fresh database
with `pg_restore`, and go back to `2c5b84d` as above. Ask before doing this: it discards everything
written since §9.

## Results

(Fill in:
- the date and `GIT_SHA`;
- the §1 benchmark totals at 30 and 90 days;
- the §5 backfill list and migrations before;
- run 7's end;
- the §10 migrate output, `_sqlx_migrations`, and the `run_premises` count;
- `NEW`, and its host band;
- the first tick line (16), and the tick that reached 21;
- analyze's `docker stats` after catch-up;
- `on_frame_us` p50/p99 against soak run 3;
- capture size at 6 h and at 24 h;
- the disk budget before and after;
- the smoke-test outputs;
- the 360 px check per page;
- the exit checks.)
