# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development commands

### Backend (run from `backend/`)
```bash
# Start dev server (auto-reload)
cd backend && ../venv/Scripts/uvicorn app.main:app --reload --port 8001

# On Linux/Mac
cd backend && ../venv/bin/uvicorn app.main:app --reload --port 8001
```

### Frontend (run from `frontend/`)
```bash
npm run dev        # dev server at http://localhost:5173
npm run build      # production build → dist/
npm run lint       # eslint
npm run preview    # preview production build locally
```

### Docker (full stack)
```bash
docker compose up --build      # build and start both services at http://localhost
docker compose up -d --build   # detached
docker compose down
```

## Architecture

**Request flow:**
```
Browser → Vite dev proxy (/api/*) → FastAPI :8001
                                         ├── gamma_client → gamma-api.polymarket.com
                                         └── clob_client  → clob.polymarket.com
```
In production (Docker), the frontend nginx container proxies `/api/` to the `backend` container by hostname.

**Backend** (`backend/app/`):
- `main.py` — FastAPI app, CORS middleware, and FastAPI lifespan that creates/closes the two httpx clients
- `config.py` — Pydantic settings from `backend/.env`. `CORS_ORIGINS` is a comma-separated string (not JSON array)
- `services/gamma_client.py` — wraps `gamma-api.polymarket.com`. Has a module-level in-memory TTL cache (60s). `_parse_market()` must `json.loads()` the `outcomePrices` and `clobTokenIds` fields because Polymarket returns them as JSON-encoded strings
- `services/clob_client.py` — wraps `clob.polymarket.com` for real-time prices and price history
- `routers/markets.py` — `GET /api/markets` (paginated via `limit`/`offset`), `GET /api/markets/{slug}`
- `routers/prices.py` — `GET /api/prices/history?token_id=...&interval=...`

**Frontend** (`frontend/src/`):
- `index.css` — single source of truth for the design system: CSS custom properties (`--mint`, `--teal`, `--salmon`, `--rose`, `--deep`), pixel art utility classes (`.pixel-box`, `.pixel-btn`), and font variables (`--font-pixel` = Press Start 2P, `--font-body` = VT323)
- `api/client.js` — axios instance with `baseURL: '/api'`; response interceptor unwraps `.data` so hooks receive the payload directly
- `hooks/` — data fetching hooks (`useMarkets` handles pagination state, `useMarketDetail` fetches market + price history in sequence)
- CSS is done with **CSS Modules** (`.module.css` per component) + the global utility classes from `index.css`

## Polymarket API notes

- **`outcomePrices`** is a JSON-encoded string in Gamma API responses — always `json.loads()` before use
- **`clobTokenIds`** (index 0 = YES token, index 1 = NO token) is what the CLOB API needs; do not use `conditionId` for price queries
- Pass `closed="false"` as a string, not Python bool, to Gamma API query params
- httpx clients are singletons created once at startup via the FastAPI lifespan — never instantiate per-request

## Environment

Copy `backend/.env.example` → `backend/.env` to run locally. Key variables:
- `CORS_ORIGINS` — comma-separated list of allowed origins (e.g. `http://localhost:5173,https://yourdomain.com`)
- `GAMMA_API_URL` — defaults to `https://gamma-api.polymarket.com`
- `CLOB_API_URL` — defaults to `https://clob.polymarket.com`

## Deployment

GitHub Actions (`.github/workflows/deploy.yml`) SSHes into the VPS on push to `main` and runs `docker compose up -d --build`. Required secrets: `VPS_HOST`, `VPS_USER`, `VPS_SSH_KEY`, `VPS_APP_PATH`.
