# Phase 4 dashboard on the VPS (radar.isoca.space)

A private dashboard over the scanner's Postgres, behind a GitHub login (only `isabellybaldo`).
Read-only (api as `analyze_ro`), paper only, no GitHub secrets. Three containers join the existing
compose project: `api`, `web`, `oauth2-proxy` (profile `web`, file `compose.web.yml`).
Disk budget: 30 GB (`sh scripts/disk-budget.sh`). The dashboard adds about 0.1 GB of images.

## 0. Once: DNS

Hostinger hPanel → Domains → isoca.space → DNS records → add **A**, name `radar`, points to
`31.97.23.123`, TTL 300. Before deploying, check from the PC (PowerShell):
```powershell
Resolve-DnsName radar.isoca.space -Server 8.8.8.8
```
Expect `31.97.23.123`. ACME (HTTP-01) needs this before the first start.

## 1. Once: the GitHub OAuth App

github.com → Settings → Developer settings → OAuth Apps → **New OAuth App**:
- Application name: `cute-trading radar`
- Homepage URL: `https://radar.isoca.space`
- Authorization callback URL: `https://radar.isoca.space/oauth2/callback`

Register, copy the **Client ID**, then **Generate a new client secret** and copy it (GitHub shows it once).
Both go only into the VPS `.env` (step 2): never into the repo, never into GitHub secrets.

## 2. Once: VPS `.env` keys and Traefik entrypoints (on the VPS)

```bash
cd /home/isoca/projects/cute-trading
cp .env .env.bak
openssl rand -base64 32 | tr -- '+/' '-_'        # the cookie secret
nano .env
```
Append (values from step 1 and the line above):
```
RADAR_DOMAIN=radar.isoca.space
OAUTH2_PROXY_CLIENT_ID=...
OAUTH2_PROXY_CLIENT_SECRET=...
OAUTH2_PROXY_COOKIE_SECRET=...
```
```bash
chmod 600 .env
```

Create the dashboard's own network and join Traefik to it. **`compose.web.yml` intentionally does not
put `api`, `web` or `oauth2-proxy` on Traefik's shared `traefik_public` network** — a fix-round security
finding: any other site's container on the shared network could otherwise call `http://api:8081`
without logging in, or another compose project could claim the `ct-oauth2-proxy` alias and answer
`200` to every forward-auth check, bypassing the login entirely. `ct_radar` is a dedicated network
only Traefik and this project's three dashboard containers join.

```bash
docker network create ct_radar
# find Traefik's container name (this only lists names, never env/secrets):
docker ps --format '{{.Names}}' | grep -i traefik
docker network connect ct_radar <traefik-container-name>
docker inspect "$(docker ps --format '{{.Names}}' | grep -i traefik | head -1)" --format '{{json .Args}}' | tr ',' '\n' | grep -i entrypoints
```
The `docker network connect` is a once-only, imperative step: it only needs repeating if Traefik's
own container is later recreated (a Traefik image update, a `docker compose up` on Traefik's own
project, etc.), since a fresh container starts with only the networks its own compose file lists.
**The durable fix** is to add `ct_radar` (as `external: true`) to Traefik's own compose file's
`networks:` and to its service's `networks:` list, so every future recreate keeps it attached without
a manual reconnect.

`compose.web.yml` assumes the entrypoints are named `web` (:80) and `websecure` (:443). If Traefik
uses other names, edit the `entrypoints=` labels before step 4. If Traefik already redirects
http→https globally, the `ct-http` router is harmless.

## 3. Build, pin and ship (on the PC)

Pin the three base images once (then commit the pins: "deploy: pin web image digests"):
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading
for i in node:22-alpine nginxinc/nginx-unprivileged:1.29-alpine quay.io/oauth2-proxy/oauth2-proxy:v7.12.0; do
  docker pull -q "$i" && docker inspect --format '{{index .RepoDigests 0}}' "$i"
done
```
Put each `name@sha256:…` in place of its tag: the two `FROM` lines of `docker/web.Dockerfile` and the
`oauth2-proxy` `image:` in `compose.web.yml` (keep the tag too: `name:tag@sha256:…`).

Build and bundle (Git Bash):
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading
sh scripts/disk-budget.sh
docker compose build --build-arg GIT_SHA="$(git rev-parse --short HEAD)" migrate    # cute-trading/rust:dev (scanner + api)
docker compose -f compose.yml -f compose.web.yml --profile web build web             # cute-trading/web:dev
mkdir -p "$HOME/ct-web" && cd "$HOME/ct-web"
docker save cute-trading/rust:dev cute-trading/web:dev | gzip -1 > image.tar.gz
cp /c/Users/Isabelly/PycharmProjects/cute-trading/compose.yml /c/Users/Isabelly/PycharmProjects/cute-trading/compose.web.yml .
tar -cf "$HOME/ct-web.tar" image.tar.gz compose.yml compose.web.yml && rm -rf "$HOME/ct-web"
docker builder prune -f --keep-storage 5GB
sh /c/Users/Isabelly/PycharmProjects/cute-trading/scripts/disk-budget.sh
```
Ship (PowerShell, the key has a passphrase):
```powershell
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" $HOME\ct-web.tar root@31.97.23.123:/root/
```

