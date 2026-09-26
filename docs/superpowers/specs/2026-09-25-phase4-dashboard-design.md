# cute-trading Phase 4: the dashboard (S6 api + S7a web), design

**Date:** 2026-09-25. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§9, §10, §14, §16, §17, §19 Phase 4).
**Status:** approved in conversation, section by section. Mockups approved: `mockup-radar.html` (night shift) and `mockup-detail-paper.html`.

## 1. Purpose and success

A private, polished dashboard over the scanner's data, used in this order:
1. **Portfolio:** shown by the owner in person. It is always behind a login, and there is no public demo.
2. **Monitoring:** health and live opportunities while the scanner runs.
3. **Research:** episodes and paper-trading results.

The three purposes carry equal weight in the structure. Polish is a requirement, not a nice-to-have.

**Exit (Phase 4, adapted from §19):** a week of capture is browsable end to end on the VPS, behind the login.

**Change from the parent spec:** Phase A assumed everything runs locally. The data now lives on the VPS (scanner, Postgres and capture), so the dashboard runs there too. This brings part of Phase 8 forward: the api and web containers behind Traefik. The pull-based deploy and GHCR stay in Phase 8.

## 2. Architecture

```
 internet ──HTTPS──► Traefik ──► oauth2-proxy (GitHub login, only user isabellybaldo)
                        │  forward-auth on every request
                        ├──► web  (nginx, static React build)        radar.isoca.space/
                        └──► api  (Rust axum, read-only)             radar.isoca.space/api/*
                                   │ internal network only
                                   ▼
                               postgres (never published) ◄── scanner
```

