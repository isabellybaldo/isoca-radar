# isoca-radar: dashboard redesign (non-Radar pages, Sweet spots page)

**Date:** 2026-09-27. **Builds on:** Phase 4 (`2026-09-25-phase4-dashboard-design.md`), 5a cards (`2026-09-26-phase5a-cards-design.md`), 5d Sweet spots (`2026-09-27-phase5d-sweet-spots-design.md`). **Leaves to phase 7:** the plain-language explain panel (`2026-09-27-phase7-playground-explain-design.md` §7).
**Status:** designed section by section with the owner on 2026-09-27, checked against the frontend-design skill (§9), approved for spec.

## 1. Purpose, scope and success

The owner reviewed radar.isoca.space in Chrome. Radar is fine. Episodes, Paper, Papers and Health are **too long, crammed and hard to understand**, and **Sweet spots feels missing**: it sits under Edge capture on `/paper` and currently shows "under review · 0 candidates in 0 tested rows" next to a row of live-looking chips.

Observed on the live site:
- `.app` caps at 1100 px; on a 2560 px screen ~70 % is empty while Episodes and Papers wrap numbers mid-value (`14/`↵`14`, `+$11.6`↵`1`, `10.52`↵`s`).
- Boxes nest inside boxes (EDGE CAPTURE › Edge capture), every box has an offset shadow and a `(?)`, so nothing stands out.
- Method paragraphs, the premise string (`depth=1+e3b0c442;…`) and rounding rules sit inline between the numbers.
- At 400 px an Episodes row is ~280 px tall (date on 4 lines, title on 6); nested borders eat ~80 px of width; the header wraps with the theme toggle alone on line 2.

**Owner preference (binding):** simple drawings and schematics over text.

**In scope:**
- One shared section pattern for every non-Radar page (§3).
- Reworked Episodes, Paper, Papers, Health (§4).
- A new `/sweet` page and a Sweet spots card on Radar; Sweet spots leaves `/paper` (§5).
- Wider desktop layout and a real phone layout (§3.4).

**Out of scope:**
- The explain panel / "for dummies" (phase 7).
- Light theme: the toggle stays a joke (`FakeToggle`).
- Episode detail (`/episodes/:id`) and Radar's existing boxes.
- Any api, Rust or deploy change. Every figure comes from payloads the web already fetches; nginx already falls back to `index.html`, so `/sweet` needs no config.

**Exit:** deployed; at 1440 px and 400 px in Chrome, every non-Radar page opens with its drawing above the fold, no number wraps mid-value, no box sits inside a box, and `/sweet` plus the Radar card show the live SWEET state. `npm test` and `npm run build` pass.

## 2. Identity kept

The look stays: night palette (`theme.css` tokens), Press Start 2P for section titles, VT323 for big numbers, IBM Plex Mono for body, hearts, the `LIVE` dot, the joke toggle. The redesign changes structure and hierarchy, not the palette or fonts.

## 3. The section pattern

### 3.1 Anatomy

Every section on a non-Radar page is, top to bottom:

1. **Title row:** section title (Press Start 2P), then right-aligned: window switch if the section has one, status tag (`WARMING`, `UNDER REVIEW`…), `(?)`.
2. **Tiles (only where the number is the answer, §3.3):** 2–4 big numbers, VT323 value over a sentence-case Plex Mono label.
3. **One drawing:** full width. Where a section has no tiles, its key numbers are written on the drawing (end-of-line values, bar counts).
4. **Folds:** `▸ all numbers` (the full table) and `▸ how it's computed` (method paragraphs, premise string, hash, rounding rules, footnotes). Closed by default.

### 3.2 Flat structure

- One box level per section. A former inner box (the Edge capture card inside EDGE CAPTURE, a Gebele card inside its paper) becomes a **sub-heading**: Plex Mono, bold, with a 1 px `--line2` rule above; no border, no shadow.
- **Hierarchy by weight:** each page's hero drawing's section keeps the 3 px border + 4 px offset shadow. Tiles and folds get a flat 2 px `--line2` border, no shadow. Other sections keep the 3 px border, no shadow.
- The page-level SCENARIO and WINDOW boxes disappear; their chips move into the relevant title row.

