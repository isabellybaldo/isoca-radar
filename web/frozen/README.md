# Frozen data

The portfolio site's only data. Exported once from the old VPS on 2026-10-25 (runbook
`docs/superpowers/runbooks/old-vps-backup.md`, step 4), byte for byte; never edited by hand.

| File | What | Read by |
|---|---|---|
| `public.json` | the api's last public snapshot: the latency ladder and the day's counts | /math |
| `cards-all.json` | `GET /api/cards?win=all`: every card, ALL window; the page uses the `SWEET` card | Sweet spots |

- Capture stopped on 2026-10-19. The last captured minute: `FROZEN_MS = 1792432680000`
  (2026-10-19 17:58 UTC).
- Sweet spots data covers 2026-10-16 15:08 → 2026-10-19 17:23 UTC (110 paper trades, since the paper
  accounts' last restart).
- Only the ALL window ships: 7D and 30D held the same three days and were still `warming`.
- `.gitattributes` keeps these files `-text`, so git never rewrites their bytes.
- Guarded by `web/src/frozen.test.ts`.
