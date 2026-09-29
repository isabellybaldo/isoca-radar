# isoca-radar

A read-only lab for Polymarket arbitrage. It watches the live order books, detects NegRisk and
combinatorial arbitrage net of fees and depth, paper-trades it, and compares what it sees with
the published papers. **Paper only: there is no code path to an exchange.**

```
 Polymarket (public, no auth)   gamma · clob /books · wss /ws/market
        │
        ▼
 scanner ──► postgres ◄── analyze (cards, sweet spots)
   │  raw capture          ▲
   ▼                       │
 compact ──► Parquet archive (one self-contained folder per UTC day)
                           │
 api ◄─────────────────────┘
   ▼
 web (React dashboard: Radar · Sweet · Episodes · Paper · Papers · Health)
```

## Layout

| Path | What |
|---|---|
| `crates/arb-core` | Pure math: lemmas, fees, books, fill walk, detectors, episodes |
| `crates/pm-api` | The only crate that touches the network (read-only Polymarket) |
| `crates/store` | The only crate that talks to Postgres (schema + migrations) |
| `crates/sim` | Pure paper trader |
| `crates/cards` | Paper-replication card math |
| `crates/tape` | Raw capture format, shared by the scanner and the compactor |
| `bins/scanner` | `run` · `census` · `replay` · `migrate` |
| `bins/api` · `bins/analyze` · `bins/compact` | HTTP api · card/sweet-spot jobs · raw hours → Parquet (`run` · `verify` · `bench`) |
| `web/` | React 19 + Vite + TypeScript dashboard |
| `docs/superpowers/` | Specs, plans and deploy runbooks |

`backend/`, `frontend/` and `legacy/` hold the old FastAPI app and are not used.

## Run it

```bash
cargo test --workspace               # Rust tests (the store/compact/api/analyze DB tests need a Postgres)
cd web && npm ci && npm test         # dashboard tests
docker compose --profile core up -d  # postgres, migrate, scanner, analyze, compact
```

The dashboard (`compose.web.yml`: api, web, oauth2-proxy) is VPS-only, behind Traefik and a login.
Deploys follow the runbooks in `docs/superpowers/runbooks/`.

Copy `.env.example` to `.env` first. See [PROJECT.md](PROJECT.md) for status and roadmap.