### 3.3 Where tiles appear

Tiles only on: Edge capture (Paper), `/sweet`, Health › Capture. Everywhere else numbers are written on the drawing. This keeps the "big number, small label" treatment from repeating on every section.

### 3.4 Width and phone

- `.app` max-width 1100 → 1440 px.
- Number columns in every table: `white-space: nowrap; font-variant-numeric: tabular-nums`. Text columns (event titles) truncate with an ellipsis and carry the full text in `title`.
- **< 600 px:** `.app` loses its border and shadow; section padding 10 → 8 px; tiles two per row; tables inside `.scroll` scroll sideways; header on one line (logo, nav chips at 7 px Press Start with 4 px padding, `LIVE` dot, toggle); charts keep a 140 px minimum height.

### 3.5 Components

| Unit | What it does | Depends on |
|---|---|---|
| `components/Tiles.tsx` | `Tiles({ items: { label, value, tone?, title? }[] })`; renders a `<dl>` grid | theme tokens |
| `components/Fold.tsx` | `Fold({ id, label, children })`; native `<details>/<summary>`; open state remembered in `localStorage` under `fold:<id>`, every access in try/catch, closed if storage fails | — |
| `components/Box.tsx` | gains `hero?: boolean` (shadow on) and `actions?: ReactNode` (title-row slot for switches/tags) | existing `Help` |
| `components/SubHead.tsx` | flat sub-heading replacing inner boxes | — |
| `theme.css` | new classes `.tiles`, `.fold`, `.subhead`, `.box.hero`, `.num`, the 600 px media query; `.blink` wrapped in `prefers-reduced-motion: no-preference` | — |

`help.ts` popovers are unchanged; fold labels and tile labels are new copy in the owning page or `lib/` module (§8).

## 4. Pages

### 4.1 Episodes

- **Filters: one line.** Kind chips (ALL/NO/YES/YES?/IMPL), a from–to date range, and a `flags ▾` disclosure holding net-positive only / suspect / stale / censored. "UTC days. Seven days at a time." moves into the filters `(?)`. The FILTERS box becomes the Episodes section's title row content; no separate box.
- **Hero drawing: timeline strip** (`components/EpisodeStrip.tsx`). x = time across the selected days; one tick per episode in view; colour = kind (NO teal, YES salmon, YES?/IMPL dim); tick height ∝ |NET| capped at the window's 95th percentile; flagged episodes hollow. Counts written on the strip's right: `868 episodes · 412 net-positive`. Hover gives the event and NET.
- **Table: one line per episode.** Rows grouped under a day heading row (`27 SEP`), so OPENED shows `17:32:11`. EVENT truncates with ellipsis. DURATION `10.5 s` / `open`. FLAGS as one-letter chips `S` (stale), `C` (censored), `!` (suspect) with the existing `FLAG_WORDS` as `title`.
- **< 600 px:** each row renders as a two-line card: line 1 time, kind tag, NET; line 2 event title (ellipsis), S, duration, flag chips.

### 4.2 Paper

Order: Edge capture, Accounts. Sweet spots is removed (§5).

