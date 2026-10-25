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

## Frozen since 2026-10-19

Capture stopped on 2026-10-19. What is left is a static site at radar.isoca.space: a landing page, `/math` and
Sweet spots, read from the frozen files in `web/frozen/`. `cd web && npm run build:portfolio` builds it into
`web/dist-portfolio/` (build the wasm first: `sh scripts/build-wasm.sh`). The live code below is kept but not deployed.

## Layout

| Path | What |
|---|---|
| `crates/arb-core` | Pure math: lemmas, fees, books, fill walk, detectors, episodes |
| `crates/pm-api` | The only crate that touches the network (read-only Polymarket) |
| `crates/store` | The only crate that talks to Postgres (schema + migrations) |
| `crates/sim` | Pure paper trader |
| `crates/cards` | Paper-replication card math |
| `crates/tape` | Raw capture format, shared by the scanner and the compactor |
| `crates/arb-wasm` | The public playground's engine: arb-core/sim behind six JSON functions, built to WASM for `web/` |
| `bins/scanner` | `run` · `census` · `replay` · `migrate` |
| `bins/api` · `bins/analyze` · `bins/compact` | HTTP api · card/sweet-spot jobs · raw hours → Parquet (`run` · `verify` · `bench`) |
| `web/` | React 19 + Vite + TypeScript dashboard |
| `docs/superpowers/` | Specs, plans and deploy runbooks |

`backend/`, `frontend/` and `legacy/` hold the old FastAPI app and are not used.

## Run it

```bash
cargo test --workspace               # Rust tests (the store/compact/api/analyze DB tests need a Postgres)
cd web && npm ci && npm test         # dashboard tests

# The /math page runs arb-wasm in the browser (run from the repo root). One-time setup:
rustup target add wasm32-unknown-unknown
cargo install --locked --version "$(sh scripts/wasm-bindgen-version.sh)" wasm-bindgen-cli   # must match Cargo.lock
# Build the package into web/src/wasm/pkg (PROFILE=dev for the test loop; the default is release, for the /math build):
PROFILE=dev sh scripts/build-wasm.sh
(cd web && WASM_REQUIRED=1 npm test)   # also runs the native = WASM parity test (skipped when pkg is missing)

docker compose --profile core up -d  # postgres, migrate, scanner, analyze, compact
```

The dashboard (`compose.web.yml`: api, web, oauth2-proxy) is VPS-only, behind Traefik and a login.
Only `/math` (with its assets and its snapshot file) is public, with no login; everything else stays behind the login.
Deploys follow the runbooks in `docs/superpowers/runbooks/`.

Copy `.env.example` to `.env` first. See [PROJECT.md](PROJECT.md) for status and roadmap.
