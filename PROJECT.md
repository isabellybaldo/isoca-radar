# isoca-radar: status and roadmap

Parent spec: [`docs/superpowers/specs/2026-09-22-arb-scanner-design.md`](docs/superpowers/specs/2026-09-22-arb-scanner-design.md)
(the math in §5, the architecture in §8, the phases in §19). Each phase has its own spec, plan and runbook
under `docs/superpowers/`.

## Where it runs

```
 VPS (memory-constrained, ≤ 75 GB for this project)
 ┌───────────────────────────────────────────────────────────────────────┐
 │ core:  postgres · scanner · analyze (128m) · compact (512m)            │
 │ web:   api (128m) · web (32m) · oauth2-proxy (64m) ◄── Traefik, login   │
 │ disk:  raw capture ≤ 30 GB (~24 h) · Parquet archive ≤ 40 GB            │
 └───────────────────────────────────────────────────────────────────────┘
 PC: builds the images, runs the tests, queries the archive with DuckDB
```

## Status (2026-09-29)

| Phase | What | State |
|---|---|---|
| 0–4 | Spike · arb-core · scanner + store · paper trader · api + dashboard | live |
| 5a | Gebele cards, edge capture | live |
| 5d | Sweet spots: captured edge after fees and capital lockup | live |
| 5b | On-VPS tiered storage: `compact` turns raw hours into a daily Parquet archive; the scanner deletes raw hours only once they're compacted | live since 2026-09-28 |
| next | Scanner engine: a duplicated live token gets an empty book on universe refresh | to fix |
| 7 | Public `/math` WASM playground + login-only "explain" panel | specced |
| 5c | Saguillo cards + Cloudflare R2 external storage | not before 2026-10-05 |
| 6 | Laya oracle + Review page | waits on VPS memory |

## Rules that hold everywhere

- **Paper only.** `pm-api` is the only network crate and is read-only; `sim` writes ledger entries, never orders.
- **No floats in the math crates**; money is `NUMERIC` in Postgres, micro-units in Parquet.
- **One writer per concern:** `store` owns all SQL, `tape` owns the capture format.
- **New crates need the owner's approval** (`deny.toml` enforces the boundaries).
- **Anything public must be static and rate-limited.**