**Edge capture** (hero):
- Tiles: `on the table` $258.01 · `captured at sp-idle` −$462.87 · `capture rate` −179.4 % · `measured latency` ≈153 ms.
- **Latency axis** (`components/LatencyAxis.tsx`), replacing the two chart panels and the four `scenario · ms → rate` lines. One horizontal ms axis; each scenario is a labelled dot at its configured latency with its rate beside it (`sp-idle 300 ms → −179.4 %`); a distinct marker for **measured ≈153 ms (estimate)**. Scenario latency and measured latency are labelled as different things, so the 300 vs 153 ms figures don't read as a contradiction. Data: the card's existing `DecayPoint[]` and the measured-latency row.
- **Split bar:** one bar for on-the-table, split `NO $51.49 cashable` | `YES $206.52 locked until resolution`, values written on the bar.
- Folds: `▸ all numbers` (today's VALUE table incl. capital needed, biggest opportunity), `▸ how it's computed` (upper-bound note, P0-converter note, capture/on-table/restart/latency paragraphs, premise string, hash, computed time).

**Accounts:**
- Title row: `ACCOUNTS`, scenario chips (COLO-IDLE / SP-BURST / SP-IDLE / SP-IDLE-NODELAY) in `actions`.
- Drawing: the equity chart; each line ends with its account's current equity written at its right end. Replaces the per-account cards (`Cards`), whose figures fold into `▸ all numbers`.
- Folds: `▸ ledger` (current `Ledger`, with its account picker and marks toggle), `▸ awaiting resolution` (positions table).

### 4.3 Papers

- Page title row: `PAPERS`, window switch, `updated 9 min ago`. The standalone intro box goes; its sentence ("Their number, their method on our data, our method.") becomes the `(?)`.
- Each paper: its citation as one small Plex Mono line with the `LIVE` tag, then its cards two per row (one per row < 900 px).
- **Each card** (`CardView`): sub-heading `G1 · Side asymmetry` + status + `(?)`; its existing chart as the drawing; beside or under the chart, the card's headline claim in three columns written in big type: `PAPER 58.28× · THEIRS 0.55× · OURS 0.53×` (the card's first table row). Folds: `▸ all numbers` (full table), `▸ how it's computed` (notes, premise string, hash).
- `NOT REPLICATED` becomes a single `▸ not replicated (N)` fold at the end.

### 4.4 Health

- Page title row: `HEALTH`, window switch (6 H / 24 H / 7 D). The WINDOW box goes.
- **Capture** tiles: `capture` recording ✓ · `shards` 40/40 · `tokens` 39,997 · `last minute` 40 s ago. Clock offset and `/ok` go into `▸ all numbers`.
- **Charts** (hero): msgs/s, lag, drops+reconnects stay three across (stacked < 900 px). "Grey bars mark minutes with reconnects" becomes a legend entry.
- **Integrity:** drawing = one horizontal bar per kind, count written at the bar's end; below it one line `N events in M messages: X per million`. Fold `▸ all numbers` holds the table.

## 5. Sweet spots

### 5.1 `/sweet` page

- **Nav:** `RADAR · SWEET · EPISODES · PAPER · PAPERS · HEALTH`. Route `/sweet` → `pages/Sweet.tsx`. `components/SweetSpots.tsx` logic moves there; `Paper` no longer renders it.
- Title row: `SWEET SPOTS`, window switch (30D default), status tag, `(?)`.
- One line under the title: "Paper, first in line. A candidate is a question for real small orders, not an answer." The full `SWEET_CAVEAT` moves into `▸ how it's computed`.
- **Tiles:** `NO candidates` · `YES candidates` · `rows tested` · `scenario` sp-idle (the headline scenario, `HEADLINE_SCENARIO`).
- **Chips:** side NO/YES; slice ALL/CATEGORY/DAYS/S/HOUR (CATEGORY only when `category_available`); scenario, one at a time, default `HEADLINE_SCENARIO`, `colo-idle` titled with `COLO_TITLE`.
- **Hero: interval chart** (`components/IntervalChart.tsx`). One row per bucket for the chosen side, slice, scenario at fee 0. Shared horizontal axis in $ per attempt with a bold zero line. Each row: bucket label left (`bucketText`), a bar from `lo` to `hi` and a dot at `mean`, `n · C` at right, and for YES rows `%/yr` from `annualized`. Encoding never relies on colour alone:
  - `candidate`: teal, filled dot, `CANDIDATE` tag.
  - `negative`, `dead_on_paper`: salmon, hollow dot, tag.
  - `inconclusive`: dim, filled dot.
  - `thin`, `not_tested`: dim, dashed bar (or no bar when `mean` is null), tag.
  - `few_trades`: small `few trades` note on the row.
- If `nothingPays`: one line above the chart, "Nothing pays at sp-idle yet. Don't trade this, or get closer."
- Folds: `▸ all numbers` (today's grid: all scenarios side by side, unchanged), `▸ how it's computed` (full caveat, Bonferroni footnote with z and its three `(?)`, the card's extra notes, hash, computed time).
- **States:** when `status` is `under_review` or `warming`, or the SWEET card is missing, chips, tiles and chart are replaced by one panel:
  - under review: "Recomputing after a premise change. The last result was HH:MM UTC; new numbers appear when it finishes."
  - warming: the existing `warmingText(card)`.
  - missing: `SWEET_EMPTY`.

### 5.2 Radar card

- Placed directly below TODAY. Title "Sweet spots, last 30 days"; the whole box is a link to `/sweet`.
- Data: `api.cards('30d')` on the same 60 s ticker the current `SweetSpots` uses. Radar's existing `api.cards('today')` call is unchanged.
- One of four bodies:
  - candidates: `NO n · YES m candidates`, plus a small `IntervalChart` row for the best candidate (highest `lo` at `HEADLINE_SCENARIO`, fee 0), e.g. `hour 14–15 UTC +$0.31 [+$0.04, +$0.58]`.
  - nothing pays: "Nothing pays on paper yet."
  - under review: "Recomputing…"
  - warming / missing: "Warming, N days of data" / `SWEET_EMPTY`.

## 6. Data flow

No new endpoints and no recomputation in the web. Pages keep their existing `useApi` calls; the new components are pure functions of those payloads:
- `LatencyAxis` ← Edge capture card's decay points + measured-latency row.
- `IntervalChart` ← `SweetSeries.rows` filtered by `grid()` / a new `rowsFor(s, side, dim, scenario)` in `lib/sweet.ts`.
- `EpisodeStrip` ← the Episodes page's current rows.
- Radar card's best candidate ← `bestCandidate(s)` in `lib/sweet.ts`.

## 7. Errors and empty states

Existing `Loadable` and `EmptyState` handling stays. New copy says what is happening and what ends it, never apologises. A drawing with no data renders its axis and one line (e.g. "No episodes in these days. Widen the dates or clear flags."), never an empty rectangle.

## 8. Copy rules

Sentence case for tile labels, fold labels and annotations; Press Start 2P all-caps only for section titles and chips (existing identity). Fold labels say what's inside. Fewer `A · B · C` meta strings.

## 9. Frontend-design review (2026-09-27)

Revisions made after checking the design against the frontend-design skill:
1. Tiles only where the number is the answer (§3.3); elsewhere numbers live on the drawing.
2. One hero drawing per page gets the shadow; everything else flatter (§3.2).
3. Pixel font for section titles only; labels in Plex Mono sentence case (§8).
4. Interval chart encodes label by fill, dash and tag as well as colour (§5.1); folds use native `<details>`; blink respects reduced motion.
5. Empty and recomputing states give direction (§5.1, §7).

## 10. Testing

Vitest + Testing Library, alongside the existing `*.test.tsx`.
- `Tiles`: renders labels and values.
- `Fold`: closed by default; remembers open state; renders closed without error when `localStorage` throws.
- `IntervalChart`: bar left/right of zero from `lo`/`hi`; each label's fill/dash/tag; null `mean` draws no bar.
- `LatencyAxis`: dot per scenario at its latency; measured marker placed and labelled separately.
- `EpisodeStrip`: one tick per row; flagged ticks hollow; empty-state line.
- `lib/sweet.ts`: `rowsFor`, `bestCandidate` (ties, no candidates, fee filter).
- Pages: existing tests updated where they read table text now inside a fold; new assertions for tiles, closed folds, Episodes day grouping, Sweet spots gone from `/paper`.
- `/sweet`: candidates, nothing pays, under review, warming, missing.
- Radar card: the same four states and its `/sweet` link.
- `npm run build` (tsc + vite).
- After deploy: Chrome check at 1440 px and 400 px on every page.

## 11. Build order

Each step ships on its own.
1. Shared: `Tiles`, `Fold`, `SubHead`, `Box` `hero`/`actions`, width, `.num` nowrap, phone CSS, reduced-motion blink.
2. `/sweet` page, `IntervalChart`, Radar card, nav entry; remove Sweet spots from `/paper`.
3. Paper: Edge capture tiles, `LatencyAxis`, split bar, Accounts section with folds.
4. Episodes: one-line filters, `EpisodeStrip`, grouped one-line table, phone cards.
5. Papers, then Health.