## 4. Start the dashboard (on the VPS)

This starts only the three dashboard containers; the running scanner keeps its current container
(a new `cute-trading/rust:dev` is only picked up by the scanner at its next recreate).
```bash
cd /home/isoca/projects/cute-trading
tar -xf /root/ct-web.tar && rm /root/ct-web.tar
gunzip -c image.tar.gz | docker load && rm image.tar.gz
docker compose -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok
docker compose -f compose.yml -f compose.web.yml --profile web pull oauth2-proxy
docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web oauth2-proxy
docker compose -f compose.yml -f compose.web.yml ps
docker compose -f compose.yml -f compose.web.yml logs --tail 20 api web oauth2-proxy
```
**Never drop `-q` from that `config` line.** Without it, `docker compose config` prints the fully
resolved compose file to stdout, including every real value substituted from `.env` (database
passwords, the OAuth client secret, the cookie secret). `-q` only validates and exits 0/1.

Expect `api: listening on 0.0.0.0:8081 (read-only, paper only)`. If the cert doesn't issue (Traefik
serves "TRAEFIK DEFAULT CERT" instead of a real one), that's Traefik's own ACME (HTTP-01) challenge,
not anything `web` does — restarting `web` does not make Traefik retry it. Instead: confirm DNS has
propagated (§0), wait a few minutes (Let's Encrypt HTTP-01 can take a little time after DNS updates),
and check Traefik's own logs for the ACME attempt and its error:
```bash
docker logs --tail 100 "$(docker ps --format '{{.Names}}' | grep -i traefik | head -1)" 2>&1 | grep -iE 'acme|certificate'
```
If it's still stuck after that, restart Traefik's own container (not `web`) so it retries the
challenge from scratch.

## 5. Post-deploy smoke test

From the PC (Git Bash), without a session:
```bash
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/api/health/now
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/api/stream
curl -sSI http://radar.isoca.space/ | head -1
curl -sSI https://radar.isoca.space/oauth2/sign_in | grep -iE 'strict-transport|x-frame-options|x-content-type-options|referrer-policy'
```
Pass: the first three are `302` to `https://github.com/login/oauth/authorize?…` (or to `/oauth2/…`, which
then goes to GitHub); never `200`. The fourth is `301`/`308`. The last shows all four headers.

With a session: open `https://radar.isoca.space/` in the browser and sign in with GitHub (authorize the
app once). Then:
- `https://radar.isoca.space/api/health/now` shows JSON; `https://radar.isoca.space/api/episodes/open`
  shows money as strings (`"net":"0.41"`).
- DevTools → Console: no CSP violations; Network: `/api/stream` is `200 text/event-stream` and the header
  dot says LIVE.
- DevTools → Application → Cookies: `_ct_radar` is `Secure`, `HttpOnly`, `SameSite=Lax`.
- A private window signed in to a different GitHub account gets a 403 from oauth2-proxy.
On the VPS:
```bash
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}'
docker compose -f compose.yml -f compose.web.yml port postgres 5432; echo "exit=$?"
docker compose -f compose.yml -f compose.web.yml exec postgres psql -U ct_owner -d cute_trading -tAc "select count(*) from pg_stat_activity where usename = 'analyze_ro'"
```
Pass: api under 128 MiB, web under 32 MiB, oauth2-proxy under 64 MiB, the scanner's CPU as before; no
published Postgres port; `analyze_ro` holds at most 4 connections (one of them the shared LISTEN).

## 6. Phase 4 exit (after a week of capture)

Behind the login: Radar live; Episodes filtered day by day back 7 days, paging to the end of each day;
one detail per kind; Paper (empty state until S3c is live, then curves per scenario); Health with 7 D.
Record it below.

## Rollback

```bash
docker compose -f compose.yml -f compose.web.yml --profile web stop api web oauth2-proxy
```
The scanner and Postgres are untouched. To also close the door: revoke the OAuth App's client secret on GitHub.

## Results

(date, image sha, smoke-test outputs, exit check)