- **Three new containers,** under a new compose profile `web` in the same compose project on the VPS: `api`, `web` and `oauth2-proxy`. They follow the existing Traefik deploy pattern (`traefik_public` network, `letsencrypt` resolver, http→https redirect).
- **Domain:** `radar.isoca.space`. It can be changed; it needs a Hostinger A record before the first deploy.
- **api:** a new binary, `bins/api` (Rust, `axum`). It reads Postgres through the `store` crate only; the §9.3 rule that only `store` writes SQL still holds. A new read-only query module in `store` serves it.
- **web:** `web/` (React + Vite + TypeScript), built to static files and served by a tiny nginx image.
- **Deploy:** the same bundle flow as the scanner (a scp'd image tarball plus compose files). There are no GitHub secrets. The pull-based deploy is Phase 8.
- **Future option, not built:** Tailscale-only access, which would take the dashboard off the public internet entirely.

## 3. Security (the dashboard must be safe on a public VPS)

- **Login:**
  - `oauth2-proxy` with a GitHub OAuth App, which the owner creates once. Allowed: GitHub user `isabellybaldo` only.
  - The session lives in a signed, `Secure`, `HttpOnly`, `SameSite=Lax` cookie.
  - The client secret and the cookie secret live **only** in the VPS `.env`, never in the repo or in GitHub secrets.
- **Everything is behind the login:** Traefik applies forward-auth to every router, including `/api/*` and `/api/stream`. No route answers unauthenticated except oauth2-proxy's own sign-in pages.
- **The API can't write:**
  - It connects as `analyze_ro` (SELECT only; statement timeout 60 s).
  - It exposes no write endpoints in Phase 4.
  - Every query is parameterized and bounded: page size ≤ 200, date ranges ≤ 7 days, series ≤ 7 days.
- **Network:**
  - Postgres is never published and stays on the internal network.
  - `api` joins `internal` + `traefik_public`; `web` and `oauth2-proxy` join `traefik_public` only.
- **Hardening:**
  - Traefik middlewares:
    - HSTS;
    - a strict CSP: `default-src 'self'`, plus fonts from Google Fonts only, with no inline scripts;
    - `X-Frame-Options: DENY` and `nosniff`;
    - `Referrer-Policy: same-origin`;
    - a per-IP rate limit.
  - Containers run as non-root with read-only root filesystems, like the scanner.
- **The scanner comes first:**
  - compose limits of `api` 0.25 CPU / 128 MB, `web` 0.1 CPU / 32 MB, `oauth2-proxy` 0.1 CPU / 64 MB (the VPS gives about 1 effective vCPU under sustained load; soak runs 2–3);
  - one shared Postgres `LISTEN` connection for all stream clients.
- **Paper-only (§16) still holds:** no credentials, keys or trading code anywhere in api or web.

## 4. API (read-only JSON)

| Endpoint | Feeds | Returns |
|---|---|---|
| `GET /api/health/now` | Radar hearts, Health | latest `health` row plus its age in ms |
| `GET /api/health/series?hours=24` | Health charts | per-minute series (hours ≤ 168) |
| `GET /api/episodes/open` | Radar table | open episodes: id, kind, event title, legs used, net/gross peak, Q*, opened ms, suspect/stale flags |
| `GET /api/episodes?kind=&from=&to=&net=&flags=&cursor=` | Episodes | keyset-paged (≤ 200/page), newest first |
| `GET /api/episodes/:id` | Episode detail | episode, `episode_legs` at peak, `observations` (re-check timeline) |
| `GET /api/today` | Radar "today" | counts of gross and net episodes today (UTC), plus an hourly series |
| `GET /api/integrity/summary?hours=24` | Health | counts by kind, events per million messages |
| `GET /api/paper/accounts` | Paper | per account: equity, cash, realized, unrealized, locked, max drawdown |
| `GET /api/paper/equity?account=&hours=` | Paper curves | minute marks (`paper_marks`) |
| `GET /api/paper/positions` | Paper | open positions awaiting resolution |
| `GET /api/paper/ledger?account=&cursor=` | Paper | keyset-paged ledger |
| `GET /api/stream` | live Radar | SSE: `episode` (on the scanner's existing `NOTIFY episode`) and `health` (on each new minute row), plus a heartbeat |

- **Money and prices are decimal strings** (e.g. `"0.41"`), never JSON numbers. `NULL` (not measured) and `"0"` (measured zero) stay distinct (§10).
- **Time** is integer ms since epoch.
- **Paper endpoints** read S3c's `paper_*` tables. Until S3c is deployed (or if the tables don't exist) they return empty lists, not errors.
- **Errors** are JSON `{ "error": code, "detail": text }`. A bad filter returns 400; a query timeout or DB unavailable returns 503 (*"The database is thinking. Try again in a moment."*); anything else unexpected returns 500 with no internals leaked.

## 5. Web (React + Vite + TypeScript, "night shift")

**Look:**
- The legacy palette on a dark plum base (`--bg #1b1026`, cards `#241532`, lines teal `#93e1d8`, accent salmon `#ffa69e`, text mint `#ddfff7`), with pixel borders and hard 4–6 px shadows.
- Fonts: Press Start 2P (headings, tags) and VT323 (body, ≥ 20 px).
- Charts are hand-made SVG with `shape-rendering: crispEdges`. No UI kit and no chart library.
- **Night mode only.**

**The fake toggle:** a ☾/☀ switch in the header. Clicking it wiggles it and shows the tooltip *"Night mode only."* It never changes the theme. A test asserts that it doesn't.

**Structure:**
- `web/src/api/`: typed fetchers, plus an `EventSource` wrapper that reconnects with backoff.
- `web/src/components/`: `Box`, `Tag`, `Table`, `Hearts`, `Spark`, `LineChart`, `Money`, `Age`, `EmptyState`, `FakeToggle`, `LiveDot`.
- `web/src/pages/`: `Radar`, `Episodes`, `EpisodeDetail`, `Paper`, `Health`.
- `web/src/theme.css`: tokens.
- Routing with React Router.

**Pages:**
- **Radar (home):**
  - Health bar: 10 hearts, minus one per trouble kind (drops > 0 in the last 5 min, parked shards, lag p99 > 5 s, a health row older than 3 min, capture paused), plus shards, msgs/s, lag p50 and drops.
  - Open episodes (live): kind tag, event, S, net and gross, Q*, a ticking age, an edge bar. A row links to its detail.
  - Today box: an hourly spark and *"N gross, M net. "Fees: N−M. Us: M.""*
  - Empty state: *"0 opportunities. The market is efficient, or we are slow. Probably both."*
- **Episodes:**
  - Filters (kind, date range, net-positive only, flags: suspect / stale / censored) live in the URL.
  - A keyset-paged table.
  - Detail page:
    - timing: opened, closed, duration, censoring, detect lag;
    - at peak: net Π, gross Π, fees, Q*, the $100-capped Π;
    - flags explained in words;
    - legs at peak: best, VWAP, levels, size at best, fees, in S; legs outside S are dimmed;
    - the re-check timeline as a pixel strip plus a table (open, +Δd, 1 s, 2 s, 5 s: still profitable, edge, Q*).
- **Paper:**
  - A latency-scenario switcher.
  - One pixel step chart of equity per account in that scenario, with a legend per policy (P0–P3).
  - Account cards, positions awaiting resolution, and a paged ledger.
  - Copy: *"The naive strategy is doing great. Until 'Other' wins."*
  - Empty until S3c ships: *"No paper trades yet. Nothing ventured, nothing lost."*
- **Health:** per-minute msgs/s, lag p50/p99, drops and reconnects (with markers), integrity by kind and per million messages, and capture status.

**Header:** ♥ logo, navigation, a live dot (solid while connected, hollow while reconnecting), and the fake toggle.

**Quality:**
- Responsive down to phone width.
- Readable sizes, and contrast checked on the night palette.
- Tags carry text, not colour alone.

## 6. Data correctness and states

- **Money:** decimal strings are rendered as sent. A test asserts that no floating-point number ever renders a money value.
- **Time:** shown in UTC, with local time on hover. Durations look like `4.19 s`.
- **Live updates:** Radar refetches `open` and `today` on each `episode` event (no client-side merging). If the stream is down, pages work through a normal refresh, and the live dot shows the state.
- **States:** every page has pixel loading, empty and error states with deadpan copy. Never a blank page.

## 7. Testing

- **API:**
  - Tests run against a throwaway Postgres (the `store` test pattern, `TEST_DATABASE_URL`), with seeded rows and one test per endpoint.
  - They cover: money as strings, 0 ≠ null, page and range limits, 400 on bad filters, empty paper lists when the tables are absent, and that the api's role can't write.
  - An SSE test: a `NOTIFY episode` reaches a stream client.
- **Web:**
  - Vitest + Testing Library component tests: Money, Age, Hearts, charts, empty states, FakeToggle does nothing.
  - Page tests against a mock API.
- **Deploy:** a `docker compose config` check. A post-deploy smoke test: `/api/health/now` without a session redirects to GitHub, and returns data with one.
- **The repo's CI** (fmt, clippy, test, deny) covers `bins/api`. The web gets `npm ci && npm test && npm run build` in CI, with no secrets.

## 8. Out of scope (later phases)

- Papers (S4), Math playground/WASM (S7b), Review with writes (S5), and a public demo.
- Tailscale-only access.
- The GHCR pull-based deploy (Phase 8).
