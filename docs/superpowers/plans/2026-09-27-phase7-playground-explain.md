# isoca-radar Phase 7: public math playground (WASM) and the explain panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A public `/math` page whose five schematics (a–e) are computed by `arb-core` compiled to WASM, served as static files behind strict per-IP limits, plus a login-only EXPLAIN side panel on every dashboard page that answers the owner's plain questions with drawings filled with live numbers.

**Architecture:** New `arb-core` functions (`payoff`, `d1_sums`, `classify`, `walk_steps`) hold every decision and money computation. A new crate `crates/arb-wasm` is a JSON-in/JSON-out facade over them (`src/api.rs`, testable natively) with thin `#[wasm_bindgen]` wrappers. Golden fixtures prove that native and WASM outputs are identical strings. The web gets pure SVG/HTML views in `src/schematics/`. The public page (`math.html`, a separate Vite build into `dist/math-assets/`) computes models with WASM. The dashboard panel (`src/explain/`) builds models from api data the pages already fetched. Traefik gets a `ct-math` router with no auth, per-IP rate and in-flight limits, and its own CSP; nginx serves precompressed static files.

**Tech Stack:** Rust 1.98.1 (edition 2024), rust_decimal (`serde-with-str`), serde_json (`arbitrary_precision`), proptest, wasm-bindgen (new, exact pin), wasm-bindgen-cli; React 19, React Router 7, Vite 8 (rolldown), TypeScript 5.9, Vitest 4 + Testing Library; nginx-unprivileged 1.29, Traefik (existing).

**Spec:** `docs/superpowers/specs/2026-09-27-phase7-playground-explain-design.md` (parent: `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` §5.7, §14, §19; the dashboard: `docs/superpowers/specs/2026-09-25-phase4-dashboard-design.md`, `docs/superpowers/specs/2026-09-26-phase5a-cards-design.md`).

This is ONE plan with 14 tasks:
- `arb-core`: Tasks 1–3.
- `arb-wasm`: Task 4.
- WASM build and parity: Task 5.
- Schematics: Tasks 6–7.
- Math page: Task 8.
- Deploy files: Task 9.
- api field: Task 10.
- Explain panel: Tasks 11–14.

Tasks 1–4 and 10 touch Rust only (Task 10 also touches web types). Tasks 6–8 and 11–14 touch web only.

## Global Constraints

- **Paper only (parent §16):** no order placement, wallets, keys, signing or exchange clients anywhere. Nothing in this plan talks to Polymarket.
- **All logic lives in `arb-core`:** `arb-wasm/src/api.rs` only parses JSON, validates limits, builds `arb-core` inputs, calls `arb-core`, and serializes results. The facade computes no money figure. Its only decision is choosing which update is "in force at `t_x`" (spec §4.1).
- **No floats in Rust:** clippy denies `float_arithmetic`, `unwrap_used`, `expect_used` (tests may unwrap: `clippy.toml`). `arb-core` and `arb-wasm` get the no-`f32`/`f64` source scan.
- **Decimals cross JSON as strings.** A JSON number where a decimal is expected is an error. Every output decimal is `normalize()`d, and zero is `"0"` (never `"-0"`). Outputs are `#[derive(Serialize)]` structs (fixed field order).
- **arb-wasm limits (spec §4.1), checked before any work:** input ≤ 16 KiB; ≤ 6 legs; ≤ 8 levels per side per leg; prices 0.01–0.99 on the 0.01 grid; sizes > 0 and ≤ 1,000,000 with ≤ 6 dp; `s` 2..=6; latencies (`proc_ms`, `net_ms`, `match_ms`) 0..=5,000; `seconds_delay` 0..=10; `gap_ms` 1..=600,000; updates 2..=8 with strictly increasing `t_ms` in 0..=600,000. Anything outside them returns `{"error":"<sentence>"}`, and nothing is clamped.
- **New dependencies:**
  - **Rust:** exactly one new crates.io crate, `wasm-bindgen`, approved with the spec. It's pinned `=x.y.z` and used only under `cfg(target_arch = "wasm32")`. No other new crate; everything else comes from `[workspace.dependencies]`.
  - **Web:** no new npm dependency (compression uses Node's built-in `zlib`).
  - **No fresh `npm install` in `web/`** (npm 10.9.2 crashes); reuse `web/node_modules`.
- **Debug builds only on the PC:** `PROFILE=dev sh scripts/build-wasm.sh`. `--release` builds happen only in Docker and CI.
  - `wasm-bindgen-cli` must be installed on the PC once, and that's an owner-approved step (Task 5 Step 1 stops and asks).
  - The wasm32 target is already installed on the PC (2026-09-27).
- **The public route never reaches the api:** the `ct-math` router matches only `Path(/math)` and `PathPrefix(/math-assets/)`. The math bundle holds no module from `src/pages/`, `src/api/`, `src/explain/` or `src/help.ts`; the build fails if it does.
- **Traefik `ct-math` (spec §5.3), exact values:**
  - router: `entrypoints=websecure`, `tls.certresolver=letsencrypt`, `service=ct-web`, `priority=1000`;
  - middlewares: `ct-math-rl@docker,ct-math-if@docker,ct-sec@docker,ct-math-csp@docker`;
  - `ct-math-rl`: `ratelimit.average=5`, `ratelimit.burst=20`, `ratelimit.sourcecriterion.ipstrategy.depth=0`;
  - `ct-math-if`: `inflightreq.amount=4`, `inflightreq.sourcecriterion.ipstrategy.depth=0`.
- **Math CSP, exact:** `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`. The dashboard CSP (`ct-csp`) is unchanged.
- **Public page size:** at most **350 KB gzip** for everything under `dist/math-assets/` (JS + WASM + CSS).
- **Explain panel honesty (spec §7.2):**
  - every money figure is labelled *paper*, *on the table*, or *captured at N ms*;
  - the rendered panel text never contains the whole words `earned`, `earn`, `made` or `income` (case-insensitive);
  - `profit` appears only inside `profit @ $100`, `net profit at the peak`, `profit after` or `profit before`.
- **The panel's fetch rule (spec §7.2):** no fetches of its own, except "Would we have caught it?" on Episode detail, which calls `api.cards('24h')` once when opened.
- **Layout:**
  - docked beside the app at `min-width: 1500px`, otherwise an overlay drawer `min(360px, 100vw)` wide;
  - no horizontal page scroll at 360 px;
  - Esc closes the drawer and returns focus to the tab.
- **localStorage** keys are `explain.open` (`"1"`/`"0"`) and `explain.q.<page>` (question id), every access wrapped in try/catch. The default is closed.
- **Copy strings, exact:**
  - math crossed book: *"Crossed book. Somewhere a feed is lying."*
  - math footer: *"The live dashboard is private. The math is not."*
  - panel caption on Episode detail: *"simplified: per-level steps are not stored"*
  - WASM failure: *"The math engine did not load. Your browser may be too old, or something blocked it."*
- **Commits:**
  - stage only the files the task touched (`git add <paths>`, never `-A` or `.`);
  - never stage `PROJECT.md` or `README.md` (untracked, the owner's);
  - commit messages have **no** `Co-Authored-By` or any other attribution trailer.
- **Shell prefix for every command:** `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`.
- **Postgres-backed tests** (Task 10 only):
  - they use the Postgres on **127.0.0.1:15432**, with commands prefixed by `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" &&`;
  - first check `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up`, and if it doesn't print `pg-up`, stop and ask the owner;
  - `SKIPPED: TEST_DATABASE_URL not set` is not a pass.
- **Secrets and the machine:**
  - never print `.env`;
  - never run `docker compose config` except as `docker compose --env-file .env.example … config -q`;
  - never restart Docker or WSL;
  - Docker builds happen only in Task 9's runbook, run by the owner.

## Review Focus

1. **Hostile or sloppy input to the public page's engine:** the likely cases are:
   - a price sent as a JSON number (`0.55`);
   - `"0.555"`, `"1.00"` or `"0"`;
   - two levels at the same price;
   - `s = 1`;
   - 7 legs;
   - a 20 KiB string.

   Each must return one plain error sentence, never panic, trap or hang. Pinned by `bad_inputs_return_one_sentence_errors` and the `never_panics` proptest (Task 4).
2. **The engine fails to load** (old browser, CSP blocks `wasm-unsafe-eval`, a truncated download): `/math` must show the WASM-failure sentence, not a blank page. Pinned by `shows_a_sentence_when_the_engine_fails_to_load` (Task 8).
3. **Money that was never measured:** `net_budget100: null`, `edge_budget100: null`, a `rate_pct: null` decay point, or `end_date: null` must read "not measured" or "unknown" in the panel, never `$0.00` or `NaN days`. Pinned by `unmeasured_figures_never_read_as_zero` (Task 12) and `unknown_end_date_reads_unknown` (Task 14).
4. **A selection that disappears:** the owner selects an open Radar row, the episode closes and the next SSE refresh drops the row. The panel must keep answering about the snapshot it holds (labelled "as selected"), with no crash or blank. Pinned by `selection_survives_its_row_leaving_the_list` (Task 12).
5. **Stale or foreign localStorage:** `explain.q.radar = "gone-question"`, `explain.open = "yes"`, or storage throwing (private window). The panel falls back to closed and the first question, and never throws. Pinned by `bad_storage_falls_back_to_defaults` (Task 11).

## File Structure

```
Cargo.toml                                        MODIFY: members += crates/arb-wasm
crates/arb-core/src/payoff.rs                     NEW: payoff matrices (§3.1)
crates/arb-core/src/book.rs                       MODIFY: d1_sums (§3.2)
crates/arb-core/src/detect.rs                     MODIFY: shared guards, Node, Classification, classify (§3.3)
crates/arb-core/src/fill.rs                       MODIFY: Step, walk_steps (§3.4)
crates/arb-core/src/lib.rs                        MODIFY: pub mod payoff
crates/arb-core/tests/payoff.rs                   NEW
crates/arb-core/tests/steps.rs                    NEW: d1_sums + walk_steps
crates/arb-core/tests/classify.rs                 NEW
crates/arb-wasm/Cargo.toml                        NEW
crates/arb-wasm/src/lib.rs                        NEW: wasm-bindgen wrappers (wasm32 only)
crates/arb-wasm/src/api.rs                        NEW: the five *_json functions
crates/arb-wasm/src/input.rs                      NEW: parsing + limits
crates/arb-wasm/fixtures/{mirror,payoff,classify,depth_walk,episode}.json  NEW: golden cases
crates/arb-wasm/tests/fixtures.rs                 NEW: native golden runner (UPDATE_FIXTURES=1)
crates/arb-wasm/tests/values.rs                   NEW: hand-computed expectations + bad inputs
crates/arb-wasm/tests/never_panics.rs             NEW: proptest
crates/arb-wasm/tests/no_floats.rs                NEW
scripts/build-wasm.sh                             NEW
.gitignore                                        MODIFY: web/src/wasm/pkg/
.github/workflows/ci.yml                          MODIFY: web job builds wasm, runs native + wasm parity
web/package.json                                  MODIFY: scripts only
web/tsconfig.json                                 MODIFY: exclude src/math, src/wasm
web/tsconfig.math.json                            NEW
web/vite.math.config.ts                           NEW (+ noDashboardCode plugin)
web/scripts/check-math-bundle.mjs                 NEW: gzip size budget + writes .gz for gzip_static
web/math.html                                     NEW
web/src/wasm/parity.test.ts                       NEW
web/src/wasm/load.ts                              NEW: typed loader over pkg
web/src/schematics/models.ts                      NEW: TS types of arb-wasm outputs
web/src/schematics/{MirrorBook,PayoffTable,ClassifyTree,DepthWalk,EpisodeTimeline}.tsx  NEW
web/src/schematics/schematics.css                 NEW
web/src/schematics/*.test.tsx                     NEW
web/src/math/{main.tsx,MathPage.tsx,copy.ts,defaults.ts,math.css}  NEW
web/src/math/sections/{A,B,C,D,E}*.tsx            NEW
web/src/math/MathPage.test.tsx                    NEW
web/src/math/guard.test.ts                        NEW
docker/web.Dockerfile                             MODIFY: wasm stage (cargo-chef), pkg copy
docker/web/nginx.conf                             MODIFY: /math, /math-assets/
compose.web.yml                                   MODIFY: ct-math router + 3 middlewares
docs/superpowers/runbooks/phase7-playground-vps.md NEW
crates/store/src/read_paper.rs                    MODIFY: PaperPosition.end_date
crates/store/tests/read_paper.rs                  MODIFY
bins/api/src/paper.rs                             MODIFY: PositionDto.end_date
web/src/api/types.ts                              MODIFY: Position.end_date
web/src/test/fixtures.ts                          MODIFY
web/src/explain/{ExplainContext.tsx,Explain.tsx,useMedia.ts,storage.ts,explain.css}  NEW
web/src/explain/{questions.tsx,copy.ts,models.ts,select.tsx}  NEW
web/src/explain/*.test.tsx                        NEW
web/src/App.tsx                                   MODIFY: provider + panel
web/src/pages/{Radar,Episodes,EpisodeDetail,Paper,Papers,Health}.tsx  MODIFY: publish data, explain buttons
README.md                                         NOT TOUCHED (owner's, untracked). Dev prerequisites go in the runbook.
```

---

### Task 1: `arb_core::payoff`

**Files:**
- Create: `crates/arb-core/src/payoff.rs`
- Modify: `crates/arb-core/src/lib.rs`
- Test: `crates/arb-core/tests/payoff.rs`

**Interfaces:**
- Produces: `arb_core::payoff::{Matrix, PayoffError, yes_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError>, no_basket(s, other_listed) -> Result<Matrix, PayoffError>, implication() -> Matrix}`.
- `Matrix { rows: Vec<String>, cols: Vec<String>, possible: Vec<bool>, cells: Vec<Vec<Option<Decimal>>>, basket: Vec<Option<Decimal>>, guaranteed: Decimal, broken: Vec<bool> }`.
- Outcome names are `"A"`, `"B"`, … (`s` ≤ 12); the uncovered column is `"Other"`; YES rows are `"YES_A"`, and NO rows are `"NO_A"`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/arb-core/tests/payoff.rs
//! Phase 7 spec §3.1: the §5.7 (b) payoff tables as code (L3, L4, L10).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::payoff::{PayoffError, implication, no_basket, yes_basket};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn yes_basket_with_other_uncovered_matches_the_spec_table() {
    let m = yes_basket(3, false).unwrap();
    assert_eq!(m.rows, vec!["YES_A", "YES_B", "YES_C"]);
    assert_eq!(m.cols, vec!["A", "B", "C", "Other"]);
    assert_eq!(m.cells[0], vec![Some(dec!(1)), Some(dec!(0)), Some(dec!(0)), Some(dec!(0))]);
    assert_eq!(m.basket, vec![Some(dec!(1)), Some(dec!(1)), Some(dec!(1)), Some(dec!(0))]);
    assert_eq!(m.guaranteed, dec!(1));
    assert_eq!(m.broken, vec![false, false, false, true]);
}

#[test]
fn no_basket_pays_s_minus_one_and_s_in_other() {
    let m = no_basket(3, false).unwrap();
    assert_eq!(m.rows, vec!["NO_A", "NO_B", "NO_C"]);
    assert_eq!(m.basket, vec![Some(dec!(2)), Some(dec!(2)), Some(dec!(2)), Some(dec!(3))]);
    assert_eq!(m.guaranteed, dec!(2));
    assert!(m.broken.iter().all(|b| !b));
}

#[test]
fn implication_has_four_columns_one_impossible() {
    let m = implication();
    assert_eq!(m.cols, vec!["A∧B", "A∧¬B", "¬A∧B", "¬A∧¬B"]);
    assert_eq!(m.possible, vec![true, false, true, true]);
    assert_eq!(m.rows, vec!["NO_A", "YES_B"]);
    assert_eq!(m.cells[0], vec![Some(dec!(0)), None, Some(dec!(1)), Some(dec!(1))]);
    assert_eq!(m.cells[1], vec![Some(dec!(1)), None, Some(dec!(1)), Some(dec!(0))]);
    assert_eq!(m.basket, vec![Some(dec!(1)), None, Some(dec!(2)), Some(dec!(1))]);
    assert_eq!(m.broken, vec![false, false, false, false]);
}

#[test]
fn sizes_outside_2_to_12_are_errors_not_clamped() {
    assert_eq!(yes_basket(1, true).unwrap_err(), PayoffError::Size(1));
    assert_eq!(no_basket(13, true).unwrap_err(), PayoffError::Size(13));
}

proptest! {
    #[test]
    fn l3_yes_basket_broken_iff_other_uncovered(s in 2usize..=12, listed: bool) {
        let m = yes_basket(s, listed).unwrap();
        prop_assert_eq!(m.broken.iter().any(|b| *b), !listed);
    }

    #[test]
    fn l4_no_basket_never_broken(s in 2usize..=12, listed: bool) {
        let m = no_basket(s, listed).unwrap();
        prop_assert!(m.broken.iter().all(|b| !b));
        let floor = Decimal::from(s as u64) - Decimal::ONE;
        for b in m.basket.iter().flatten() { prop_assert!(*b >= floor); }
    }
}

#[test]
fn l10_implication_pays_at_least_one_where_possible() {
    let m = implication();
    for (c, b) in m.basket.iter().enumerate() {
        if m.possible[c] { assert!(b.unwrap() >= dec!(1)); }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-core --test payoff`
Expected: FAIL to compile, with "could not find `payoff` in `arb_core`".

- [ ] **Step 3: Implement**

```rust
// crates/arb-core/src/payoff.rs
//! Payoff tables (parent spec §5.7 (b); Phase 7 spec §3.1). Rows are holdings, columns are the
//! winner. `broken[c]`: a possible column where the basket pays less than it guarantees (L3).
use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PayoffError {
    #[error("a basket needs 2 to 12 outcomes, got {0}")]
    Size(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub rows: Vec<String>,
    pub cols: Vec<String>,
    pub possible: Vec<bool>,
    pub cells: Vec<Vec<Option<Decimal>>>,
    pub basket: Vec<Option<Decimal>>,
    pub guaranteed: Decimal,
    pub broken: Vec<bool>,
}

const NAMES: [&str; 12] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L"];

fn check(s: usize) -> Result<(), PayoffError> {
    if (2..=12).contains(&s) { Ok(()) } else { Err(PayoffError::Size(s)) }
}

fn cols(s: usize, other_listed: bool) -> Vec<String> {
    let mut c: Vec<String> = NAMES[..s].iter().map(|n| (*n).to_string()).collect();
    if !other_listed {
        c.push("Other".to_string());
    }
    c
}

fn finish(rows: Vec<String>, cols: Vec<String>, possible: Vec<bool>, cells: Vec<Vec<Option<Decimal>>>, guaranteed: Decimal) -> Matrix {
    let basket: Vec<Option<Decimal>> = (0..cols.len())
        .map(|c| if possible[c] { Some(cells.iter().filter_map(|r| r[c]).sum()) } else { None })
        .collect();
    let broken = (0..cols.len())
        .map(|c| possible[c] && basket[c].is_some_and(|b| b < guaranteed))
        .collect();
    Matrix { rows, cols, possible, cells, basket, guaranteed, broken }
}

fn unit(pays: bool) -> Option<Decimal> {
    Some(if pays { Decimal::ONE } else { Decimal::ZERO })
}

/// D2a: one YES of each listed outcome. Pays 1 if a listed outcome wins, 0 if "Other" does (L3).
pub fn yes_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError> {
    check(s)?;
    let cols = cols(s, other_listed);
    let rows = NAMES[..s].iter().map(|n| format!("YES_{n}")).collect();
    let cells = (0..s).map(|i| (0..cols.len()).map(|c| unit(c == i)).collect()).collect();
    Ok(finish(rows, cols.clone(), vec![true; cols.len()], cells, Decimal::ONE))
}

/// D2b: one NO of each listed outcome. Pays s − 1 if a listed outcome wins, s otherwise (L4).
pub fn no_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError> {
    check(s)?;
    let cols = cols(s, other_listed);
    let rows = NAMES[..s].iter().map(|n| format!("NO_{n}")).collect();
    let cells = (0..s).map(|i| (0..cols.len()).map(|c| unit(c != i)).collect()).collect();
    let guaranteed = Decimal::from(s as u64) - Decimal::ONE;
    Ok(finish(rows, cols.clone(), vec![true; cols.len()], cells, guaranteed))
}

/// D3: A ⇒ B, holding NO_A + YES_B. A∧¬B is impossible (L10).
pub fn implication() -> Matrix {
    let cols: Vec<String> = ["A∧B", "A∧¬B", "¬A∧B", "¬A∧¬B"].iter().map(|c| (*c).to_string()).collect();
    let possible = vec![true, false, true, true];
    let z = unit(false);
    let o = unit(true);
    let cells = vec![vec![z, None, o, o], vec![o, None, o, z]];
    finish(vec!["NO_A".into(), "YES_B".into()], cols, possible, cells, Decimal::ONE)
}
```

Add `pub mod payoff;` to `crates/arb-core/src/lib.rs` after `pub mod money;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p arb-core --test payoff && cargo test -p arb-core --test no_floats && cargo clippy -p arb-core --all-targets -- -D warnings`
Expected: PASS, no warnings. (`s as u64` is an integer cast, not float arithmetic.)

- [ ] **Step 5: Commit**

```bash
git add crates/arb-core/src/payoff.rs crates/arb-core/src/lib.rs crates/arb-core/tests/payoff.rs
git commit -m "arb-core: payoff matrices for the playground (L3, L4, L10)"
```

---

### Task 2: `book::d1_sums` and `fill::walk_steps`

**Files:**
- Modify: `crates/arb-core/src/book.rs` (after `impl Book`), `crates/arb-core/src/fill.rs` (after `walk`)
- Test: `crates/arb-core/tests/steps.rs`

**Interfaces:**
- Produces: `arb_core::book::d1_sums(top: &Top) -> Option<(Decimal, Decimal)>` returning (long, short).
- Produces: `arb_core::fill::{Step { q_from: Decimal, q_to: Decimal, cost_per_basket: Decimal, payoff_per_basket: Decimal, profitable: bool }, walk_steps(legs: &[BuyLeg], rule: BasketRule) -> Vec<Step>}`.
  - `cost_per_basket` = Σ price + Σ φ(price) (exact fees).
  - `profitable` = `payoff_per_basket − cost_per_basket > 0`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/arb-core/tests/steps.rs
//! Phase 7 spec §3.2 (D1 sums, L2) and §3.4 (walk_steps agrees with walk, L8).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::{Book, d1_sums};
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk, walk_steps};
use arb_core::money::{Price, Shares};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn px(s: &str) -> Price { Price::parse(s).unwrap() }
fn sh(s: &str) -> Shares { Shares::parse(s).unwrap() }

#[test]
fn d1_sums_on_the_fed_book() {
    let mut b = Book::default();
    b.apply_snapshot(&[(px("0.54"), sh("5603.74"))], &[(px("0.55"), sh("32356.03"))], 0);
    assert_eq!(d1_sums(&b.top()), Some((dec!(1.01), dec!(0.99))));
}

#[test]
fn d1_sums_needs_both_sides() {
    let mut b = Book::default();
    b.apply_snapshot(&[(px("0.54"), sh("1"))], &[], 0);
    assert_eq!(d1_sums(&b.top()), None);
}

#[test]
fn walk_steps_includes_the_unprofitable_tail() {
    let legs = vec![
        BuyLeg { levels: vec![(px("0.30"), sh("10")), (px("0.40"), sh("10"))], fee: FeeSchedule::ZERO },
        BuyLeg { levels: vec![(px("0.60"), sh("15"))], fee: FeeSchedule::ZERO },
    ];
    let steps = walk_steps(&legs, BasketRule::AllLegs { payoff: Decimal::ONE });
    assert_eq!(steps.len(), 2);
    assert_eq!((steps[0].q_from, steps[0].q_to, steps[0].cost_per_basket, steps[0].profitable), (dec!(0), dec!(10), dec!(0.90), true));
    assert_eq!((steps[1].q_from, steps[1].q_to, steps[1].cost_per_basket, steps[1].profitable), (dec!(10), dec!(15), dec!(1.00), false));
    let w = walk(&legs, BasketRule::AllLegs { payoff: Decimal::ONE }, None);
    assert_eq!(w.q_max, dec!(10));
}

fn level() -> impl Strategy<Value = (Price, Shares)> {
    (1u32..=99, 1u32..=500).prop_map(|(c, s)| (Price::new(Decimal::new(c.into(), 2)).unwrap(), Shares::new(Decimal::from(s)).unwrap()))
}

fn legs() -> impl Strategy<Value = Vec<BuyLeg>> {
    prop::collection::vec(
        (prop::collection::btree_map(1u32..=99, 1u32..=500, 1..=4), any::<bool>()),
        2..=5,
    )
    .prop_map(|v| {
        v.into_iter()
            .map(|(m, fee)| BuyLeg {
                levels: m.into_iter().map(|(c, s)| (Price::new(Decimal::new(c.into(), 2)).unwrap(), Shares::new(Decimal::from(s)).unwrap())).collect(),
                fee: if fee { FeeSchedule::new(dec!(0.07), 1).unwrap() } else { FeeSchedule::ZERO },
            })
            .collect()
    })
}

proptest! {
    #[test]
    fn profitable_prefix_equals_walk(legs in legs(), nested: bool) {
        let rule = if nested { BasketRule::NestedNo } else { BasketRule::AllLegs { payoff: Decimal::ONE } };
        let steps = walk_steps(&legs, rule);
        let w = walk(&legs, rule, None);
        let prefix: Vec<_> = steps.iter().take_while(|s| s.profitable).collect();
        let q: Decimal = prefix.iter().map(|s| s.q_to - s.q_from).sum();
        let pnl: Decimal = prefix.iter().map(|s| (s.payoff_per_basket - s.cost_per_basket) * (s.q_to - s.q_from)).sum();
        prop_assert_eq!(q, w.q_max);
        prop_assert_eq!(pnl, w.pnl_exact);
        for pair in steps.windows(2) { prop_assert_eq!(pair[0].q_to, pair[1].q_from); }
        if !nested {
            for pair in steps.windows(2) { prop_assert!(pair[0].cost_per_basket <= pair[1].cost_per_basket); }
        }
    }

    #[test]
    fn d1_crossed_iff_long_below_one(b in level(), a in level()) {
        let mut book = Book::default();
        book.apply_snapshot(&[b], &[a], 0);
        let (long, short) = d1_sums(&book.top()).unwrap();
        prop_assert_eq!(book.is_crossed(), long < Decimal::ONE);
        prop_assert_eq!(book.is_crossed(), short > Decimal::ONE);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-core --test steps`
Expected: FAIL to compile, with "unresolved imports `arb_core::book::d1_sums`, `arb_core::fill::walk_steps`".

- [ ] **Step 3: Implement**

Append to `crates/arb-core/src/book.rs`:

```rust
/// D1 sums (parent §5.7 (a), L2): long = A + (1 − B), short = B + (1 − A). Both need a bid B and an
/// ask A. The book is crossed (B > A) exactly when long < 1 and short > 1.
pub fn d1_sums(top: &Top) -> Option<(Decimal, Decimal)> {
    let (b, _) = top.best_bid?;
    let (a, _) = top.best_ask?;
    let (a, b) = (a.get(), b.get());
    Some((a + (Decimal::ONE - b), b + (Decimal::ONE - a)))
}
```

Append to `crates/arb-core/src/fill.rs`:

```rust
/// One segment of the merged level breakpoints (Phase 7 spec §3.4), for drawing the depth walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub q_from: Decimal,
    pub q_to: Decimal,
    /// Σ price + Σ φ(price) with exact fees: what one more basket costs on this segment.
    pub cost_per_basket: Decimal,
    pub payoff_per_basket: Decimal,
    pub profitable: bool,
}

/// Every segment `walk` would visit, without its stop at the first unprofitable one, until the
/// rule runs out of legs. The profitable prefix is exactly `walk(legs, rule, None)` (tested).
pub fn walk_steps(legs: &[BuyLeg], rule: BasketRule) -> Vec<Step> {
    let n = legs.len();
    let mut idx = vec![0usize; n];
    let mut rem: Vec<Decimal> = legs.iter().map(|l| l.levels.first().map_or(Decimal::ZERO, |x| x.1.get())).collect();
    let mut q = Decimal::ZERO;
    let mut out = Vec::new();
    loop {
        let active: Vec<usize> = (0..n).filter(|&i| idx[i] < legs[i].levels.len()).collect();
        let payoff = match rule {
            BasketRule::AllLegs { payoff } => {
                if n == 0 || active.len() < n {
                    break;
                }
                payoff
            }
            BasketRule::NestedNo => {
                if active.len() < 2 {
                    break;
                }
                Decimal::from(active.len() as u64) - Decimal::ONE
            }
        };
        let cost: Decimal = active
            .iter()
            .map(|&i| {
                let p = legs[i].levels[idx[i]].0;
                p.get() + legs[i].fee.phi(p)
            })
            .sum();
        let seg = active.iter().map(|&i| rem[i]).min().unwrap_or(Decimal::ZERO);
        if seg <= Decimal::ZERO {
            break;
        }
        out.push(Step { q_from: q, q_to: q + seg, cost_per_basket: cost, payoff_per_basket: payoff, profitable: payoff - cost > Decimal::ZERO });
        q += seg;
        for &i in &active {
            rem[i] -= seg;
            if rem[i] == Decimal::ZERO {
                idx[i] += 1;
                rem[i] = legs[i].levels.get(idx[i]).map_or(Decimal::ZERO, |x| x.1.get());
            }
        }
    }
    out
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p arb-core && cargo clippy -p arb-core --all-targets -- -D warnings`
Expected: PASS, including the existing lemma tests (nothing existing changed).

- [ ] **Step 5: Commit**

```bash
git add crates/arb-core/src/book.rs crates/arb-core/src/fill.rs crates/arb-core/tests/steps.rs
git commit -m "arb-core: d1_sums and walk_steps (the full depth staircase for drawing)"
```

---

### Task 3: `detect::classify`

**Files:**
- Modify: `crates/arb-core/src/detect.rs`
- Test: `crates/arb-core/tests/classify.rs`

**Interfaces:**
- Produces: `arb_core::detect::{Node, Classification { path: Vec<Node>, signal: Option<Signal> }, classify(g: &Group) -> Classification}`.
- `Node` variants, in the order they can appear:
  - `Decided`;
  - `NoBasketTried`, `NoBasketFewLegs`, `NoBasketCrossed`, `NoBasketNotPositive`, `NoBasketPositive`;
  - `YesBasketTried`, `YesBasketNoLive`, `YesBasketCrossed`, `YesBasketMissingAsk`, `YesBasketNotPositive`, `YesBasketPositive`;
  - `Complete`, `Incomplete`.
- `Signal` gains `#[derive(PartialEq, Eq)]`, alongside `Debug, Clone` (Kind, Realization and BasketQuote already have them). Existing callers are unaffected.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/arb-core/tests/classify.rs
//! Phase 7 spec §3.3: classify = the existing detectors plus the path taken (parent §5.7 (c)).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Book;
use arb_core::detect::{Group, GroupLeg, Kind, LegStatus, Node, classify, detect_no_basket, detect_yes_basket};
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use proptest::prelude::*;
use rust_decimal::Decimal;

fn leg(status: LegStatus, bid: Option<&str>, ask: Option<&str>) -> GroupLeg {
    let mut book = Book::default();
    let side = |p: Option<&str>| p.map(|p| vec![(Price::parse(p).unwrap(), Shares::parse("100").unwrap())]).unwrap_or_default();
    book.apply_snapshot(&side(bid), &side(ask), 0);
    GroupLeg { status, book, fee: FeeSchedule::ZERO }
}

#[test]
fn a_no_basket_edge_stops_after_d2b() {
    let g = Group { legs: vec![leg(LegStatus::Live, Some("0.40"), Some("0.41")), leg(LegStatus::Live, Some("0.35"), Some("0.36")), leg(LegStatus::Live, Some("0.30"), Some("0.31"))], augmented: false };
    let c = classify(&g);
    assert_eq!(c.path, vec![Node::NoBasketTried, Node::NoBasketPositive]);
    assert_eq!(c.signal.unwrap().kind, Kind::NoBasket);
}

#[test]
fn an_other_leg_makes_the_yes_basket_incomplete() {
    let g = Group { legs: vec![leg(LegStatus::Live, Some("0.20"), Some("0.30")), leg(LegStatus::Live, Some("0.20"), Some("0.30")), leg(LegStatus::Other, None, None)], augmented: false };
    let c = classify(&g);
    assert_eq!(c.path, vec![Node::NoBasketTried, Node::NoBasketNotPositive, Node::YesBasketTried, Node::YesBasketPositive, Node::Incomplete]);
    assert_eq!(c.signal.unwrap().kind, Kind::YesBasketIncomplete);
}

#[test]
fn a_decided_event_yields_nothing() {
    let g = Group { legs: vec![leg(LegStatus::ClosedYes, None, None), leg(LegStatus::Live, Some("0.2"), Some("0.3"))], augmented: false };
    assert_eq!(classify(&g).path, vec![Node::Decided]);
    assert!(classify(&g).signal.is_none());
}

#[test]
fn a_missing_ask_is_named() {
    let g = Group { legs: vec![leg(LegStatus::Live, Some("0.20"), Some("0.30")), leg(LegStatus::Live, Some("0.20"), None)], augmented: false };
    let c = classify(&g);
    assert_eq!(c.path.last(), Some(&Node::YesBasketMissingAsk));
}

fn status() -> impl Strategy<Value = LegStatus> {
    prop_oneof![Just(LegStatus::Live), Just(LegStatus::Live), Just(LegStatus::Live), Just(LegStatus::Placeholder), Just(LegStatus::Other), Just(LegStatus::ClosedNo), Just(LegStatus::ClosedYes), Just(LegStatus::Halted)]
}

fn side() -> impl Strategy<Value = Vec<(Price, Shares)>> {
    prop::collection::btree_map(1u32..=99, 1u32..=300, 0..=3).prop_map(|m| {
        m.into_iter().map(|(c, s)| (Price::new(Decimal::new(c.into(), 2)).unwrap(), Shares::new(Decimal::from(s)).unwrap())).collect()
    })
}

fn group() -> impl Strategy<Value = Group> {
    (prop::collection::vec((status(), side(), side(), any::<bool>()), 1..=6), any::<bool>()).prop_map(|(legs, augmented)| Group {
        legs: legs
            .into_iter()
            .map(|(status, bids, asks, fee)| {
                let mut book = Book::default();
                book.apply_snapshot(&bids, &asks, 0);
                GroupLeg { status, book, fee: if fee { FeeSchedule::new(Decimal::new(7, 2), 1).unwrap() } else { FeeSchedule::ZERO } }
            })
            .collect(),
        augmented,
    })
}

proptest! {
    #[test]
    fn classify_agrees_with_the_detectors(g in group()) {
        let expected = detect_no_basket(&g).or_else(|| detect_yes_basket(&g));
        prop_assert_eq!(classify(&g).signal, expected);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-core --test classify`
Expected: FAIL to compile, with "no `Node` in `detect`".

- [ ] **Step 3: Implement**

In `crates/arb-core/src/detect.rs`:

1. Change `#[derive(Debug, Clone)] pub struct Signal` to `#[derive(Debug, Clone, PartialEq, Eq)]`.
2. Add the shared guards, placed after `fn decided`:

```rust
/// Legs D2a buys: every live leg.
fn yes_live(g: &Group) -> Vec<usize> {
    (0..g.legs.len()).filter(|&i| g.legs[i].status == LegStatus::Live).collect()
}

/// Legs D2b buys: every live leg with a YES bid.
fn no_used(g: &Group) -> Vec<usize> {
    (0..g.legs.len())
        .filter(|&i| g.legs[i].status == LegStatus::Live && g.legs[i].book.top().best_bid.is_some())
        .collect()
}

fn any_crossed(g: &Group, legs: &[usize]) -> bool {
    legs.iter().any(|&i| g.legs[i].book.is_crossed())
}
```

3. In `detect_yes_basket`, replace the `let live: Vec<usize> = …collect();` block with `let live = yes_live(g);` and `if live.iter().any(|&i| g.legs[i].book.is_crossed())` with `if any_crossed(g, &live)`. In `detect_no_basket`, replace the `let used …` block with `let used = no_used(g);` and its crossed check with `if any_crossed(g, &used)`. Behaviour must not change: the full existing suite is the proof.
4. Append:

```rust
/// One test on the parent §5.7 (c) decision tree, in the order `classify` evaluates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Decided,
    NoBasketTried,
    NoBasketFewLegs,
    NoBasketCrossed,
    NoBasketNotPositive,
    NoBasketPositive,
    YesBasketTried,
    YesBasketNoLive,
    YesBasketCrossed,
    YesBasketMissingAsk,
    YesBasketNotPositive,
    YesBasketPositive,
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub path: Vec<Node>,
    pub signal: Option<Signal>,
}

/// The detectors' answer plus the path taken (Phase 7 spec §3.3). D2b first: when it is positive
/// D2a is not tried (L7: they are never both positive). `post_end` is the scanner's, not ours.
pub fn classify(g: &Group) -> Classification {
    let mut path = Vec::new();
    if decided(g) {
        path.push(Node::Decided);
        return Classification { path, signal: None };
    }
    path.push(Node::NoBasketTried);
    let used = no_used(g);
    if used.len() < 2 {
        path.push(Node::NoBasketFewLegs);
    } else if any_crossed(g, &used) {
        path.push(Node::NoBasketCrossed);
    } else if let Some(sig) = detect_no_basket(g) {
        path.push(Node::NoBasketPositive);
        return Classification { path, signal: Some(sig) };
    } else {
        path.push(Node::NoBasketNotPositive);
    }
    path.push(Node::YesBasketTried);
    let live = yes_live(g);
    if live.is_empty() {
        path.push(Node::YesBasketNoLive);
        return Classification { path, signal: None };
    }
    if any_crossed(g, &live) {
        path.push(Node::YesBasketCrossed);
        return Classification { path, signal: None };
    }
    if live.iter().any(|&i| g.legs[i].book.top().best_ask.is_none()) {
        path.push(Node::YesBasketMissingAsk);
        return Classification { path, signal: None };
    }
    match detect_yes_basket(g) {
        Some(sig) => {
            path.push(Node::YesBasketPositive);
            path.push(if sig.kind == Kind::YesBasket { Node::Complete } else { Node::Incomplete });
            Classification { path, signal: Some(sig) }
        }
        None => {
            path.push(Node::YesBasketNotPositive);
            Classification { path, signal: None }
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p arb-core && cargo test -p sim && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS. `sim` and `bins/scanner` still compile, and their suites are green (the detectors' behaviour is unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/arb-core/src/detect.rs crates/arb-core/tests/classify.rs
git commit -m "arb-core: classify returns the detectors' answer plus the decision path"
```

---

### Task 4: `crates/arb-wasm` (native facade, fixtures, limits)

**Files:**
- Modify: `Cargo.toml` (members += `"crates/arb-wasm"`)
- Create: `crates/arb-wasm/Cargo.toml`, `src/lib.rs`, `src/api.rs`, `src/input.rs`, `fixtures/*.json`, `tests/{fixtures,values,never_panics,no_floats}.rs`

**Interfaces:**
- Consumes: Tasks 1–3, plus `sim::config::Scenario` and `sim::latency::exec_delay_ms`.
- Produces: `arb_wasm::api::{mirror_json, payoff_json, classify_json, depth_walk_json, episode_json}`, each `fn(&str) -> String`. On wasm32, the same functions are exported as `mirror`, `payoff`, `classify`, `depth_walk` and `episode`.
- Input and output JSON shapes, which Tasks 6–8 mirror in TypeScript:

```
Level      {"p":"0.55","s":"100"}
Fee        {"rate":"0.07","exponent":1}
LegIn      {"status":"live|placeholder|other|closed_no|closed_yes|halted","bids":[Level],"asks":[Level],"fee":Fee|null}

mirror     in  {"bids":[Level],"asks":[Level]}
           out {"no_asks":[Level],"no_bids":[Level],"best_bid":Level|null,"best_ask":Level|null,
                "spread":str|null,"d1_long":str|null,"d1_short":str|null,"crossed":bool}
payoff     in  {"kind":"yes|no|implication","s":3,"other_listed":false,"prices":[{"bid":"0.29","ask":"0.30"}]|null}
           out {"rows":[str],"cols":[str],"possible":[bool],"cells":[[str|null]],"basket":[str|null],
                "guaranteed":str,"broken":[bool],"top_edge_gross":str|null}
classify   in  {"augmented":false,"legs":[LegIn]}
           out {"path":[str],"kind":str|null,"realization":str|null,"net":str|null,"gross":str|null,"q_star":str|null}
depth_walk in  {"rule":"yes|no","legs":[{"levels":[Level],"fee":Fee|null}]}
           out {"steps":[{"q_from":str,"q_to":str,"cost":str,"payoff":str,"profitable":bool}],
                "q_star":str,"pi":str,"pi_gross":str}
episode    in  {"kind":"yes|no","gap_ms":30000,"updates":[{"t_ms":0,"legs":[LegIn]}],
                "scenario":{"proc_ms":0,"net_ms":145,"match_ms":150,"honor_seconds_delay":true},"seconds_delay":0|null}
           out {"updates":[{"t_ms":int,"net":str|null}],"open_ms":int|null,"close_ms":int|null,
                "peak_ms":int|null,"peak_net":str|null,"t_x":int|null,"captured":str|null,"caught":bool}
error      {"error":"<one sentence>"}
```

- **`path` names** are snake case of `Node`: `decided`, `no_basket_tried`, `no_basket_few_legs`, `no_basket_crossed`, `no_basket_not_positive`, `no_basket_positive`, `yes_basket_tried`, `yes_basket_no_live`, `yes_basket_crossed`, `yes_basket_missing_ask`, `yes_basket_not_positive`, `yes_basket_positive`, `complete`, `incomplete`.
- **`kind` values:** `yes_basket`, `yes_basket_incomplete`, `no_basket`, `implication`.
- **`realization` values:** `settlement`, `converter`, `not_arbitrage`.

- [ ] **Step 1: Pick the wasm-bindgen version**

Run: `cargo search wasm-bindgen --limit 1`
Record the printed version (e.g. `0.2.1xx`) as `WBG`. It's used verbatim in `crates/arb-wasm/Cargo.toml`, `scripts/build-wasm.sh`, `.github/workflows/ci.yml` and `docker/web.Dockerfile`.

- [ ] **Step 2: Create the crate skeleton and write the failing tests**

```toml
# crates/arb-wasm/Cargo.toml
[package]
name = "arb-wasm"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

# Phase 7 spec §4.1: a JSON facade over arb-core for the public playground. Features are declared
# here explicitly so a lone wasm32 build and `cargo test --workspace` parse and print identically.
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
arb-core = { path = "../arb-core" }
sim = { path = "../sim" }
rust_decimal = { workspace = true, features = ["serde-with-str"] }
serde.workspace = true
serde_json = { workspace = true, features = ["arbitrary_precision"] }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=WBG"

[dev-dependencies]
proptest.workspace = true

[lints]
workspace = true
```

Replace `WBG` with the version from Step 1. Add `"crates/arb-wasm"` to `members` in the root `Cargo.toml`, after `"crates/cards"`.

```rust
// crates/arb-wasm/tests/values.rs
//! Hand-computed expectations for the facade (Phase 7 spec §4.1) and the limits.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{classify_json, depth_walk_json, episode_json, mirror_json, payoff_json};

fn err(out: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(out).unwrap();
    v["error"].as_str().unwrap_or_else(|| panic!("expected an error, got {out}")).to_string()
}

#[test]
fn mirror_reflects_the_fed_book() {
    let out = mirror_json(r#"{"bids":[{"p":"0.54","s":"5603.74"},{"p":"0.53","s":"85887.16"}],"asks":[{"p":"0.55","s":"32356.03"}]}"#);
    assert_eq!(
        out,
        r#"{"no_asks":[{"p":"0.46","s":"5603.74"},{"p":"0.47","s":"85887.16"}],"no_bids":[{"p":"0.45","s":"32356.03"}],"best_bid":{"p":"0.54","s":"5603.74"},"best_ask":{"p":"0.55","s":"32356.03"},"spread":"0.01","d1_long":"1.01","d1_short":"0.99","crossed":false}"#
    );
}

#[test]
fn payoff_yes_with_prices_reports_the_top_edge() {
    let out = payoff_json(r#"{"kind":"yes","s":2,"other_listed":true,"prices":[{"bid":"0.40","ask":"0.45"},{"bid":"0.50","ask":"0.52"}]}"#);
    assert!(out.ends_with(r#""top_edge_gross":"0.03"}"#), "{out}");
}

#[test]
fn payoff_no_top_edge_is_sum_of_bids_minus_one() {
    // L5: Σ_S v(B) − 1 with zero fees = (0.60 + 0.50) − 1 = 0.10.
    let out = payoff_json(r#"{"kind":"no","s":2,"other_listed":true,"prices":[{"bid":"0.60","ask":"0.61"},{"bid":"0.50","ask":"0.51"}]}"#);
    assert!(out.ends_with(r#""top_edge_gross":"0.1"}"#), "{out}");
}

#[test]
fn classify_names_its_path() {
    let out = classify_json(r#"{"augmented":false,"legs":[{"status":"live","bids":[{"p":"0.40","s":"10"}],"asks":[{"p":"0.41","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.35","s":"10"}],"asks":[{"p":"0.36","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.30","s":"10"}],"asks":[{"p":"0.31","s":"10"}],"fee":null}]}"#);
    assert!(out.starts_with(r#"{"path":["no_basket_tried","no_basket_positive"],"kind":"no_basket","realization":"converter""#), "{out}");
}

#[test]
fn depth_walk_draws_past_q_star() {
    let out = depth_walk_json(r#"{"rule":"yes","legs":[{"levels":[{"p":"0.30","s":"10"},{"p":"0.40","s":"10"}],"fee":null},{"levels":[{"p":"0.60","s":"15"}],"fee":null}]}"#);
    assert_eq!(
        out,
        r#"{"steps":[{"q_from":"0","q_to":"10","cost":"0.9","payoff":"1","profitable":true},{"q_from":"10","q_to":"15","cost":"1","payoff":"1","profitable":false}],"q_star":"10","pi":"1","pi_gross":"1"}"#
    );
}

#[test]
fn a_slow_trader_misses_a_short_episode() {
    let open = r#"[{"status":"live","bids":[{"p":"0.60","s":"10"}],"asks":[{"p":"0.61","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.50","s":"10"}],"asks":[{"p":"0.51","s":"10"}],"fee":null}]"#;
    let shut = r#"[{"status":"live","bids":[{"p":"0.40","s":"10"}],"asks":[{"p":"0.41","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.50","s":"10"}],"asks":[{"p":"0.51","s":"10"}],"fee":null}]"#;
    let input = format!(r#"{{"kind":"no","gap_ms":30000,"updates":[{{"t_ms":0,"legs":{shut}}},{{"t_ms":1000,"legs":{open}}},{{"t_ms":1200,"legs":{shut}}}],"scenario":{{"proc_ms":0,"net_ms":145,"match_ms":150,"honor_seconds_delay":false}},"seconds_delay":null}}"#);
    let out = episode_json(&input);
    assert!(out.contains(r#""open_ms":1000,"close_ms":1200"#), "{out}");
    assert!(out.ends_with(r#""t_x":1295,"captured":"0","caught":false}"#), "{out}");
}

#[test]
fn bad_inputs_return_one_sentence_errors() {
    let cases = [
        (r#"{"bids":[{"p":0.55,"s":"1"}],"asks":[]}"#, "not valid input"),
        (r#"{"bids":[{"p":"0.555","s":"1"}],"asks":[]}"#, "steps of 0.01"),
        (r#"{"bids":[{"p":"1.00","s":"1"}],"asks":[]}"#, "steps of 0.01"),
        (r#"{"bids":[{"p":"0","s":"1"}],"asks":[]}"#, "steps of 0.01"),
        (r#"{"bids":[{"p":"0.50","s":"1"},{"p":"0.50","s":"2"}],"asks":[]}"#, "same price"),
        (r#"{"bids":[{"p":"0.50","s":"0"}],"asks":[]}"#, "size"),
        (r#"{"bids":[{"p":"0.50","s":"1000001"}],"asks":[]}"#, "size"),
    ];
    for (input, needle) in cases {
        let e = err(&mirror_json(input));
        assert!(e.contains(needle), "{input} → {e}");
        assert!(!e.contains('\n'));
    }
    assert!(err(&payoff_json(r#"{"kind":"yes","s":1,"other_listed":true,"prices":null}"#)).contains("2 to 6"));
    let seven = format!(r#"{{"rule":"yes","legs":[{}]}}"#, vec![r#"{"levels":[{"p":"0.1","s":"1"}],"fee":null}"#; 7].join(","));
    assert!(err(&depth_walk_json(&seven)).contains("at most 6 legs"));
    let big = format!(r#"{{"bids":[],"asks":[],"pad":"{}"}}"#, "x".repeat(20 * 1024));
    assert!(err(&mirror_json(&big)).contains("16 KiB"));
}
```

```rust
// crates/arb-wasm/tests/fixtures.rs
//! Golden fixtures (Phase 7 spec §4.3). `input` and `output` are the exact strings; the web's
//! parity test runs the same files through the WASM build. UPDATE_FIXTURES=1 rewrites outputs.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{classify_json, depth_walk_json, episode_json, mirror_json, payoff_json};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize)]
struct Case {
    name: String,
    input: String,
    output: String,
}

fn check(func: &str, f: fn(&str) -> String) {
    let path = format!("{}/fixtures/{func}.json", env!("CARGO_MANIFEST_DIR"));
    let mut cases: Vec<Case> = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!cases.is_empty(), "{path} has no cases");
    let update = std::env::var("UPDATE_FIXTURES").is_ok_and(|v| v == "1");
    let mut bad = Vec::new();
    for c in &mut cases {
        let got = f(&c.input);
        if got != c.output {
            if update {
                c.output = got;
            } else {
                bad.push(format!("{}:\n  expected {}\n  got      {}", c.name, c.output, got));
            }
        }
    }
    if update {
        fs::write(&path, serde_json::to_string_pretty(&cases).unwrap() + "\n").unwrap();
    }
    assert!(bad.is_empty(), "{func}:\n{}", bad.join("\n"));
}

#[test] fn mirror() { check("mirror", mirror_json); }
#[test] fn payoff() { check("payoff", payoff_json); }
#[test] fn classify() { check("classify", classify_json); }
#[test] fn depth_walk() { check("depth_walk", depth_walk_json); }
#[test] fn episode() { check("episode", episode_json); }
```

```rust
// crates/arb-wasm/tests/never_panics.rs
//! Phase 7 spec §8: any input up to 16 KiB returns a model or {"error"}, never a panic.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{classify_json, depth_walk_json, episode_json, mirror_json, payoff_json};
use proptest::prelude::*;

fn json_ish() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{0,300}",
        r#"\{"(bids|asks|legs|kind|s|rule|updates)":\[?\{?"?[a-z0-9.\-]{0,8}"?\]?\}"#,
        prop::collection::vec(r#"\{"p":"0\.[0-9]{1,3}","s":"[0-9]{1,8}"\}"#, 0..12).prop_map(|v| format!(r#"{{"bids":[{}],"asks":[]}}"#, v.join(","))),
    ]
}

proptest! {
    #[test]
    fn never_panics(s in json_ish()) {
        for f in [mirror_json, payoff_json, classify_json, depth_walk_json, episode_json] {
            let out = f(&s);
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            prop_assert!(v.is_object());
        }
    }
}
```

Copy `crates/cards/tests/no_floats.rs` to `crates/arb-wasm/tests/no_floats.rs` verbatim and change its first doc line to `//! Phase 7: arb-wasm must never use floating point (same scan as arb-core).`

Create each fixture file with its cases and `"output": ""`, as below. The next step fills in the outputs.

`crates/arb-wasm/fixtures/mirror.json`: cases named
- `fed-book` (input as in `mirror_reflects_the_fed_book`, all 8 levels from parent §5.7 (a));
- `crossed` (`bids 0.56×10`, `asks 0.55×10`);
- `bids-only`;
- `empty`;
- `json-number-price` (`"p":0.55`);
- `off-grid` (`"0.555"`);
- `duplicate-price`;
- `size-zero`;
- `nine-levels` (9 bids).

`payoff.json`:
- `yes-3-other`;
- `yes-3-listed`;
- `no-3-other`;
- `no-2-listed-prices` (as in `payoff_no_top_edge_is_sum_of_bids_minus_one`);
- `yes-2-prices`;
- `implication` (with prices `[{"bid":"0.70","ask":"0.72"},{"bid":"0.40","ask":"0.42"}]`);
- `yes-s1`;
- `no-s7`;
- `prices-wrong-count` (s = 3 with 2 prices).

`classify.json`:
- `no-edge-3` (as in `classify_names_its_path`);
- `yes-incomplete-other`;
- `decided`;
- `missing-ask`;
- `crossed-leg`;
- `nothing` (Σ asks > 1, Σ bids < 1);
- `fees-on` (the same as `no-edge-3` with `"fee":{"rate":"0.07","exponent":1}` on every leg);
- `augmented-complete` (`"augmented":true`, all live);
- `seven-legs`;
- `bad-status` (`"status":"won"`).

`depth_walk.json`:
- `yes-two-legs` (as in `depth_walk_draws_past_q_star`);
- `no-nested-three`;
- `exactly-one` (asks summing to exactly 1.00);
- `fees-on`;
- `zero-depth-leg` (`"levels":[]` on one leg);
- `four-legs-three-levels`;
- `bad-rule` (`"rule":"maybe"`).

`episode.json`:
- `slow-miss` (as in `a_slow_trader_misses_a_short_episode`);
- `fast-catch` (the same with `net_ms` 1 and `match_ms` 50);
- `never-opens`;
- `still-open-at-end`;
- `seconds-delay-honored` (`honor_seconds_delay` true, `seconds_delay` 1);
- `yes-kind`;
- `gap-censor` (`gap_ms` 100 and a 500 ms gap inside the episode);
- `one-update`;
- `non-increasing-t`;
- `latency-too-big` (`net_ms` 5001).

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p arb-wasm`
Expected: FAIL to compile, with "unresolved import `arb_wasm::api`".

- [ ] **Step 4: Implement `input.rs`**

```rust
// crates/arb-wasm/src/input.rs
//! Parsing and limits (Phase 7 spec §4.1). Every check happens before any arb-core call.
use arb_core::book::Book;
use arb_core::detect::{Group, GroupLeg, LegStatus};
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use serde::Deserialize;

pub const MAX_INPUT: usize = 16 * 1024;
pub const MAX_LEGS: usize = 6;
pub const MAX_LEVELS: usize = 8;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelIn {
    pub p: String,
    pub s: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeIn {
    pub rate: String,
    pub exponent: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegIn {
    pub status: String,
    #[serde(default)]
    pub bids: Vec<LevelIn>,
    #[serde(default)]
    pub asks: Vec<LevelIn>,
    #[serde(default)]
    pub fee: Option<FeeIn>,
}

pub fn price(s: &str) -> Result<Price, String> {
    let bad = || format!("price {s:?} must be 0.01 to 0.99 in steps of 0.01");
    let p = Price::parse(s).map_err(|_| bad())?;
    let d = p.get();
    if d < Decimal::new(1, 2) || d > Decimal::new(99, 2) || (d * Decimal::ONE_HUNDRED).fract() != Decimal::ZERO {
        return Err(bad());
    }
    Ok(p)
}

pub fn size(s: &str) -> Result<Shares, String> {
    let bad = || format!("size {s:?} must be above 0 and at most 1,000,000, with at most 6 decimals");
    let q = Shares::parse(s).map_err(|_| bad())?;
    if q.get() <= Decimal::ZERO || q.get() > Decimal::from(1_000_000u32) {
        return Err(bad());
    }
    Ok(q)
}

pub fn levels(v: &[LevelIn]) -> Result<Vec<(Price, Shares)>, String> {
    if v.len() > MAX_LEVELS {
        return Err(format!("at most {MAX_LEVELS} levels per side"));
    }
    let mut out: Vec<(Price, Shares)> = Vec::with_capacity(v.len());
    for l in v {
        let p = price(&l.p)?;
        if out.iter().any(|(q, _)| *q == p) {
            return Err(format!("two levels at the same price {}", l.p));
        }
        out.push((p, size(&l.s)?));
    }
    Ok(out)
}

pub fn fee(f: &Option<FeeIn>) -> Result<FeeSchedule, String> {
    match f {
        None => Ok(FeeSchedule::ZERO),
        Some(f) => {
            let rate = Decimal::from_str_exact(f.rate.trim()).map_err(|_| format!("fee rate {:?} is not a decimal", f.rate))?;
            FeeSchedule::new(rate, f.exponent).map_err(|e| format!("fee: {e}"))
        }
    }
}

pub fn status(s: &str) -> Result<LegStatus, String> {
    Ok(match s {
        "live" => LegStatus::Live,
        "placeholder" => LegStatus::Placeholder,
        "other" => LegStatus::Other,
        "closed_no" => LegStatus::ClosedNo,
        "closed_yes" => LegStatus::ClosedYes,
        "halted" => LegStatus::Halted,
        _ => return Err(format!("unknown leg status {s:?}")),
    })
}

pub fn book(bids: &[LevelIn], asks: &[LevelIn]) -> Result<Book, String> {
    let mut b = Book::default();
    b.apply_snapshot(&levels(bids)?, &levels(asks)?, 0);
    Ok(b)
}

pub fn group(legs: &[LegIn], augmented: bool) -> Result<Group, String> {
    if legs.is_empty() || legs.len() > MAX_LEGS {
        return Err(format!("1 to at most {MAX_LEGS} legs"));
    }
    let legs = legs
        .iter()
        .map(|l| Ok(GroupLeg { status: status(&l.status)?, book: book(&l.bids, &l.asks)?, fee: fee(&l.fee)? }))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Group { legs, augmented })
}

/// Decimal → JSON string: normalized, and zero is always "0".
pub fn d(x: Decimal) -> String {
    if x.is_zero() { "0".to_string() } else { x.normalize().to_string() }
}
```

- [ ] **Step 5: Implement `api.rs` and `lib.rs`**

```rust
// crates/arb-wasm/src/api.rs
//! JSON facade over arb-core (Phase 7 spec §4.1). Parses, checks limits, calls arb-core, prints.
//! It computes no money figure; its one decision is which update is in force at t_x (§4.1).
#![forbid(unsafe_code)]
use crate::input::{LegIn, LevelIn, MAX_INPUT, MAX_LEGS, d, fee, group, levels, price};
use arb_core::book::{Book, d1_sums};
use arb_core::detect::{Kind, Node, Realization, Signal, classify};
use arb_core::episode::{EpisodeEvent, EpisodeTracker};
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk, walk_steps};
use arb_core::money::{Price, Shares};
use arb_core::payoff;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ErrOut<'a> {
    error: &'a str,
}

fn err(m: &str) -> String {
    serde_json::to_string(&ErrOut { error: m }).unwrap_or_else(|_| r#"{"error":"internal"}"#.to_string())
}

fn run<I: DeserializeOwned, O: Serialize>(input: &str, f: impl FnOnce(I) -> Result<O, String>) -> String {
    if input.len() > MAX_INPUT {
        return err("input is larger than 16 KiB");
    }
    let parsed: I = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => return err(&format!("not valid input: {}", e.to_string().replace('\n', " "))),
    };
    match f(parsed).and_then(|o| serde_json::to_string(&o).map_err(|e| e.to_string())) {
        Ok(s) => s,
        Err(m) => err(&m),
    }
}

#[derive(Serialize)]
struct LevelOut {
    p: String,
    s: String,
}

fn lv((p, s): (Price, Shares)) -> LevelOut {
    LevelOut { p: d(p.get()), s: d(s.get()) }
}

// ---- mirror ----
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MirrorIn {
    bids: Vec<LevelIn>,
    asks: Vec<LevelIn>,
}

#[derive(Serialize)]
struct MirrorOut {
    no_asks: Vec<LevelOut>,
    no_bids: Vec<LevelOut>,
    best_bid: Option<LevelOut>,
    best_ask: Option<LevelOut>,
    spread: Option<String>,
    d1_long: Option<String>,
    d1_short: Option<String>,
    crossed: bool,
}

pub fn mirror_json(input: &str) -> String {
    run(input, |i: MirrorIn| {
        let mut b = Book::default();
        b.apply_snapshot(&levels(&i.bids)?, &levels(&i.asks)?, 0);
        let top = b.top();
        let sums = d1_sums(&top);
        Ok(MirrorOut {
            no_asks: b.no_asks_best_first().into_iter().map(lv).collect(),
            no_bids: b.no_bids_best_first().into_iter().map(lv).collect(),
            best_bid: top.best_bid.map(lv),
            best_ask: top.best_ask.map(lv),
            spread: match (top.best_bid, top.best_ask) {
                (Some((bp, _)), Some((ap, _))) => Some(d(ap.get() - bp.get())),
                _ => None,
            },
            d1_long: sums.map(|s| d(s.0)),
            d1_short: sums.map(|s| d(s.1)),
            crossed: arb_core::detect::canary(&b),
        })
    })
}

// ---- payoff ----
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PriceIn {
    bid: String,
    ask: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayoffIn {
    kind: String,
    s: usize,
    other_listed: bool,
    prices: Option<Vec<PriceIn>>,
}

#[derive(Serialize)]
struct PayoffOut {
    rows: Vec<String>,
    cols: Vec<String>,
    possible: Vec<bool>,
    cells: Vec<Vec<Option<String>>>,
    basket: Vec<Option<String>>,
    guaranteed: String,
    broken: Vec<bool>,
    top_edge_gross: Option<String>,
}

fn one(p: &str) -> Result<Vec<(Price, Shares)>, String> {
    Ok(vec![(price(p)?, Shares::new(Decimal::ONE).map_err(|e| e.to_string())?)])
}

pub fn payoff_json(input: &str) -> String {
    run(input, |i: PayoffIn| {
        if i.kind != "implication" && !(2..=6).contains(&i.s) {
            return Err("s must be 2 to 6".to_string());
        }
        let m = match i.kind.as_str() {
            "yes" => payoff::yes_basket(i.s, i.other_listed).map_err(|e| e.to_string())?,
            "no" => payoff::no_basket(i.s, i.other_listed).map_err(|e| e.to_string())?,
            "implication" => payoff::implication(),
            k => return Err(format!("unknown kind {k:?}")),
        };
        let top_edge_gross = match &i.prices {
            None => None,
            Some(ps) => {
                let want = if i.kind == "implication" { 2 } else { i.s };
                if ps.len() != want {
                    return Err(format!("expected {want} prices, got {}", ps.len()));
                }
                // Zero-fee, one-share, top-of-book legs; the rule is the detector's own (§5.3).
                let legs: Vec<BuyLeg> = match i.kind.as_str() {
                    "yes" => ps.iter().map(|p| Ok(BuyLeg { levels: one(&p.ask)?, fee: FeeSchedule::ZERO })).collect::<Result<_, String>>()?,
                    "no" => ps.iter().map(|p| Ok(BuyLeg { levels: vec![(price(&p.bid)?.complement(), Shares::new(Decimal::ONE).map_err(|e| e.to_string())?)], fee: FeeSchedule::ZERO })).collect::<Result<_, String>>()?,
                    _ => vec![
                        BuyLeg { levels: vec![(price(&ps[0].bid)?.complement(), Shares::new(Decimal::ONE).map_err(|e| e.to_string())?)], fee: FeeSchedule::ZERO },
                        BuyLeg { levels: one(&ps[1].ask)?, fee: FeeSchedule::ZERO },
                    ],
                };
                let rule = if i.kind == "no" { BasketRule::NestedNo } else { BasketRule::AllLegs { payoff: Decimal::ONE } };
                Some(d(walk(&legs, rule, None).top_edge_gross))
            }
        };
        let opt = |x: &Option<Decimal>| x.map(d);
        Ok(PayoffOut {
            rows: m.rows,
            cols: m.cols,
            possible: m.possible,
            cells: m.cells.iter().map(|r| r.iter().map(opt).collect()).collect(),
            basket: m.basket.iter().map(opt).collect(),
            guaranteed: d(m.guaranteed),
            broken: m.broken,
            top_edge_gross,
        })
    })
}

// ---- classify ----
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyIn {
    #[serde(default)]
    augmented: bool,
    legs: Vec<LegIn>,
}

#[derive(Serialize)]
struct ClassifyOut {
    path: Vec<&'static str>,
    kind: Option<&'static str>,
    realization: Option<&'static str>,
    net: Option<String>,
    gross: Option<String>,
    q_star: Option<String>,
}

fn node(n: Node) -> &'static str {
    match n {
        Node::Decided => "decided",
        Node::NoBasketTried => "no_basket_tried",
        Node::NoBasketFewLegs => "no_basket_few_legs",
        Node::NoBasketCrossed => "no_basket_crossed",
        Node::NoBasketNotPositive => "no_basket_not_positive",
        Node::NoBasketPositive => "no_basket_positive",
        Node::YesBasketTried => "yes_basket_tried",
        Node::YesBasketNoLive => "yes_basket_no_live",
        Node::YesBasketCrossed => "yes_basket_crossed",
        Node::YesBasketMissingAsk => "yes_basket_missing_ask",
        Node::YesBasketNotPositive => "yes_basket_not_positive",
        Node::YesBasketPositive => "yes_basket_positive",
        Node::Complete => "complete",
        Node::Incomplete => "incomplete",
    }
}

fn kind(k: Kind) -> &'static str {
    match k {
        Kind::YesBasket => "yes_basket",
        Kind::YesBasketIncomplete => "yes_basket_incomplete",
        Kind::NoBasket => "no_basket",
        Kind::Implication => "implication",
    }
}

fn realization(r: Realization) -> &'static str {
    match r {
        Realization::Settlement => "settlement",
        Realization::Converter => "converter",
        Realization::NotArbitrage => "not_arbitrage",
    }
}

pub fn classify_json(input: &str) -> String {
    run(input, |i: ClassifyIn| {
        let c = classify(&group(&i.legs, i.augmented)?);
        let s: Option<&Signal> = c.signal.as_ref();
        Ok(ClassifyOut {
            path: c.path.into_iter().map(node).collect(),
            kind: s.map(|s| kind(s.kind)),
            realization: s.map(|s| realization(s.realization)),
            net: s.map(|s| d(s.quote.pnl)),
            gross: s.map(|s| d(s.gross.pnl_gross)),
            q_star: s.map(|s| d(s.quote.q_max)),
        })
    })
}

// ---- depth walk ----
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkLegIn {
    levels: Vec<LevelIn>,
    fee: Option<crate::input::FeeIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkIn {
    rule: String,
    legs: Vec<WalkLegIn>,
}

#[derive(Serialize)]
struct StepOut {
    q_from: String,
    q_to: String,
    cost: String,
    payoff: String,
    profitable: bool,
}

#[derive(Serialize)]
struct WalkOut {
    steps: Vec<StepOut>,
    q_star: String,
    pi: String,
    pi_gross: String,
}

pub fn depth_walk_json(input: &str) -> String {
    run(input, |i: WalkIn| {
        if i.legs.is_empty() || i.legs.len() > MAX_LEGS {
            return Err(format!("1 to at most {MAX_LEGS} legs"));
        }
        let rule = match i.rule.as_str() {
            "yes" => BasketRule::AllLegs { payoff: Decimal::ONE },
            "no" => BasketRule::NestedNo,
            r => return Err(format!("unknown rule {r:?}")),
        };
        let legs = i.legs.iter().map(|l| Ok(BuyLeg { levels: levels(&l.levels)?, fee: fee(&l.fee)? })).collect::<Result<Vec<_>, String>>()?;
        let w = walk(&legs, rule, None);
        Ok(WalkOut {
            steps: walk_steps(&legs, rule)
                .into_iter()
                .map(|s| StepOut { q_from: d(s.q_from), q_to: d(s.q_to), cost: d(s.cost_per_basket), payoff: d(s.payoff_per_basket), profitable: s.profitable })
                .collect(),
            q_star: d(w.q_max),
            pi: d(w.pnl),
            pi_gross: d(w.pnl_gross),
        })
    })
}

// ---- episode ----
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioIn {
    proc_ms: i64,
    net_ms: i64,
    match_ms: i64,
    honor_seconds_delay: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateIn {
    t_ms: i64,
    legs: Vec<LegIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodeIn {
    kind: String,
    gap_ms: i64,
    updates: Vec<UpdateIn>,
    scenario: ScenarioIn,
    seconds_delay: Option<i32>,
}

#[derive(Serialize)]
struct UpdateOut {
    t_ms: i64,
    net: Option<String>,
}

#[derive(Serialize)]
struct EpisodeOut {
    updates: Vec<UpdateOut>,
    open_ms: Option<i64>,
    close_ms: Option<i64>,
    peak_ms: Option<i64>,
    peak_net: Option<String>,
    t_x: Option<i64>,
    captured: Option<String>,
    caught: bool,
}

pub fn episode_json(input: &str) -> String {
    run(input, |i: EpisodeIn| {
        let want = match i.kind.as_str() {
            "yes" => Kind::YesBasket,
            "no" => Kind::NoBasket,
            k => return Err(format!("unknown kind {k:?}")),
        };
        if !(1..=600_000).contains(&i.gap_ms) {
            return Err("gap_ms must be 1 to 600,000".to_string());
        }
        if !(2..=8).contains(&i.updates.len()) {
            return Err("2 to 8 updates".to_string());
        }
        for w in i.updates.windows(2) {
            if w[1].t_ms <= w[0].t_ms {
                return Err("update times must strictly increase".to_string());
            }
        }
        if i.updates.iter().any(|u| !(0..=600_000).contains(&u.t_ms)) {
            return Err("update times must be 0 to 600,000 ms".to_string());
        }
        let sc = &i.scenario;
        for (n, v) in [("proc_ms", sc.proc_ms), ("net_ms", sc.net_ms), ("match_ms", sc.match_ms)] {
            if !(0..=5000).contains(&v) {
                return Err(format!("{n} must be 0 to 5,000"));
            }
        }
        if i.seconds_delay.is_some_and(|s| !(0..=10).contains(&s)) {
            return Err("seconds_delay must be 0 to 10".to_string());
        }
        let scenario = sim::config::Scenario {
            name: "playground".to_string(),
            proc_ms: sc.proc_ms,
            net_ms: sc.net_ms,
            match_ms: sc.match_ms,
            honor_seconds_delay: sc.honor_seconds_delay,
            burst: Vec::new(),
        };
        let mut tracker = EpisodeTracker::new(i.gap_ms);
        let mut nets: Vec<Option<Decimal>> = Vec::new();
        let (mut open_ms, mut close_ms, mut peak_ms, mut peak_net) = (None, None, None, None);
        for u in &i.updates {
            let c = classify(&group(&u.legs, false)?);
            let net = c.signal.filter(|s| s.kind == want).map(|s| s.quote.pnl);
            nets.push(net);
            for ev in tracker.on_state("e", u.t_ms, net) {
                match ev {
                    EpisodeEvent::Open { ts_ms, pnl, .. } if open_ms.is_none() => {
                        open_ms = Some(ts_ms);
                        peak_ms = Some(ts_ms);
                        peak_net = Some(pnl);
                    }
                    EpisodeEvent::Peak { ts_ms, pnl, .. } if close_ms.is_none() => {
                        peak_ms = Some(ts_ms);
                        peak_net = Some(pnl);
                    }
                    EpisodeEvent::Close { ts_ms, .. } if close_ms.is_none() => close_ms = Some(ts_ms),
                    _ => {}
                }
            }
        }
        let t_x = open_ms.map(|o| o + sim::latency::exec_delay_ms(&scenario, i.seconds_delay, 0));
        // The one decision here (§4.1): the latest update at or before t_x, 0 once closed.
        let captured = t_x.map(|t| {
            if close_ms.is_some_and(|c| c <= t) {
                return Decimal::ZERO;
            }
            i.updates.iter().zip(&nets).rev().find(|(u, _)| u.t_ms <= t).and_then(|(_, n)| *n).unwrap_or(Decimal::ZERO)
        });
        Ok(EpisodeOut {
            updates: i.updates.iter().zip(&nets).map(|(u, n)| UpdateOut { t_ms: u.t_ms, net: n.map(d) }).collect(),
            open_ms,
            close_ms,
            peak_ms,
            peak_net: peak_net.map(d),
            t_x,
            caught: captured.is_some_and(|c| c > Decimal::ZERO),
            captured: captured.map(d),
        })
    })
}
```

```rust
// crates/arb-wasm/src/lib.rs
//! arb-wasm: the public playground's engine (Phase 7 spec §4). All logic is arb-core's; `api` is
//! the JSON facade, and the wasm32-only wrappers below only forward strings.
pub mod api;
mod input;

#[cfg(target_arch = "wasm32")]
mod bindings {
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen]
    pub fn mirror(input: &str) -> String {
        crate::api::mirror_json(input)
    }
    #[wasm_bindgen]
    pub fn payoff(input: &str) -> String {
        crate::api::payoff_json(input)
    }
    #[wasm_bindgen]
    pub fn classify(input: &str) -> String {
        crate::api::classify_json(input)
    }
    #[wasm_bindgen]
    pub fn depth_walk(input: &str) -> String {
        crate::api::depth_walk_json(input)
    }
    #[wasm_bindgen]
    pub fn episode(input: &str) -> String {
        crate::api::episode_json(input)
    }
}
```

In `payoff_json`, the `s` check must accept `s` for implication (it ignores `s`) and name the range `2 to 6`, as `bad_inputs_return_one_sentence_errors` expects.

- [ ] **Step 6: Generate the fixture outputs and review them**

Run: `UPDATE_FIXTURES=1 cargo test -p arb-wasm --test fixtures && git diff --stat crates/arb-wasm/fixtures`
Then open each fixture file and check by hand:
- **mirror.json:** `fed-book` equals the string in `mirror_reflects_the_fed_book` (extended to all 8 levels), and `crossed` has `"crossed":true` with `d1_long` below 1.
- **payoff.json:** `no-2-listed-prices` ends with `"top_edge_gross":"0.1"`.
- **classify.json:** `decided` has `"path":["decided"]`.
- **Every error case:** a single `{"error":…}` sentence.

If any output looks wrong, fix the code, not the fixture.

- [ ] **Step 7: Run everything**

Run: `cargo test -p arb-wasm && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS. The fixtures pass without `UPDATE_FIXTURES`, and `values.rs` and `never_panics.rs` pass.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/arb-wasm
git commit -m "arb-wasm: JSON facade over arb-core with golden fixtures and limits"
```

---

### Task 5: WASM build, tsconfig split and the parity check in CI

**Files:**
- Create: `scripts/build-wasm.sh`, `web/tsconfig.math.json`, `web/src/wasm/load.ts`, `web/src/wasm/parity.test.ts`
- Modify: `.gitignore`, `web/tsconfig.json`, `web/package.json` (scripts only), `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `web/src/wasm/pkg/arb_wasm.js` (generated) with `default init`, `initSync` and `mirror|payoff|classify|depth_walk|episode(input: string): string`.
- Produces: `web/src/wasm/load.ts`:

```ts
export interface Engine { mirror(i: string): string; payoff(i: string): string; classify(i: string): string; depth_walk(i: string): string; episode(i: string): string }
export async function loadEngine(): Promise<Engine>  // rejects if the module or .wasm fails
```

- [ ] **Step 1: Owner step: install wasm-bindgen-cli on the PC**

Stop and ask the owner to run `! cargo install wasm-bindgen-cli --locked --version WBG` (a one-time `cargo install`, which the debug-only rule otherwise forbids). Continue once `wasm-bindgen --version` prints `WBG`.

- [ ] **Step 2: Write the build script and ignore rule**

```sh
#!/bin/sh
# Phase 7 spec §4.2: build crates/arb-wasm for the browser into web/src/wasm/pkg.
# PROFILE=release (Docker, CI; the default) or PROFILE=dev (the PC: debug builds only).
set -eu
cd "$(dirname "$0")/.."
PROFILE="${PROFILE:-release}"
if [ "$PROFILE" = release ]; then
  cargo build -p arb-wasm --target wasm32-unknown-unknown --release
  dir=release
else
  cargo build -p arb-wasm --target wasm32-unknown-unknown
  dir=debug
fi
wasm-bindgen --target web --out-dir web/src/wasm/pkg --out-name arb_wasm \
  "target/wasm32-unknown-unknown/$dir/arb_wasm.wasm"
```

Append to `.gitignore`: `web/src/wasm/pkg/`

- [ ] **Step 3: Write the failing parity test**

```ts
// web/src/wasm/parity.test.ts
// Phase 7 spec §4.3: the WASM build returns exactly the strings the native fixtures hold.
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';

const wasmPath = fileURLToPath(new URL('./pkg/arb_wasm_bg.wasm', import.meta.url));
const jsUrl = new URL('./pkg/arb_wasm.js', import.meta.url).href;
const have = existsSync(wasmPath);
const required = process.env.WASM_REQUIRED === '1';
const FUNCS = ['mirror', 'payoff', 'classify', 'depth_walk', 'episode'] as const;
type Case = { name: string; input: string; output: string };

if (!have && !required) console.info('parity: web/src/wasm/pkg missing, skipped (run scripts/build-wasm.sh)');

describe.skipIf(!have && !required)('native = WASM', () => {
  let mod: Record<string, (s: string) => string> = {};
  beforeAll(async () => {
    expect(have, 'web/src/wasm/pkg is missing: run scripts/build-wasm.sh').toBe(true);
    const m = await import(/* @vite-ignore */ jsUrl);
    m.initSync({ module: readFileSync(wasmPath) });
    mod = m;
  });
  for (const f of FUNCS) {
    it(`${f} fixtures`, () => {
      const file = fileURLToPath(new URL(`../../../crates/arb-wasm/fixtures/${f}.json`, import.meta.url));
      const cases = JSON.parse(readFileSync(file, 'utf8')) as Case[];
      expect(cases.length).toBeGreaterThan(0);
      for (const c of cases) expect(mod[f](c.input), c.name).toBe(c.output);
    });
  }
});
```

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/wasm/parity.test.ts`
Expected: FAIL: "web/src/wasm/pkg is missing".

- [ ] **Step 4: Build and pass**

Run: `PROFILE=dev sh scripts/build-wasm.sh && cd web && WASM_REQUIRED=1 npx vitest run src/wasm/parity.test.ts`
Expected: PASS, 5 tests.

- [ ] **Step 5: Loader, tsconfig split and scripts**

```ts
// web/src/wasm/load.ts
// The only module that touches the generated package (Phase 7 spec §2).
import init, * as pkg from './pkg/arb_wasm.js';
import wasmUrl from './pkg/arb_wasm_bg.wasm?url';

export interface Engine {
  mirror(i: string): string;
  payoff(i: string): string;
  classify(i: string): string;
  depth_walk(i: string): string;
  episode(i: string): string;
}

export async function loadEngine(): Promise<Engine> {
  await init({ module_or_path: wasmUrl });
  return { mirror: pkg.mirror, payoff: pkg.payoff, classify: pkg.classify, depth_walk: pkg.depth_walk, episode: pkg.episode };
}
```

In `web/tsconfig.json` add `"exclude": ["src/math", "src/wasm"]`. Create:

```json
{
  "extends": "./tsconfig.json",
  "include": ["src/math", "src/wasm", "src/schematics", "vite.math.config.ts"],
  "exclude": []
}
```

In `web/package.json` `scripts`:

```json
"build": "tsc --noEmit && tsc --noEmit -p tsconfig.math.json && vite build && vite build -c vite.math.config.ts && node scripts/check-math-bundle.mjs",
"build:dashboard": "tsc --noEmit && vite build",
"test": "vitest run"
```

(`vite.math.config.ts` and `check-math-bundle.mjs` arrive in Task 8. Until then, run `build:dashboard`.)

- [ ] **Step 6: CI**

Replace the `web` job in `.github/workflows/ci.yml` with:

```yaml
  web:
    runs-on: ubuntu-latest
    env:
      WASM_REQUIRED: "1"
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: rustup target add wasm32-unknown-unknown
      - uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/wasm-bindgen
          key: wasm-bindgen-cli-WBG
      - run: command -v wasm-bindgen || cargo install wasm-bindgen-cli --locked --version WBG
      - run: sh scripts/build-wasm.sh
      - run: cargo test -p arb-wasm
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: web/package-lock.json
      - run: npm ci
        working-directory: web
      - run: npm test
        working-directory: web
      - run: npm run build:dashboard
        working-directory: web
```

(Task 8 changes the last step to `npm run build`.)

- [ ] **Step 7: Verify the dashboard is still Rust-free**

Run: `mv web/src/wasm/pkg /tmp/pkg-away && cd web && npm run build:dashboard && npx vitest run && cd .. && mv /tmp/pkg-away web/src/wasm/pkg`
Expected: build OK; vitest passes with the parity suite skipped and the info line printed.

- [ ] **Step 8: Commit**

```bash
git add scripts/build-wasm.sh .gitignore web/tsconfig.json web/tsconfig.math.json web/package.json web/src/wasm/load.ts web/src/wasm/parity.test.ts .github/workflows/ci.yml
git commit -m "wasm: build script, tsconfig split, native = WASM parity in CI"
```

---

### Task 6: Schematic views a, b, c

**Files:**
- Create: `web/src/schematics/models.ts`, `MirrorBook.tsx`, `PayoffTable.tsx`, `ClassifyTree.tsx`, `schematics.css`, `schematics-abc.test.tsx`

**Interfaces:**
- Produces (TypeScript mirrors of Task 4's outputs):

```ts
export type Level = { p: string; s: string };
export interface MirrorModel { no_asks: Level[]; no_bids: Level[]; best_bid: Level | null; best_ask: Level | null; spread: string | null; d1_long: string | null; d1_short: string | null; crossed: boolean }
export interface PayoffModel { rows: string[]; cols: string[]; possible: boolean[]; cells: (string | null)[][]; basket: (string | null)[]; guaranteed: string; broken: boolean[]; top_edge_gross: string | null }
export interface ClassifyModel { path: string[]; kind: string | null; realization: string | null; net: string | null; gross: string | null; q_star: string | null }
export interface StepModel { q_from: string; q_to: string; cost: string; payoff: string; profitable: boolean }
export interface WalkModel { steps: StepModel[]; q_star: string; pi: string; pi_gross: string }
export interface EpisodeModel { updates: { t_ms: number; net: string | null }[]; open_ms: number | null; close_ms: number | null; peak_ms: number | null; peak_net: string | null; t_x: number | null; captured: string | null; caught: boolean }
export type EngineError = { error: string };
export function isError(x: unknown): x is EngineError  // typeof x === 'object' && x !== null && 'error' in x
```

- Components (all props-only, no fetch, no WASM import):
  - `MirrorBook({ bids, asks, model, compact? })`, where `bids`/`asks` are the YES levels shown on the left;
  - `PayoffTable({ model, colLabels?, compact? })`, where `colLabels` overrides column names (the panel uses "an outcome not in the set" for "Other");
  - `ClassifyTree({ model, compact? })`.
- Money and prices are shown with the existing `formatDecimal` from `src/lib/money.ts`; geometry uses `Number()` only.

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/schematics/schematics-abc.test.tsx
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ClassifyTree } from './ClassifyTree';
import { MirrorBook } from './MirrorBook';
import type { ClassifyModel, MirrorModel, PayoffModel } from './models';
import { PayoffTable } from './PayoffTable';

const mirror: MirrorModel = {
  no_asks: [{ p: '0.46', s: '5603.74' }], no_bids: [{ p: '0.45', s: '32356.03' }],
  best_bid: { p: '0.54', s: '5603.74' }, best_ask: { p: '0.55', s: '32356.03' },
  spread: '0.01', d1_long: '1.01', d1_short: '0.99', crossed: false,
};

describe('MirrorBook', () => {
  it('shows each NO price as 1 − p of its YES level and the D1 sums', () => {
    render(<MirrorBook bids={[{ p: '0.54', s: '5603.74' }]} asks={[{ p: '0.55', s: '32356.03' }]} model={mirror} />);
    expect(screen.getByText('0.46')).toBeInTheDocument();
    expect(screen.getByText('(1 − 0.54)')).toBeInTheDocument();
    expect(screen.getByText(/D1 long: 1\.01/)).toBeInTheDocument();
    expect(screen.queryByText('Crossed book. Somewhere a feed is lying.')).toBeNull();
  });
  it('names a crossed book', () => {
    render(<MirrorBook bids={[]} asks={[]} model={{ ...mirror, crossed: true, d1_long: '0.99', d1_short: '1.01' }} />);
    expect(screen.getByText('Crossed book. Somewhere a feed is lying.')).toBeInTheDocument();
  });
});

const payoff: PayoffModel = {
  rows: ['YES_A', 'YES_B'], cols: ['A', 'B', 'Other'], possible: [true, true, true],
  cells: [['1', '0', '0'], ['0', '1', '0']], basket: ['1', '1', '0'], guaranteed: '1',
  broken: [false, false, true], top_edge_gross: null,
};

describe('PayoffTable', () => {
  it('marks the broken column with ✗ and a label', () => {
    render(<PayoffTable model={payoff} />);
    const other = screen.getByRole('columnheader', { name: /Other/ });
    expect(other).toHaveTextContent('✗');
    expect(screen.getByRole('row', { name: /basket/ })).toHaveTextContent('0 ✗');
  });
  it('renames columns for the panel', () => {
    render(<PayoffTable model={payoff} colLabels={{ Other: 'an outcome not in the set' }} />);
    expect(screen.getByRole('columnheader', { name: /an outcome not in the set/ })).toBeInTheDocument();
  });
  it('draws an impossible column as a dash', () => {
    render(<PayoffTable model={{ ...payoff, cols: ['A∧B', 'A∧¬B', '¬A∧B'], possible: [true, false, true], cells: [['0', null, '1'], ['1', null, '1']], basket: ['1', null, '2'], broken: [false, false, false] }} />);
    expect(within(screen.getByRole('row', { name: /basket/ })).getAllByText('—')).toHaveLength(1);
  });
});

describe('ClassifyTree', () => {
  it('lights the path taken and ends in the kind', () => {
    const m: ClassifyModel = { path: ['no_basket_tried', 'no_basket_positive'], kind: 'no_basket', realization: 'converter', net: '0.41', gross: '1.37', q_star: '83' };
    render(<ClassifyTree model={m} />);
    expect(screen.getByText(/D2b: NO basket/).closest('li')).toHaveClass('lit');
    expect(screen.getByText(/D2a: YES basket/).closest('li')).not.toHaveClass('lit');
    expect(screen.getByText(/negrisk_no_basket · converter/)).toBeInTheDocument();
  });
  it('ends in nothing', () => {
    render(<ClassifyTree model={{ path: ['decided'], kind: null, realization: null, net: null, gross: null, q_star: null }} />);
    expect(screen.getByText(/nothing: the event is decided/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npx vitest run src/schematics/schematics-abc.test.tsx`
Expected: FAIL, "Failed to resolve import './ClassifyTree'".

- [ ] **Step 3: Implement**

```ts
// web/src/schematics/models.ts
// TypeScript shapes of arb-wasm's outputs (Phase 7 spec §4.1). Decimals are strings.
export type Level = { p: string; s: string };
export interface MirrorModel { no_asks: Level[]; no_bids: Level[]; best_bid: Level | null; best_ask: Level | null; spread: string | null; d1_long: string | null; d1_short: string | null; crossed: boolean }
export interface PayoffModel { rows: string[]; cols: string[]; possible: boolean[]; cells: (string | null)[][]; basket: (string | null)[]; guaranteed: string; broken: boolean[]; top_edge_gross: string | null }
export interface ClassifyModel { path: string[]; kind: string | null; realization: string | null; net: string | null; gross: string | null; q_star: string | null }
export interface StepModel { q_from: string; q_to: string; cost: string; payoff: string; profitable: boolean }
export interface WalkModel { steps: StepModel[]; q_star: string; pi: string; pi_gross: string }
export interface EpisodeModel { updates: { t_ms: number; net: string | null }[]; open_ms: number | null; close_ms: number | null; peak_ms: number | null; peak_net: string | null; t_x: number | null; captured: string | null; caught: boolean }
export type EngineError = { error: string };

export function isError(x: unknown): x is EngineError {
  return typeof x === 'object' && x !== null && 'error' in x;
}
```

```tsx
// web/src/schematics/MirrorBook.tsx
// Parent §5.7 (a): one book, shown twice. NO prices are arb-core's (model); YES are the input.
import type { Level, MirrorModel } from './models';

export const CROSSED = 'Crossed book. Somewhere a feed is lying.';

function Side({ title, rows, note }: { title: string; rows: Level[]; note?: (l: Level, i: number) => string }) {
  return (
    <div className="sch-side">
      <b>{title}</b>
      <ul>
        {rows.map((l, i) => (
          <li key={`${l.p}-${i}`}>
            <span>{l.p}</span> × <span>{l.s}</span>
            {note && <small> {note(l, i)}</small>}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function MirrorBook({ bids, asks, model, compact = false }: { bids: Level[]; asks: Level[]; model: MirrorModel; compact?: boolean }) {
  return (
    <figure className={compact ? 'sch sch-compact' : 'sch'} aria-label="one book, shown twice">
      <div className="sch-books">
        <div>
          <b className="sch-h">YES token book</b>
          <Side title="asks" rows={[...asks].reverse()} />
          <Side title="bids" rows={bids} />
        </div>
        <div>
          <b className="sch-h">NO token book (mirror)</b>
          <Side title="asks" rows={[...model.no_asks].reverse()} note={(_, i) => `(1 − ${bids[model.no_asks.length - 1 - i]?.p ?? '?'})`} />
          <Side title="bids" rows={model.no_bids} note={(_, i) => `(1 − ${asks[i]?.p ?? '?'})`} />
        </div>
      </div>
      <figcaption>
        {model.spread !== null && <span>spread {model.spread} · </span>}
        <span>D1 long: {model.d1_long ?? '—'}</span> · <span>D1 short: {model.d1_short ?? '—'}</span>
        {model.crossed && <p className="sch-alert">{CROSSED}</p>}
      </figcaption>
    </figure>
  );
}
```

`bids` are best-first (highest first), so `no_asks[k]` is `1 − bids[k]`. In the reversed display, index `i` maps to `no_asks.length − 1 − i`. `no_bids[k]` is `1 − asks[k]`, with `asks` best-first (lowest first).

```tsx
// web/src/schematics/PayoffTable.tsx
// Parent §5.7 (b): rows are holdings, columns the winner; ✗ marks a column the basket loses in.
import type { PayoffModel } from './models';

export function PayoffTable({ model, colLabels = {}, compact = false }: { model: PayoffModel; colLabels?: Record<string, string>; compact?: boolean }) {
  const name = (c: string) => colLabels[c] ?? c;
  const cell = (v: string | null, c: number) => (model.possible[c] ? (v ?? '—') : '—');
  return (
    <div className={compact ? 'scroll sch-compact' : 'scroll'}>
      <table className="sch-payoff">
        <thead>
          <tr>
            <th scope="col">winner →</th>
            {model.cols.map((c, i) => (
              <th key={c} scope="col" className={model.broken[i] ? 'broken' : undefined}>
                {name(c)}
                {model.broken[i] ? ' ✗' : ''}
                {!model.possible[i] ? ' (impossible)' : ''}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {model.rows.map((r, ri) => (
            <tr key={r}>
              <th scope="row">{r}</th>
              {model.cells[ri].map((v, c) => (
                <td key={model.cols[c]}>{cell(v, c)}</td>
              ))}
            </tr>
          ))}
          <tr className="sch-basket" aria-label="basket">
            <th scope="row">basket</th>
            {model.basket.map((v, c) => (
              <td key={model.cols[c]} className={model.broken[c] ? 'broken' : undefined}>
                {model.possible[c] ? `${v ?? '—'}${model.broken[c] ? ' ✗' : ''}` : '—'}
              </td>
            ))}
          </tr>
        </tbody>
      </table>
      <p className="sch-note">
        guaranteed: {model.guaranteed}
        {model.top_edge_gross !== null && <> · edge at the top of the book, before fees: {model.top_edge_gross}</>}
      </p>
    </div>
  );
}
```

```tsx
// web/src/schematics/ClassifyTree.tsx
// Parent §5.7 (c): the decision tree with arb-core's classify path lit.
import type { ClassifyModel } from './models';

const STEPS: { id: string; text: string; nodes: string[] }[] = [
  { id: 'decided', text: 'event decided?', nodes: ['decided'] },
  { id: 'd2b', text: 'D2b: NO basket over legs with a YES bid (valid for any S, L4)', nodes: ['no_basket_tried', 'no_basket_few_legs', 'no_basket_crossed', 'no_basket_not_positive', 'no_basket_positive'] },
  { id: 'd2a', text: 'D2a: YES basket over every live leg', nodes: ['yes_basket_tried', 'yes_basket_no_live', 'yes_basket_crossed', 'yes_basket_missing_ask', 'yes_basket_not_positive', 'yes_basket_positive'] },
  { id: 'set', text: 'is the set complete (no Other, placeholder or halted leg)?', nodes: ['complete', 'incomplete'] },
];

const WHY: Record<string, string> = {
  decided: 'nothing: the event is decided',
  no_basket_few_legs: 'fewer than two legs have a YES bid',
  no_basket_crossed: 'a crossed book: our state is wrong (L2)',
  no_basket_not_positive: 'Σ bids − 1 is not positive',
  yes_basket_no_live: 'no live leg',
  yes_basket_crossed: 'a crossed book: our state is wrong (L2)',
  yes_basket_missing_ask: 'a live leg has no ask',
  yes_basket_not_positive: '1 − Σ asks is not positive',
};

export function ClassifyTree({ model, compact = false }: { model: ClassifyModel; compact?: boolean }) {
  const last = model.path[model.path.length - 1];
  const end = model.kind ? `negrisk_${model.kind} · ${model.realization}` : `nothing${WHY[last] && last !== 'decided' ? `: ${WHY[last]}` : ''}`;
  return (
    <figure className={compact ? 'sch sch-compact' : 'sch'} aria-label="classification of one update">
      <ol className="sch-tree">
        {STEPS.map((s) => (
          <li key={s.id} className={s.nodes.some((n) => model.path.includes(n)) ? 'lit' : undefined}>
            {s.text}
            {s.nodes.filter((n) => n !== 'decided' && model.path.includes(n) && WHY[n]).map((n) => (
              <small key={n}> → {WHY[n]}</small>
            ))}
          </li>
        ))}
      </ol>
      <figcaption>
        <b>{model.path.includes('decided') ? WHY.decided : end}</b>
        {model.net !== null && <> · net {model.net} · gross {model.gross} · Q* {model.q_star}</>}
        <p className="sch-note">post_end is added by the scanner from event dates and never changes the kind.</p>
      </figcaption>
    </figure>
  );
}
```

```css
/* web/src/schematics/schematics.css: shared by /math and the explain panel. Tokens from theme.css. */
.sch { margin: 0; display: grid; gap: 8px; }
.sch-books { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.sch-side ul { list-style: none; margin: 0; padding: 0; }
.sch-h { font-family: var(--px); font-size: 8px; }
.sch-alert { color: var(--bad); font-weight: 600; }
.sch-note { font-size: 14px; opacity: 0.85; }
.sch-payoff th.broken, .sch-payoff td.broken { color: var(--bad); }
.sch-basket th, .sch-basket td { border-top: 2px solid var(--line); font-weight: 600; }
.sch-tree { margin: 0; padding-left: 18px; }
.sch-tree li { opacity: 0.55; }
.sch-tree li.lit { opacity: 1; font-weight: 600; }
.sch-compact { font-size: 14px; }
.sch-svg { width: 100%; height: auto; shape-rendering: crispEdges; }
@media (max-width: 640px) { .sch-books { grid-template-columns: minmax(0, 1fr); } }
```

Check that `--bad` exists in `theme.css`'s `:root`. If the alert token has another name there (`grep -n "\-\-bad\|--salmon\|--good" web/src/theme.css`), use that name.

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && npx vitest run src/schematics && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/schematics
git commit -m "web: schematic views a-c (mirror book, payoff table, classify tree)"
```

---

### Task 7: Schematic views d, e

**Files:**
- Create: `web/src/schematics/DepthWalk.tsx`, `EpisodeTimeline.tsx`, `schematics-de.test.tsx`

**Interfaces:**
- Consumes: `WalkModel`, `EpisodeModel` from `models.ts`.
- Produces:
  - `DepthWalk({ model, caption?, compact? })`: an SVG staircase with one `<rect data-step={i} data-profitable>` per step, a `<line data-role="payoff">` at payoff, and `<line data-role="qstar">` at Q*;
  - `EpisodeTimeline({ model, latencyLabel?, caption?, compact? })`: an SVG with one `<circle data-role="update">` per update, `<rect data-role="episode">` from open to close (or the last update), and `<line data-role="tx">` at `t_x`, plus the text "caught" or "missed".

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/schematics/schematics-de.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { DepthWalk } from './DepthWalk';
import { EpisodeTimeline } from './EpisodeTimeline';

describe('DepthWalk', () => {
  const model = {
    steps: [
      { q_from: '0', q_to: '10', cost: '0.9', payoff: '1', profitable: true },
      { q_from: '10', q_to: '15', cost: '1', payoff: '1', profitable: false },
    ],
    q_star: '10', pi: '1', pi_gross: '1',
  };
  it('draws every step, marks Q* and says where it stops', () => {
    const { container } = render(<DepthWalk model={model} />);
    expect(container.querySelectorAll('rect[data-step]')).toHaveLength(2);
    expect(container.querySelector('rect[data-step="1"]')?.getAttribute('data-profitable')).toBe('false');
    expect(container.querySelector('line[data-role="qstar"]')).not.toBeNull();
    expect(screen.getByText(/Q\* 10 · Π 1/)).toBeInTheDocument();
  });
  it('says so when nothing is profitable', () => {
    render(<DepthWalk model={{ steps: [{ q_from: '0', q_to: '5', cost: '1.02', payoff: '1', profitable: false }], q_star: '0', pi: '0', pi_gross: '0' }} />);
    expect(screen.getByText('Unprofitable from the first share.')).toBeInTheDocument();
  });
  it('shows a caption when given one', () => {
    render(<DepthWalk model={model} caption="simplified: per-level steps are not stored" />);
    expect(screen.getByText('simplified: per-level steps are not stored')).toBeInTheDocument();
  });
});

describe('EpisodeTimeline', () => {
  const model = {
    updates: [{ t_ms: 0, net: null }, { t_ms: 1000, net: '0.4' }, { t_ms: 1200, net: null }],
    open_ms: 1000, close_ms: 1200, peak_ms: 1000, peak_net: '0.4', t_x: 1295, captured: '0', caught: false,
  };
  it('draws updates, the episode and t_x, and says missed', () => {
    const { container } = render(<EpisodeTimeline model={model} latencyLabel="295 ms" />);
    expect(container.querySelectorAll('circle[data-role="update"]')).toHaveLength(3);
    expect(container.querySelector('rect[data-role="episode"]')).not.toBeNull();
    expect(container.querySelector('line[data-role="tx"]')).not.toBeNull();
    expect(screen.getByText(/missed/)).toBeInTheDocument();
    expect(screen.getByText(/lasted 200 ms/)).toBeInTheDocument();
  });
  it('without an episode draws no t_x', () => {
    const { container } = render(<EpisodeTimeline model={{ ...model, open_ms: null, close_ms: null, peak_ms: null, peak_net: null, t_x: null, captured: null }} />);
    expect(container.querySelector('line[data-role="tx"]')).toBeNull();
    expect(screen.getByText('No episode: nothing was net-positive.')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npx vitest run src/schematics/schematics-de.test.tsx`
Expected: FAIL, "Failed to resolve import './DepthWalk'".

- [ ] **Step 3: Implement**

```tsx
// web/src/schematics/DepthWalk.tsx
// Parent §5.7 (d), L9: cost per basket against the payoff, Π shaded, Q* marked. Geometry only uses
// Number(); every figure printed is the model's decimal string.
import type { WalkModel } from './models';

const W = 320;
const H = 160;
const PAD = 24;

export function DepthWalk({ model, caption, compact = false }: { model: WalkModel; caption?: string; compact?: boolean }) {
  const qMax = Math.max(1, ...model.steps.map((s) => Number(s.q_to)));
  const ys = model.steps.flatMap((s) => [Number(s.cost), Number(s.payoff)]);
  const lo = Math.min(...ys, 1) - 0.02;
  const hi = Math.max(...ys, 1) + 0.02;
  const x = (q: number) => PAD + ((W - 2 * PAD) * q) / qMax;
  const y = (v: number) => H - PAD - ((H - 2 * PAD) * (v - lo)) / (hi - lo || 1);
  const none = model.steps.length > 0 && !model.steps[0].profitable;
  return (
    <figure className={compact ? 'sch sch-compact' : 'sch'} aria-label="depth walk">
      <svg className="sch-svg" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`cost per basket against payoff, Q* ${model.q_star}`}>
        {model.steps.map((s, i) => {
          const x0 = x(Number(s.q_from));
          const x1 = x(Number(s.q_to));
          const top = Math.min(y(Number(s.cost)), y(Number(s.payoff)));
          const h = Math.abs(y(Number(s.cost)) - y(Number(s.payoff)));
          return (
            <g key={i}>
              <rect data-step={i} data-profitable={String(s.profitable)} x={x0} y={top} width={Math.max(1, x1 - x0)} height={Math.max(1, h)} className={s.profitable ? 'sch-pi' : 'sch-loss'} />
              <line x1={x0} x2={x1} y1={y(Number(s.cost))} y2={y(Number(s.cost))} className="sch-cost" />
            </g>
          );
        })}
        <line data-role="payoff" x1={PAD} x2={W - PAD} y1={y(1)} y2={y(1)} className="sch-payoff-line" />
        <line data-role="qstar" x1={x(Number(model.q_star))} x2={x(Number(model.q_star))} y1={PAD} y2={H - PAD} className="sch-marker" />
      </svg>
      <figcaption>
        {none ? 'Unprofitable from the first share.' : `Q* ${model.q_star} · Π ${model.pi} (before fees ${model.pi_gross})`}
        {caption && <p className="sch-note">{caption}</p>}
      </figcaption>
    </figure>
  );
}
```

```tsx
// web/src/schematics/EpisodeTimeline.tsx
// Parent §5.7 (e): updates, the episode, and where a trader at t_x lands.
import type { EpisodeModel } from './models';

const W = 320;
const H = 90;
const PAD = 20;

export function EpisodeTimeline({ model, latencyLabel, caption, compact = false }: { model: EpisodeModel; latencyLabel?: string; caption?: string; compact?: boolean }) {
  const ts = [...model.updates.map((u) => u.t_ms), model.t_x ?? 0];
  const t0 = Math.min(...ts);
  const t1 = Math.max(...ts, t0 + 1);
  const x = (t: number) => PAD + ((W - 2 * PAD) * (t - t0)) / (t1 - t0);
  const end = model.close_ms ?? model.updates[model.updates.length - 1]?.t_ms ?? t0;
  return (
    <figure className={compact ? 'sch sch-compact' : 'sch'} aria-label="episode and latency">
      <svg className="sch-svg" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="episode timeline">
        <line x1={PAD} x2={W - PAD} y1={H / 2} y2={H / 2} className="sch-axis" />
        {model.open_ms !== null && <rect data-role="episode" x={x(model.open_ms)} y={H / 2 - 8} width={Math.max(2, x(end) - x(model.open_ms))} height={16} className="sch-pi" />}
        {model.updates.map((u) => (
          <circle key={u.t_ms} data-role="update" cx={x(u.t_ms)} cy={H / 2} r={4} className={u.net !== null ? 'sch-dot-on' : 'sch-dot'} />
        ))}
        {model.t_x !== null && <line data-role="tx" x1={x(model.t_x)} x2={x(model.t_x)} y1={8} y2={H - 8} className="sch-marker" />}
      </svg>
      <figcaption>
        {model.open_ms === null ? (
          'No episode: nothing was net-positive.'
        ) : (
          <>
            {model.close_ms !== null ? `lasted ${model.close_ms - model.open_ms} ms` : 'still open at the last update'}
            {model.peak_net !== null && ` · peak net ${model.peak_net}`}
            {model.t_x !== null && ` · a trader${latencyLabel ? ` at ${latencyLabel}` : ''} arrives at +${model.t_x - model.open_ms} ms: `}
            <b>{model.caught ? `caught ${model.captured}` : 'missed'}</b>
          </>
        )}
        {caption && <p className="sch-note">{caption}</p>}
      </figcaption>
    </figure>
  );
}
```

Append to `schematics.css`:

```css
.sch-pi { fill: var(--good); opacity: 0.45; }
.sch-loss { fill: var(--bad); opacity: 0.35; }
.sch-cost { stroke: var(--fg); stroke-width: 2; }
.sch-payoff-line, .sch-axis { stroke: var(--line); stroke-width: 2; }
.sch-marker { stroke: var(--accent); stroke-width: 2; stroke-dasharray: 4 3; }
.sch-dot { fill: var(--bg); stroke: var(--fg); stroke-width: 2; }
.sch-dot-on { fill: var(--good); stroke: var(--fg); stroke-width: 2; }
```

Use the token names from `theme.css`'s `:root` (check with `grep -n "^  --" web/src/theme.css`) and substitute the nearest existing tokens if a name differs.

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && npx vitest run src/schematics && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/schematics
git commit -m "web: schematic views d-e (depth walk, episode timeline)"
```

---

### Task 8: The math page and its build

**Files:**
- Create: `web/math.html`, `web/vite.math.config.ts`, `web/scripts/check-math-bundle.mjs`, `web/src/math/{main.tsx,MathPage.tsx,copy.ts,defaults.ts,math.css}`, `web/src/math/sections/{SectionA,SectionB,SectionC,SectionD,SectionE}.tsx`, `web/src/math/MathPage.test.tsx`, `web/src/math/guard.test.ts`
- Modify: `.github/workflows/ci.yml` (last web step → `npm run build`)

**Interfaces:**
- Consumes: `loadEngine(): Promise<Engine>` (Task 5); schematics (Tasks 6–7).
- Produces:
  - `MathPage({ engine }: { engine: Promise<Engine> })`. Tests inject a real or failing engine; `main.tsx` passes `loadEngine()`;
  - `noDashboardCode(): Plugin` exported from `vite.math.config.ts`.
- Section props: `{ engine: Engine }`. Each section holds its input state, calls `engine.<fn>(JSON.stringify(input))` in a `useMemo` over the state (synchronous and pure), `JSON.parse`s the result, and shows either the schematic or the error sentence.

- [ ] **Step 1: Write the failing tests**

```ts
// web/src/math/guard.test.ts
import { describe, expect, it, vi } from 'vitest';
import { noDashboardCode } from '../../vite.math.config';

describe('noDashboardCode', () => {
  const run = (ids: string[]) => {
    const error = vi.fn((m: string) => { throw new Error(m); });
    const plugin = noDashboardCode() as { generateBundle: (this: unknown, o: unknown, b: unknown) => void };
    plugin.generateBundle.call({ error }, {}, { 'a.js': { type: 'chunk', moduleIds: ids } });
  };
  it('accepts math, wasm and schematics', () => {
    expect(() => run(['C:\\x\\web\\src\\math\\MathPage.tsx', '/x/web/src/schematics/PayoffTable.tsx', '/x/web/src/lib/money.ts'])).not.toThrow();
  });
  it('rejects pages, api, explain and help.ts', () => {
    for (const bad of ['/x/web/src/pages/Radar.tsx', 'C:\\x\\web\\src\\api\\client.ts', '/x/web/src/explain/Explain.tsx', '/x/web/src/help.ts']) {
      expect(() => run([bad])).toThrow(/dashboard module/);
    }
  });
});
```

```tsx
// web/src/math/MathPage.test.tsx
import type { ReactElement } from 'react';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';

const wasmPath = fileURLToPath(new URL('../wasm/pkg/arb_wasm_bg.wasm', import.meta.url));
const jsUrl = new URL('../wasm/pkg/arb_wasm.js', import.meta.url).href;
const pageUrl = new URL('./MathPage.tsx', import.meta.url).href;
const have = existsSync(wasmPath);
const required = process.env.WASM_REQUIRED === '1';

describe('MathPage without an engine', () => {
  it('shows_a_sentence_when_the_engine_fails_to_load', async () => {
    const { MathPage } = await import(/* @vite-ignore */ pageUrl);
    render(<MathPage engine={Promise.reject(new Error('blocked'))} />);
    expect(await screen.findByText('The math engine did not load. Your browser may be too old, or something blocked it.')).toBeInTheDocument();
  });
});

describe.skipIf(!have && !required)('MathPage with the real engine', () => {
  let MathPage: (p: { engine: Promise<unknown> }) => ReactElement;
  let engine: Promise<unknown>;
  beforeAll(async () => {
    const m = await import(/* @vite-ignore */ jsUrl);
    m.initSync({ module: readFileSync(wasmPath) });
    engine = Promise.resolve({ mirror: m.mirror, payoff: m.payoff, classify: m.classify, depth_walk: m.depth_walk, episode: m.episode });
    ({ MathPage } = await import(/* @vite-ignore */ pageUrl));
  });
  it('renders the five sections from the spec examples', async () => {
    render(<MathPage engine={engine} />);
    for (const h of ['a · one book, shown twice', 'b · payoff tables', 'c · classifying one update', 'd · depth walk', 'e · episode and latency']) {
      expect(await screen.findByRole('heading', { name: h })).toBeInTheDocument();
    }
    expect(screen.getByText(/D1 long: 1\.01/)).toBeInTheDocument();
    expect(screen.getByText('The live dashboard is private. The math is not.')).toBeInTheDocument();
  });
  it('crossing the book updates the drawing', async () => {
    render(<MathPage engine={engine} />);
    fireEvent.click(await screen.findByRole('button', { name: 'cross the book' }));
    expect(screen.getByText('Crossed book. Somewhere a feed is lying.')).toBeInTheDocument();
  });
  it('an invalid S shows the engine sentence, not a blank', async () => {
    render(<MathPage engine={engine} />);
    fireEvent.change(await screen.findByLabelText('outcomes in the set (S)'), { target: { value: '1' } });
    expect(screen.getByRole('alert')).toHaveTextContent(/2 to 6/);
  });
  it('a slower trader misses the example episode', async () => {
    // Example: open at 400 ms, close at 1900 ms. 1000 + 1000 ms puts t_x at 2400 ms, after the close.
    render(<MathPage engine={engine} />);
    fireEvent.change(await screen.findByLabelText('net ms'), { target: { value: '1000' } });
    fireEvent.change(screen.getByLabelText('match ms'), { target: { value: '1000' } });
    expect(screen.getByText('missed')).toBeInTheDocument();
  });
});
```

The page test imports via a variable URL on purpose, because Vite must not resolve `pkg` when it's absent (Task 5).

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math`
Expected: FAIL: `vite.math.config` and `MathPage.tsx` not found.

- [ ] **Step 3: Build config, guard and size check**

```ts
// web/vite.math.config.ts
/// <reference types="vitest/config" />
// Phase 7 spec §5.1: the public /math page, a separate build into dist/math-assets/.
import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';

const DASHBOARD = /\/src\/(pages|api|explain)\/|\/src\/help\.ts$/;

/** Fails the build if any dashboard module reaches the public bundle (spec §4.4). */
export function noDashboardCode(): Plugin {
  return {
    name: 'no-dashboard-code',
    generateBundle(_opts, bundle) {
      for (const chunk of Object.values(bundle)) {
        if (chunk.type !== 'chunk') continue;
        const ids: string[] = (chunk as { moduleIds?: string[]; modules?: Record<string, unknown> }).moduleIds ?? Object.keys((chunk as { modules?: Record<string, unknown> }).modules ?? {});
        for (const id of ids) {
          const p = id.replace(/\\/g, '/');
          if (DASHBOARD.test(p)) this.error(`math bundle includes dashboard module ${p}`);
        }
      }
    },
  };
}

export default defineConfig({
  plugins: [react(), noDashboardCode()],
  build: {
    outDir: 'dist',
    emptyOutDir: false,
    assetsDir: 'math-assets',
    modulePreload: { polyfill: false },
    sourcemap: false,
    rollupOptions: { input: 'math.html' },
  },
});
```

```js
// web/scripts/check-math-bundle.mjs
// Phase 7 spec §4.4 + §5.2: fail above 350 KB gzip, and write .gz files for nginx gzip_static.
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';

const DIR = new URL('../dist/math-assets/', import.meta.url);
const LIMIT = 350 * 1024;
let total = 0;
for (const name of readdirSync(DIR)) {
  if (name.endsWith('.gz')) continue;
  const path = join(DIR.pathname.replace(/^\/([A-Za-z]:)/, '$1'), name);
  if (!statSync(path).isFile()) continue;
  const gz = gzipSync(readFileSync(path), { level: 9 });
  writeFileSync(`${path}.gz`, gz);
  total += gz.length;
  console.log(`${name}: ${gz.length} B gzip`);
}
console.log(`math-assets total: ${total} B gzip (limit ${LIMIT})`);
if (total === 0) { console.error('no math assets found'); process.exit(1); }
if (total > LIMIT) { console.error('over the 350 KB gzip budget'); process.exit(1); }
```

```html
<!-- web/math.html -->
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="description" content="Arbitrage math on Polymarket NegRisk events, computed by the same Rust code as the scanner." />
    <title>isoca-radar · math</title>
    <link rel="icon" href="data:," />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=Press+Start+2P&family=VT323&display=swap" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/math/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 4: Page, copy, defaults, sections**

```ts
// web/src/math/copy.ts
export const FOOTER = 'The live dashboard is private. The math is not.';
export const ENGINE_FAILED = 'The math engine did not load. Your browser may be too old, or something blocked it.';
export const LOADING = 'Loading the math engine…';
export const QUIPS = {
  a: 'Same book, twice. If both sides ever look like free money, the free money is a bug.',
  b: 'A basket is only risk-free if nothing outside it can win.',
  c: 'Most updates end in "nothing". That is the correct answer most of the time.',
  d: 'The first shares are the cheap ones. The walk stops the moment the next basket costs more than it pays.',
  e: 'The edge was real. It just did not wait for you.',
};
export const CAPTIONS = {
  a: 'Computed by arb_core::book (d1_sums, canary).',
  b: 'Computed by arb_core::payoff and fill::walk.',
  c: 'Computed by arb_core::detect::classify.',
  d: 'Computed by arb_core::fill::walk_steps and walk.',
  e: 'Teaching model: one taker at t_x, no queue, no consumed liquidity, no unwinds. The Paper page simulates the rest. Computed by detect::classify, episode::EpisodeTracker and sim::latency.',
};
```

```ts
// web/src/math/defaults.ts
// Starting states (spec §6.2): the parent §5.7 examples.
import type { Level } from '../schematics/models';

export const FED_BIDS: Level[] = [{ p: '0.54', s: '5603.74' }, { p: '0.53', s: '85887.16' }, { p: '0.52', s: '42239.53' }];
export const FED_ASKS: Level[] = [{ p: '0.55', s: '32356.03' }, { p: '0.56', s: '252281.27' }, { p: '0.57', s: '23963.36' }, { p: '0.58', s: '5637.30' }];

export type LegState = { status: string; bid: string; ask: string };
export const CLASSIFY_LEGS: LegState[] = [
  { status: 'live', bid: '0.40', ask: '0.41' },
  { status: 'live', bid: '0.35', ask: '0.36' },
  { status: 'live', bid: '0.30', ask: '0.31' },
];

export const WALK_LEGS: Level[][] = [
  [{ p: '0.30', s: '40' }, { p: '0.32', s: '60' }, { p: '0.35', s: '100' }],
  [{ p: '0.31', s: '50' }, { p: '0.33', s: '50' }, { p: '0.36', s: '100' }],
  [{ p: '0.33', s: '30' }, { p: '0.34', s: '70' }, { p: '0.37', s: '100' }],
];

/** Five updates of a two-leg NO basket: u1 ≤ 0, u2 +, u3 peak, u4 +, u5 ≤ 0 (parent §5.7 (e)). */
export const EPISODE_BIDS: [string, string][] = [['0.50', '0.49'], ['0.55', '0.50'], ['0.60', '0.52'], ['0.54', '0.50'], ['0.50', '0.49']];
export const EPISODE_T = [0, 400, 900, 1400, 1900];

export const PRESETS = [
  { name: 'colo-idle', proc_ms: 0, net_ms: 1, match_ms: 50 },
  { name: 'sp-idle', proc_ms: 0, net_ms: 145, match_ms: 150 },
  { name: 'sp-burst', proc_ms: 0, net_ms: 145, match_ms: 300 },
];
```

Check that the `PRESETS` values match `config/policies.toml` exactly (`grep -n "proc_ms\|net_ms\|match_ms\|name" config/policies.toml`), and fix any that differ.

```tsx
// web/src/math/MathPage.tsx
import { useEffect, useState } from 'react';
import type { Engine } from '../wasm/load';
import { ENGINE_FAILED, FOOTER, LOADING } from './copy';
import { SectionA } from './sections/SectionA';
import { SectionB } from './sections/SectionB';
import { SectionC } from './sections/SectionC';
import { SectionD } from './sections/SectionD';
import { SectionE } from './sections/SectionE';

export function MathPage({ engine }: { engine: Promise<Engine> }) {
  const [e, setE] = useState<Engine | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    engine.then((x) => live && setE(x), () => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [engine]);
  return (
    <div className="app math">
      <header className="top">
        <span className="logo">♥ isoca-radar · math</span>
      </header>
      <main className="body">
        {failed && <p role="alert">{ENGINE_FAILED}</p>}
        {!failed && !e && <p>{LOADING}</p>}
        {e && (
          <>
            <SectionA engine={e} />
            <SectionB engine={e} />
            <SectionC engine={e} />
            <SectionD engine={e} />
            <SectionE engine={e} />
          </>
        )}
        <p className="quip">{FOOTER}</p>
      </main>
    </div>
  );
}
```

```tsx
// web/src/math/main.tsx
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../theme.css';
import '../schematics/schematics.css';
import './math.css';
import { loadEngine } from '../wasm/load';
import { MathPage } from './MathPage';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <MathPage engine={loadEngine()} />
    </StrictMode>,
  );
}
```

Every section follows the same shape; here are all five in full:

```tsx
// web/src/math/sections/SectionA.tsx
import { useMemo, useState } from 'react';
import type { Engine } from '../../wasm/load';
import { MirrorBook } from '../../schematics/MirrorBook';
import { isError, type Level, type MirrorModel } from '../../schematics/models';
import { CAPTIONS, QUIPS } from '../copy';
import { FED_ASKS, FED_BIDS } from '../defaults';

const cents = (p: string) => Math.round(Number(p) * 100);
const price = (c: number) => (Math.min(99, Math.max(1, c)) / 100).toFixed(2);

export function SectionA({ engine }: { engine: Engine }) {
  const [bids, setBids] = useState<Level[]>(FED_BIDS);
  const [asks, setAsks] = useState<Level[]>(FED_ASKS);
  const out = useMemo(() => JSON.parse(engine.mirror(JSON.stringify({ bids, asks }))) as MirrorModel | { error: string }, [engine, bids, asks]);
  const setP = (side: 'b' | 'a', i: number, c: number) => (side === 'b' ? setBids : setAsks)((ls) => ls.map((l, k) => (k === i ? { ...l, p: price(c) } : l)));
  // Best bid one cent above the best ask; drop other bids at or above it (no duplicate prices).
  const crossBook = () => setBids((ls) => {
    const p = price(cents(asks[0].p) + 1);
    return [{ ...ls[0], p }, ...ls.slice(1).filter((l) => cents(l.p) < cents(p))];
  });
  return (
    <section className="box">
      <h2>a · one book, shown twice</h2>
      {isError(out) ? <p role="alert">{out.error}</p> : <MirrorBook bids={bids} asks={asks} model={out} />}
      <div className="controls">
        {bids.map((l, i) => (
          <label key={`b${i}`}>bid {i + 1} <input type="range" min={1} max={99} value={cents(l.p)} onChange={(e) => setP('b', i, Number(e.target.value))} /> {l.p}</label>
        ))}
        {asks.map((l, i) => (
          <label key={`a${i}`}>ask {i + 1} <input type="range" min={1} max={99} value={cents(l.p)} onChange={(e) => setP('a', i, Number(e.target.value))} /> {l.p}</label>
        ))}
        <button type="button" onClick={crossBook}>cross the book</button>
        <button type="button" onClick={() => { setBids(FED_BIDS); setAsks(FED_ASKS); }}>reset</button>
      </div>
      <p className="quip">{QUIPS.a}</p>
      <p className="sch-note">{CAPTIONS.a}</p>
    </section>
  );
}
```

```tsx
// web/src/math/sections/SectionB.tsx
import { useMemo, useState } from 'react';
import type { Engine } from '../../wasm/load';
import { isError, type PayoffModel } from '../../schematics/models';
import { PayoffTable } from '../../schematics/PayoffTable';
import { CAPTIONS, QUIPS } from '../copy';

export function SectionB({ engine }: { engine: Engine }) {
  const [kind, setKind] = useState<'yes' | 'no' | 'implication'>('yes');
  const [s, setS] = useState(3);
  const [listed, setListed] = useState(false);
  const [withPrices, setWithPrices] = useState(false);
  const n = kind === 'implication' ? 2 : s;
  const prices = withPrices ? Array.from({ length: n }, (_, i) => ({ bid: (0.3 - i * 0.02).toFixed(2), ask: (0.31 - i * 0.02).toFixed(2) })) : null;
  const out = useMemo(() => JSON.parse(engine.payoff(JSON.stringify({ kind, s, other_listed: listed, prices }))) as PayoffModel | { error: string }, [engine, kind, s, listed, withPrices]);
  return (
    <section className="box">
      <h2>b · payoff tables</h2>
      <div className="controls">
        {(['yes', 'no', 'implication'] as const).map((k) => (
          <button key={k} type="button" aria-pressed={kind === k} className={kind === k ? 'on' : undefined} onClick={() => setKind(k)}>{k === 'yes' ? 'YES basket' : k === 'no' ? 'NO basket' : 'implication A ⇒ B'}</button>
        ))}
        <label>outcomes in the set (S) <input type="number" min={1} max={7} value={s} onChange={(e) => setS(Number(e.target.value))} /></label>
        <label><input type="checkbox" checked={listed} onChange={(e) => setListed(e.target.checked)} /> "Other" is listed</label>
        <label><input type="checkbox" checked={withPrices} onChange={(e) => setWithPrices(e.target.checked)} /> with example prices</label>
      </div>
      {isError(out) ? <p role="alert">{out.error}</p> : <PayoffTable model={out} />}
      <p className="quip">{QUIPS.b}</p>
      <p className="sch-note">{CAPTIONS.b}</p>
    </section>
  );
}
```

(The number input allows 1 and 7 so that out-of-range values reach the engine and show its sentence, as the test expects.)

```tsx
// web/src/math/sections/SectionC.tsx
import { useMemo, useState } from 'react';
import type { Engine } from '../../wasm/load';
import { ClassifyTree } from '../../schematics/ClassifyTree';
import { isError, type ClassifyModel } from '../../schematics/models';
import { CAPTIONS, QUIPS } from '../copy';
import { CLASSIFY_LEGS, type LegState } from '../defaults';

const STATUSES = ['live', 'placeholder', 'other', 'halted', 'closed_no', 'closed_yes'];

export function SectionC({ engine }: { engine: Engine }) {
  const [legs, setLegs] = useState<LegState[]>(CLASSIFY_LEGS);
  const [augmented, setAugmented] = useState(false);
  const input = { augmented, legs: legs.map((l) => ({ status: l.status, bids: l.bid ? [{ p: l.bid, s: '100' }] : [], asks: l.ask ? [{ p: l.ask, s: '100' }] : [], fee: null })) };
  const out = useMemo(() => JSON.parse(engine.classify(JSON.stringify(input))) as ClassifyModel | { error: string }, [engine, legs, augmented]);
  const set = (i: number, patch: Partial<LegState>) => setLegs((ls) => ls.map((l, k) => (k === i ? { ...l, ...patch } : l)));
  return (
    <section className="box">
      <h2>c · classifying one update</h2>
      <div className="controls">
        {legs.map((l, i) => (
          <fieldset key={i}>
            <legend>leg {i + 1}</legend>
            <label>status <select value={l.status} onChange={(e) => set(i, { status: e.target.value })}>{STATUSES.map((s) => <option key={s}>{s}</option>)}</select></label>
            <label>bid <input value={l.bid} onChange={(e) => set(i, { bid: e.target.value })} size={5} /></label>
            <label>ask <input value={l.ask} onChange={(e) => set(i, { ask: e.target.value })} size={5} /></label>
          </fieldset>
        ))}
        <label><input type="checkbox" checked={augmented} onChange={(e) => setAugmented(e.target.checked)} /> augmented event</label>
        <button type="button" onClick={() => { setLegs(CLASSIFY_LEGS); setAugmented(false); }}>reset</button>
      </div>
      {isError(out) ? <p role="alert">{out.error}</p> : <ClassifyTree model={out} />}
      <p className="quip">{QUIPS.c}</p>
      <p className="sch-note">{CAPTIONS.c}</p>
    </section>
  );
}
```

```tsx
// web/src/math/sections/SectionD.tsx
import { useMemo, useState } from 'react';
import type { Engine } from '../../wasm/load';
import { DepthWalk } from '../../schematics/DepthWalk';
import { isError, type Level, type WalkModel } from '../../schematics/models';
import { CAPTIONS, QUIPS } from '../copy';
import { WALK_LEGS } from '../defaults';

const cents = (p: string) => Math.round(Number(p) * 100);
const price = (c: number) => (Math.min(99, Math.max(1, c)) / 100).toFixed(2);

export function SectionD({ engine }: { engine: Engine }) {
  const [legs, setLegs] = useState<Level[][]>(WALK_LEGS);
  const [fees, setFees] = useState(false);
  const input = { rule: 'yes', legs: legs.map((levels) => ({ levels, fee: fees ? { rate: '0.07', exponent: 1 } : null })) };
  const out = useMemo(() => JSON.parse(engine.depth_walk(JSON.stringify(input))) as WalkModel | { error: string }, [engine, legs, fees]);
  const setP = (li: number, i: number, c: number) => setLegs((ls) => ls.map((l, k) => (k === li ? l.map((x, j) => (j === i ? { ...x, p: price(c) } : x)) : l)));
  const exactlyOne = () => setLegs((ls) => {
    // Shift leg 1's levels so Σ best asks = 1.00 exactly, keeping each leg's level order.
    const rest = ls.slice(1).reduce((a, l) => a + cents(l[0].p), 0);
    const target = 100 - rest;
    const shift = target - cents(ls[0][0].p);
    return [ls[0].map((x) => ({ ...x, p: price(cents(x.p) + shift) })), ...ls.slice(1)];
  });
  return (
    <section className="box">
      <h2>d · depth walk</h2>
      <div className="controls">
        {legs.map((l, li) => l.map((x, i) => (
          <label key={`${li}-${i}`}>leg {li + 1} level {i + 1} <input type="range" min={1} max={99} value={cents(x.p)} onChange={(e) => setP(li, i, Number(e.target.value))} /> {x.p}</label>
        )))}
        {legs.length < 4 && <button type="button" onClick={() => setLegs((ls) => [...ls, [{ p: '0.05', s: '50' }]])}>add a leg</button>}
        <label><input type="checkbox" checked={fees} onChange={(e) => setFees(e.target.checked)} /> taker fees on</label>
        <button type="button" onClick={exactlyOne}>exactly 1.00</button>
        <button type="button" onClick={() => { setLegs(WALK_LEGS); setFees(false); }}>reset</button>
      </div>
      {isError(out) ? <p role="alert">{out.error}</p> : <DepthWalk model={out} />}
      <p className="quip">{QUIPS.d}</p>
      <p className="sch-note">{CAPTIONS.d}</p>
    </section>
  );
}
```

```tsx
// web/src/math/sections/SectionE.tsx
import { useMemo, useState } from 'react';
import type { Engine } from '../../wasm/load';
import { EpisodeTimeline } from '../../schematics/EpisodeTimeline';
import { isError, type EpisodeModel } from '../../schematics/models';
import { CAPTIONS, QUIPS } from '../copy';
import { EPISODE_BIDS, EPISODE_T, PRESETS } from '../defaults';

const leg = (bid: string) => ({ status: 'live', bids: [{ p: bid, s: '100' }], asks: [{ p: (Number(bid) + 0.01).toFixed(2), s: '100' }], fee: null });

export function SectionE({ engine }: { engine: Engine }) {
  const [sc, setSc] = useState({ proc_ms: 0, net_ms: 145, match_ms: 150 });
  const [delay, setDelay] = useState(false);
  const input = {
    kind: 'no', gap_ms: 30000,
    updates: EPISODE_T.map((t, i) => ({ t_ms: t, legs: EPISODE_BIDS[i].map(leg) })),
    scenario: { ...sc, honor_seconds_delay: delay }, seconds_delay: delay ? 1 : null,
  };
  const out = useMemo(() => JSON.parse(engine.episode(JSON.stringify(input))) as EpisodeModel | { error: string }, [engine, sc, delay]);
  const total = sc.proc_ms + sc.net_ms + sc.match_ms + (delay ? 1000 : 0);
  return (
    <section className="box">
      <h2>e · episode and latency</h2>
      <div className="controls">
        {PRESETS.map((p) => (
          <button key={p.name} type="button" onClick={() => setSc({ proc_ms: p.proc_ms, net_ms: p.net_ms, match_ms: p.match_ms })}>{p.name}</button>
        ))}
        {(['proc_ms', 'net_ms', 'match_ms'] as const).map((k) => (
          <label key={k}>{k.replace('_', ' ')} <input aria-label={k.replace('_', ' ')} type="range" min={0} max={1000} value={sc[k]} onChange={(e) => setSc((s) => ({ ...s, [k]: Number(e.target.value) }))} /> {sc[k]}</label>
        ))}
        <label><input type="checkbox" checked={delay} onChange={(e) => setDelay(e.target.checked)} /> market has a 1 s secondsDelay</label>
      </div>
      {isError(out) ? <p role="alert">{out.error}</p> : <EpisodeTimeline model={out} latencyLabel={`${total} ms`} />}
      <p className="quip">{QUIPS.e}</p>
      <p className="sch-note">{CAPTIONS.e}</p>
    </section>
  );
}
```

The `aria-label` (`net ms`, `match ms`) gives each slider an exact accessible name for the tests; the visible label also shows the value.

`math.css`: `.math .controls { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; } .math input[type=range] { width: min(160px, 40vw); } .math fieldset { border: 2px solid var(--line); }`.

- [ ] **Step 5: Run the tests and the full build**

Run: `PROFILE=dev sh scripts/build-wasm.sh && cd web && WASM_REQUIRED=1 npx vitest run && npm run build && ls dist/math.html dist/math-assets`
Expected: all tests pass. The build prints the gzip total under 350 KB. `dist/math.html` exists and `dist/math-assets/` holds `.js`, `.wasm`, `.css` and their `.gz` files. Then:

Run: `grep -l "api/today\|EventSource" dist/math-assets/*.js || echo clean`
Expected: `clean`.

- [ ] **Step 6: CI uses the full build**

In `.github/workflows/ci.yml`, change the last `web` step from `npm run build:dashboard` to `npm run build`.

- [ ] **Step 7: Commit**

```bash
git add web/math.html web/vite.math.config.ts web/scripts/check-math-bundle.mjs web/src/math .github/workflows/ci.yml
git commit -m "web: public math page (five schematics on arb-core WASM), separate build, size and dashboard-code guards"
```

---

### Task 9: Deploy files (nginx, Traefik, Dockerfile, runbook)

**Files:**
- Modify: `docker/web/nginx.conf`, `compose.web.yml`, `docker/web.Dockerfile`
- Create: `docs/superpowers/runbooks/phase7-playground-vps.md`

**Interfaces:**
- Consumes: `dist/math.html`, `dist/math-assets/*` (+ `.gz`) from `npm run build`.

- [ ] **Step 1: nginx**

Insert before `location / {` in `docker/web/nginx.conf`:

```nginx
    # Phase 7 (spec §5.2): the public playground. Only these two locations are routed without login
    # (Traefik ct-math); gzip_static serves the .gz files check-math-bundle.mjs wrote.
    location = /math {
        add_header Cache-Control "no-cache";
        try_files /math.html =404;
    }

    location /math-assets/ {
        add_header Cache-Control "public, max-age=31536000, immutable";
        gzip_static on;
        try_files $uri =404;
    }
```

- [ ] **Step 2: Traefik labels**

Append to the `web` service `labels` in `compose.web.yml`:

```yaml
      # Phase 7 (spec §5.3): the public playground. Static files only, no ct-auth, strict per-IP
      # limits (depth=0: DNS is a plain A record, no CDN; phase 4 runbook §0), its own CSP with
      # 'wasm-unsafe-eval'. Every other path still matches ct-web (login).
      - "traefik.http.routers.ct-math.rule=Host(`${RADAR_DOMAIN}`) && (Path(`/math`) || PathPrefix(`/math-assets/`))"
      - traefik.http.routers.ct-math.entrypoints=websecure
      - traefik.http.routers.ct-math.tls.certresolver=letsencrypt
      - traefik.http.routers.ct-math.service=ct-web
      - traefik.http.routers.ct-math.priority=1000
      - traefik.http.routers.ct-math.middlewares=ct-math-rl@docker,ct-math-if@docker,ct-sec@docker,ct-math-csp@docker
      - traefik.http.middlewares.ct-math-rl.ratelimit.average=5
      - traefik.http.middlewares.ct-math-rl.ratelimit.burst=20
      - traefik.http.middlewares.ct-math-rl.ratelimit.sourcecriterion.ipstrategy.depth=0
      - traefik.http.middlewares.ct-math-if.inflightreq.amount=4
      - traefik.http.middlewares.ct-math-if.inflightreq.sourcecriterion.ipstrategy.depth=0
      - "traefik.http.middlewares.ct-math-csp.headers.contentSecurityPolicy=default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
```

Run: `docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q`
Expected: no output (valid).

- [ ] **Step 3: Dockerfile**

Replace the top of `docker/web.Dockerfile` (keep the nginx stage as is):

```dockerfile
# syntax=docker/dockerfile:1
# Build context: repository root.  docker compose -f compose.yml -f compose.web.yml --profile web build web
# Phase 7: arb-wasm is built first (same pinned Rust image as rust.Dockerfile, cargo-chef so a
# web-only change reuses the cooked dependencies), then the node stage builds both pages.
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS wasm-chef
RUN rustup target add wasm32-unknown-unknown \
 && cargo install cargo-chef --locked \
 && cargo install wasm-bindgen-cli --locked --version WBG
WORKDIR /src

FROM wasm-chef AS wasm-planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM wasm-chef AS wasm
COPY --from=wasm-planner /src/recipe.json recipe.json
RUN cargo chef cook --release --target wasm32-unknown-unknown --recipe-path recipe.json --package arb-wasm
COPY . .
RUN sh scripts/build-wasm.sh

FROM node:22-alpine@sha256:0a7108bf6c7bf5de370ffb1a3ed6be93d405b43ff159f681a8d18c0e2bc2e402 AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
COPY --from=wasm /src/web/src/wasm/pkg ./src/wasm/pkg
RUN npm run build
```

(The node stage no longer has `crates/` fixtures, and doesn't need them: `npm run build` runs no tests.)

- [ ] **Step 4: Runbook**

Write `docs/superpowers/runbooks/phase7-playground-vps.md` with these sections, each as numbered, copy-pasteable commands in the style of `phase5a-cards-vps.md`:

1. **Prerequisites on the PC:** `rustup target add wasm32-unknown-unknown` (done 2026-09-27), `cargo install wasm-bindgen-cli --locked --version WBG`, `PROFILE=dev sh scripts/build-wasm.sh`, then `cd web && npm run build`.
2. **Build and pin:** `docker compose -f compose.yml -f compose.web.yml --profile web build web`, push to the private registry as in the phase 4 runbook, then pin the new web digest in `compose.web.yml`.
3. **Deploy:** `docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web`.
4. **No-cookie checks** from the PC, each with its expected result:
   - `curl -sI https://radar.isoca.space/math` → `200`, `content-security-policy:` containing `'wasm-unsafe-eval'`;
   - `curl -sI -H 'Accept-Encoding: gzip' https://radar.isoca.space/math-assets/<the .wasm name from dist>` → `200`, `content-type: application/wasm`, `content-encoding: gzip`;
   - `curl -sI https://radar.isoca.space/math/` and `/math.html` and `/api/today` → `302` to the login;
   - `curl -sI http://radar.isoca.space/math` → a redirect to `https://`.
5. **Flood test** (spec §5.5). Install `oha` on the PC if it's missing (`winget install hatoo.oha`); otherwise use `for i in $(seq 1 600); do curl -s -o /dev/null -w '%{http_code}\n' https://radar.isoca.space/math-assets/<wasm>; done | sort | uniq -c`.
   - Run `oha -z 60s -c 50 https://radar.isoca.space/math-assets/<wasm>` and expect mostly `429`.
   - Meanwhile, on the VPS, run `docker stats --no-stream` every 10 s: `web` ≤ 32 MiB, and `api`, `scanner` and `postgres` unchanged.
   - A phone on mobile data loads `/math` during the run.
   - The dashboard (logged in) still loads.
   - Record the numbers in a results section.
6. **What this test does not show:** distributed floods and Traefik TLS cost (spec §5.4).
7. **Rollback:** re-pin the previous web digest, then `up -d --no-build --no-deps web`.

- [ ] **Step 5: Commit**

```bash
git add docker/web/nginx.conf compose.web.yml docker/web.Dockerfile docs/superpowers/runbooks/phase7-playground-vps.md
git commit -m "deploy: public /math router (per-IP limits, own CSP), gzip_static, wasm stage, phase 7 runbook"
```

---

### Task 10: `Position.end_date` (store, api, web type)

**Files:**
- Modify: `crates/store/src/read_paper.rs`, `crates/store/tests/read_paper.rs`, `bins/api/src/paper.rs`, `web/src/api/types.ts`, `web/src/test/fixtures.ts`

**Interfaces:**
- Produces: `PaperPosition.end_date: Option<String>`, `PositionDto.end_date: Option<String>`, and the web `Position.end_date: string | null` (the event's `end_date` text as stored, an ISO date string).

- [ ] **Step 1: Write the failing store test**

In `crates/store/tests/read_paper.rs`, in the test that inserts `INSERT INTO events (event_id, slug, title, …)` at line ~198:
- add `end_date` to the column list with the value `'2026-10-29T00:00:00Z'`;
- in the expected `PaperPosition` for `m1`, add `end_date: Some("2026-10-29T00:00:00Z".into())`;
- for `m9` (no leg row, so no event), add `end_date: None`.

Run (with the Postgres prefix): `cargo test -p store --test read_paper`
Expected: FAIL to compile, with "struct `PaperPosition` has no field named `end_date`".

- [ ] **Step 2: Implement**

In `crates/store/src/read_paper.rs`:
- add `pub end_date: Option<String>,` to `PaperPosition` after `event_title`;
- in `paper_positions`' final SELECT, change `e.title AS event_title` to `e.title AS event_title, e.end_date AS end_date`.

In `bins/api/src/paper.rs`, add `pub end_date: Option<String>,` to `PositionDto` and `end_date: p.end_date,` in the `From` impl.

In `web/src/api/types.ts`, add `end_date: string | null;` to `Position`. In `web/src/test/fixtures.ts`, add `end_date: '2026-10-29T00:00:00Z',` to `positionFixture`'s defaults.

- [ ] **Step 3: Run**

Run (Postgres prefix): `cargo test -p store --test read_paper && cargo test -p api && cd web && npx vitest run && npx tsc --noEmit`
Expected: PASS. If an api test snapshots the positions JSON, add `"end_date"` there as well.

- [ ] **Step 4: Commit**

```bash
git add crates/store/src/read_paper.rs crates/store/tests/read_paper.rs bins/api/src/paper.rs web/src/api/types.ts web/src/test/fixtures.ts
git commit -m "api: paper positions carry the event end_date (for days to resolution)"
```

---

### Task 11: Explain panel shell

**Files:**
- Create: `web/src/explain/{ExplainContext.tsx,Explain.tsx,useMedia.ts,storage.ts,explain.css,questions.tsx,select.tsx}`, `web/src/explain/Explain.test.tsx`
- Modify: `web/src/App.tsx`, `web/src/main.tsx` (import `explain.css` and `schematics/schematics.css`)

**Interfaces:**
- Produces:

```ts
// ExplainContext.tsx
export type ExplainPage = 'radar' | 'episodes' | 'episode' | 'paper' | 'papers' | 'health';
export type Selection =
  | { kind: 'episode'; episode: EpisodeSummary }
  | { kind: 'position'; position: Position }
  | { kind: 'card'; card: CardBody }
  | null;
export interface PageData {
  radar?: { open?: EpisodeSummary[]; today?: Today; edgeToday?: CardBody };
  episodes?: { items?: EpisodeSummary[] };
  episode?: { detail?: EpisodeDetail };
  paper?: { accounts?: PaperAccount[]; positions?: Position[] };
  papers?: { cards?: CardBody[] };
  health?: { health?: HealthNow; integrity?: Integrity };
}
export interface ExplainState { page: ExplainPage | null; data: PageData; selection: Selection }
export function ExplainProvider({ children }: { children: ReactNode }): ReactElement
export function useExplain(): ExplainState & { select(s: Selection): void; open(): void }
/** A page publishes the data it already fetched; the panel only reads it. */
export function useExplainData<P extends ExplainPage>(page: P, data: NonNullable<PageData[P]>): void
// questions.tsx
export interface Question { id: string; q: string; when?: (s: ExplainState) => boolean; Answer: (p: { s: ExplainState }) => ReactElement }
export const QUESTIONS: Record<ExplainPage, Question[]>   // Task 11 ships one placeholder-free question per page: see Step 3
// select.tsx
export function ExplainButton({ selection }: { selection: NonNullable<Selection> }): ReactElement  // "explain" row button: select + open
// storage.ts
export function readOpen(): boolean; export function writeOpen(v: boolean): void;
export function readQ(page: ExplainPage): string | null; export function writeQ(page: ExplainPage, id: string): void;
// useMedia.ts
export function useMedia(query: string): boolean   // false when matchMedia is missing
```

- **Selection vs spec §7.2:** the selection holds the row object itself, not just its id, so the answer never refetches. It's the snapshot of the row at selection time; the panel says "as selected" (Review Focus 4).
- `useExplainData` sets `page` and `data[page]` in an effect keyed on `page` plus the data object identities, and clears `page`/`selection` on unmount.

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/explain/Explain.test.tsx
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Explain } from './Explain';
import { ExplainProvider, useExplainData } from './ExplainContext';

function Page() {
  useExplainData('health', {});
  return <p>page</p>;
}

function mount() {
  return render(
    <MemoryRouter>
      <ExplainProvider>
        <Page />
        <Explain />
      </ExplainProvider>
    </MemoryRouter>,
  );
}

function stubMedia(wide: boolean) {
  vi.stubGlobal('matchMedia', (q: string) => ({ matches: wide && q.includes('1500'), media: q, addEventListener: () => {}, removeEventListener: () => {} }));
}

beforeEach(() => localStorage.clear());
afterEach(() => localStorage.clear());

describe('Explain panel shell', () => {
  it('is closed by default and opens from the tab', () => {
    stubMedia(false);
    mount();
    const tab = screen.getByRole('button', { name: /EXPLAIN/ });
    expect(tab).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(tab);
    expect(tab).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('complementary', { name: 'explain' })).toBeInTheDocument();
    expect(localStorage.getItem('explain.open')).toBe('1');
  });
  it('Escape closes the drawer and returns focus to the tab', () => {
    stubMedia(false);
    mount();
    const tab = screen.getByRole('button', { name: /EXPLAIN/ });
    fireEvent.click(tab);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(tab).toHaveAttribute('aria-expanded', 'false');
    expect(tab).toHaveFocus();
  });
  it('docks at 1500 px and is a drawer below', () => {
    stubMedia(true);
    localStorage.setItem('explain.open', '1');
    const { unmount } = mount();
    expect(screen.getByRole('complementary', { name: 'explain' })).toHaveAttribute('data-mode', 'dock');
    unmount();
    stubMedia(false);
    mount();
    expect(screen.getByRole('complementary', { name: 'explain' })).toHaveAttribute('data-mode', 'drawer');
  });
  it('bad_storage_falls_back_to_defaults', () => {
    stubMedia(false);
    localStorage.setItem('explain.open', 'yes');
    localStorage.setItem('explain.q.health', 'gone-question');
    mount();
    expect(screen.getByRole('button', { name: /EXPLAIN/ })).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
    const first = screen.getAllByRole('button', { expanded: true }).find((b) => b.className.includes('q'));
    expect(first).toBeDefined();
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('private'); });
    expect(() => act(() => { fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ })); })).not.toThrow();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npx vitest run src/explain/Explain.test.tsx`
Expected: FAIL, "Failed to resolve import './Explain'".

- [ ] **Step 3: Implement**

```ts
// web/src/explain/storage.ts
// Phase 7 spec §7.1: open/closed and the last question per page. Every access is guarded; a
// private window or a foreign value falls back to the defaults (closed, first question).
import type { ExplainPage } from './ExplainContext';

function get(k: string): string | null {
  try { return localStorage.getItem(k); } catch { return null; }
}
function set(k: string, v: string): void {
  try { localStorage.setItem(k, v); } catch { /* storage unavailable: keep in memory only */ }
}
export const readOpen = (): boolean => get('explain.open') === '1';
export const writeOpen = (v: boolean): void => set('explain.open', v ? '1' : '0');
export const readQ = (page: ExplainPage): string | null => get(`explain.q.${page}`);
export const writeQ = (page: ExplainPage, id: string): void => set(`explain.q.${page}`, id);
```

```ts
// web/src/explain/useMedia.ts
import { useEffect, useState } from 'react';

export function useMedia(query: string): boolean {
  const mq = () => (typeof window.matchMedia === 'function' ? window.matchMedia(query) : null);
  const [on, setOn] = useState(() => mq()?.matches ?? false);
  useEffect(() => {
    const m = mq();
    if (!m) return;
    const f = () => setOn(m.matches);
    m.addEventListener('change', f);
    return () => m.removeEventListener('change', f);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query]);
  return on;
}
```

```tsx
// web/src/explain/ExplainContext.tsx
// Phase 7 spec §7.2: pages publish what they already fetched; the panel reads it.
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import type { CardBody, EpisodeDetail, EpisodeSummary, HealthNow, Integrity, PaperAccount, Position, Today } from '../api/types';
import { readOpen, writeOpen } from './storage';

export type ExplainPage = 'radar' | 'episodes' | 'episode' | 'paper' | 'papers' | 'health';
export type Selection =
  | { kind: 'episode'; episode: EpisodeSummary }
  | { kind: 'position'; position: Position }
  | { kind: 'card'; card: CardBody }
  | null;
export interface PageData {
  radar?: { open?: EpisodeSummary[]; today?: Today; edgeToday?: CardBody };
  episodes?: { items?: EpisodeSummary[] };
  episode?: { detail?: EpisodeDetail };
  paper?: { accounts?: PaperAccount[]; positions?: Position[] };
  papers?: { cards?: CardBody[] };
  health?: { health?: HealthNow; integrity?: Integrity };
}
export interface ExplainState { page: ExplainPage | null; data: PageData; selection: Selection }

interface Ctx extends ExplainState {
  isOpen: boolean;
  setOpen(v: boolean): void;
  select(s: Selection): void;
  open(): void;
  publish(page: ExplainPage, d: PageData[ExplainPage] | null): void;
}

const C = createContext<Ctx | null>(null);

export function ExplainProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<ExplainPage | null>(null);
  const [data, setData] = useState<PageData>({});
  const [selection, setSelection] = useState<Selection>(null);
  const [isOpen, setIsOpen] = useState(readOpen);
  const setOpen = useCallback((v: boolean) => {
    setIsOpen(v);
    writeOpen(v);
  }, []);
  const publish = useCallback((p: ExplainPage, d: PageData[ExplainPage] | null) => {
    if (d === null) {
      setPage((cur) => (cur === p ? null : cur));
      setSelection(null);
      return;
    }
    setPage(p);
    setData((cur) => ({ ...cur, [p]: d }));
  }, []);
  const value = useMemo<Ctx>(
    () => ({ page, data, selection, isOpen, setOpen, select: setSelection, open: () => setOpen(true), publish }),
    [page, data, selection, isOpen, setOpen, publish],
  );
  return <C.Provider value={value}>{children}</C.Provider>;
}

export function useExplainCtx(): Ctx {
  const c = useContext(C);
  if (!c) throw new Error('useExplain outside ExplainProvider');
  return c;
}

export function useExplain() {
  const { page, data, selection, select, open } = useExplainCtx();
  return { page, data, selection, select, open };
}

export function useExplainData<P extends ExplainPage>(page: P, d: NonNullable<PageData[P]>): void {
  const { publish } = useExplainCtx();
  const deps = Object.values(d);
  useEffect(() => {
    publish(page, d);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page, publish, ...deps]);
  useEffect(() => () => publish(page, null), [page, publish]);
}
```

```tsx
// web/src/explain/select.tsx
import { useExplain, type Selection } from './ExplainContext';

/** The small "explain" button on a row or card (spec §7.2): selects it and opens the panel. */
export function ExplainButton({ selection, label = 'explain' }: { selection: NonNullable<Selection>; label?: string }) {
  const { select, open } = useExplain();
  return (
    <button type="button" className="explain-row" aria-label={`${label} this`} onClick={(e) => { e.stopPropagation(); select(selection); open(); }}>
      ?
    </button>
  );
}
```

```tsx
// web/src/explain/questions.tsx
import type { ReactElement } from 'react';
// Phase 7 spec §7.3. Task 11 registers the Health question only; Tasks 12–14 add the rest.
import type { ExplainPage, ExplainState } from './ExplainContext';

export interface Question {
  id: string;
  q: string;
  when?: (s: ExplainState) => boolean;
  Answer: (p: { s: ExplainState }) => ReactElement;
}

export const QUESTIONS: Record<ExplainPage, Question[]> = {
  radar: [],
  episodes: [],
  episode: [],
  paper: [],
  papers: [],
  // Replaced by the full answer in Task 14; here so the shell has a question to show.
  health: [{ id: 'seeing-everything', q: 'Are we seeing everything?', Answer: () => <p>The hearts count what the scanner can see.</p> }],
};
```

Later tasks add each page's list with `QUESTIONS.<page> = [...]` below this literal.

```tsx
// web/src/explain/Explain.tsx
// Phase 7 spec §7.1: a tab on the right edge; docked beside the app at ≥ 1500 px, else a drawer.
import { useEffect, useRef, useState } from 'react';
import { useExplainCtx } from './ExplainContext';
import { QUESTIONS } from './questions';
import { readQ, writeQ } from './storage';
import { useMedia } from './useMedia';

export function Explain() {
  const { page, data, selection, isOpen, setOpen, select } = useExplainCtx();
  const wide = useMedia('(min-width: 1500px)');
  const tab = useRef<HTMLButtonElement>(null);
  const s = { page, data, selection };
  const qs = page ? QUESTIONS[page].filter((q) => !q.when || q.when(s)) : [];
  const stored = page ? readQ(page) : null;
  const [cur, setCur] = useState<string | null>(null);
  const active = qs.find((q) => q.id === cur) ?? qs.find((q) => q.id === stored) ?? qs[0];
  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false);
        tab.current?.focus();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [isOpen, setOpen]);
  return (
    <>
      <button ref={tab} type="button" className="explain-tab" aria-expanded={isOpen} aria-controls="explain-panel" onClick={() => setOpen(!isOpen)}>
        EXPLAIN {isOpen ? '▸' : '◂'}
      </button>
      {isOpen && (
        <aside id="explain-panel" aria-label="explain" className="explain" data-mode={wide ? 'dock' : 'drawer'}>
          {selection && (
            <p className="explain-sel">
              as selected · <button type="button" onClick={() => select(null)}>back to the page</button>
            </p>
          )}
          {qs.length === 0 && <p>Nothing to explain on this page yet.</p>}
          <ul className="explain-qs">
            {qs.map((q) => (
              <li key={q.id}>
                <button
                  type="button"
                  className="q"
                  aria-expanded={q.id === active?.id}
                  onClick={() => {
                    setCur(q.id);
                    if (page) writeQ(page, q.id);
                  }}
                >
                  {q.q}
                </button>
                {q.id === active?.id && (
                  <div className="explain-a">
                    <q.Answer s={s} />
                  </div>
                )}
              </li>
            ))}
          </ul>
        </aside>
      )}
    </>
  );
}
```

```css
/* web/src/explain/explain.css (Phase 7 spec §7.1) */
.shell { display: flex; justify-content: center; align-items: flex-start; gap: 16px; }
.shell > .app { flex: 0 1 1100px; margin: 0; min-width: 0; }
.explain-tab { position: fixed; right: 0; top: 40%; writing-mode: vertical-rl; font-family: var(--px); font-size: 8px; padding: 8px 5px; border: 2px solid var(--line); background: var(--chip); z-index: 11; cursor: pointer; }
.explain { background: var(--bar); border: 3px solid var(--line); box-shadow: 6px 6px 0 var(--line); padding: 12px; font-size: 15px; line-height: 1.5; overflow-y: auto; }
.explain[data-mode='drawer'] { position: fixed; right: 0; top: 0; height: 100vh; width: min(360px, 100vw); z-index: 10; box-sizing: border-box; }
.explain[data-mode='dock'] { flex: 0 0 360px; position: sticky; top: 12px; max-height: calc(100vh - 24px); }
.explain-qs { list-style: none; margin: 0; padding: 0; display: grid; gap: 8px; }
.explain-qs .q { width: 100%; text-align: left; font-family: var(--body); font-weight: 600; border: 2px solid var(--line); background: var(--chip); padding: 6px 8px; cursor: pointer; }
.explain-a { padding: 8px 2px; }
.explain-row { font-family: var(--px); font-size: 7px; border: 2px solid var(--line); background: var(--chip); padding: 2px 5px; cursor: pointer; }
```

Check the font variable names in `theme.css` (`--px`, and the body font variable) and use the ones that exist.

In `web/src/App.tsx`, wrap the return:

```tsx
export function App() {
  return (
    <ExplainProvider>
      <div className="shell">
        <div className="app">
          <Header />
          <main className="body">
            <Routes>{/* unchanged */}</Routes>
          </main>
        </div>
        <Explain />
      </div>
    </ExplainProvider>
  );
}
```

The docked aside is a flex sibling of `.app`; the drawer is `position: fixed`, so it takes no flex space. In `web/src/main.tsx`, add `import './schematics/schematics.css';` and `import './explain/explain.css';` after `import './theme.css';`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && npx vitest run && npx tsc --noEmit`
Expected: PASS, and existing page tests are unaffected (they render pages without `App`). If a test renders `App`, it now needs nothing more, because the provider is inside `App`.

- [ ] **Step 5: Commit**

```bash
git add web/src/explain web/src/App.tsx web/src/main.tsx
git commit -m "web: explain panel shell (tab, dock at 1500 px, drawer below, Esc, guarded storage)"
```

---

### Task 12: Explain on Radar (copy, honesty, payoff structure)

**Files:**
- Create: `web/src/explain/copy.ts`, `web/src/explain/models.ts`, `web/src/explain/radar.test.tsx`, `web/src/explain/honesty.ts`
- Modify: `web/src/explain/questions.tsx`, `web/src/pages/Radar.tsx`

**Interfaces:**
- Consumes: `PayoffTable` (Task 6), the context (Task 11), `formatMoneyCents` and `formatDecimal` from `src/lib/money.ts`, and `ON_TABLE` from `src/lib/cards.ts` (export it if it's not exported).
- Produces:
  - `payoffStructure(kind: 'yes' | 'no', s: number, otherListed: boolean): PayoffModel` (structure only; `top_edge_gross: null`);
  - `checkHonesty(text: string): string[]` (returns the offending words; empty = honest);
  - the Radar questions `make-money`, `generated-today`, `not-arbitrage` (when the selection is `yes_basket_incomplete`) and `columns`.

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/explain/radar.test.tsx
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EpisodeSummary } from '../api/types';
import { episodeSummaryFixture, todayFixture } from '../test/fixtures';
import { Explain } from './Explain';
import { ExplainProvider, useExplain, useExplainData } from './ExplainContext';
import { checkHonesty } from './honesty';
import { payoffStructure } from './models';

function Radarish({ open, sel }: { open: EpisodeSummary[]; sel?: EpisodeSummary }) {
  useExplainData('radar', { open, today: todayFixture(3, 1, null) });
  const { select } = useExplain();
  return <button onClick={() => sel && select({ kind: 'episode', episode: sel })}>pick</button>;
}

function mount(open: EpisodeSummary[], sel?: EpisodeSummary) {
  const r = render(
    <MemoryRouter>
      <ExplainProvider>
        <Radarish open={open} sel={sel} />
        <Explain />
      </ExplainProvider>
    </MemoryRouter>,
  );
  fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
  return r;
}

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal('matchMedia', undefined);
});

describe('payoffStructure', () => {
  it('matches the engine fixtures for s 2..6', () => {
    const file = fileURLToPath(new URL('../../../crates/arb-wasm/fixtures/payoff.json', import.meta.url));
    const cases = JSON.parse(readFileSync(file, 'utf8')) as { name: string; input: string; output: string }[];
    let checked = 0;
    for (const c of cases) {
      const i = JSON.parse(c.input) as { kind: string; s: number; other_listed: boolean; prices: unknown };
      if (i.kind === 'implication' || i.prices !== null || i.s < 2 || i.s > 6) continue;
      const o = JSON.parse(c.output);
      expect({ ...payoffStructure(i.kind as 'yes' | 'no', i.s, i.other_listed), top_edge_gross: null }, c.name).toEqual({ ...o, top_edge_gross: null });
      checked++;
    }
    expect(checked).toBeGreaterThanOrEqual(3);
  });
});

describe('Radar questions', () => {
  it('How can this make money? uses the selected row', () => {
    mount([episodeSummaryFixture()], episodeSummaryFixture({ kind: 'yes_basket', s_legs: 3, net_budget100: '0.33' }));
    fireEvent.click(screen.getByText('pick'));
    fireEvent.click(screen.getByRole('button', { name: 'How can this make money?' }));
    const a = screen.getByRole('complementary', { name: 'explain' });
    expect(within(a).getByRole('columnheader', { name: 'A' })).toBeInTheDocument();
    expect(within(a).getByText(/\$0\.33 on the table at \$100/)).toBeInTheDocument();
  });
  it('Why is this row not arbitrage? appears only for an incomplete set, with one generic column', () => {
    mount([], episodeSummaryFixture({ kind: 'yes_basket_incomplete', s_legs: 2 }));
    expect(screen.queryByRole('button', { name: 'Why is this row not arbitrage?' })).toBeNull();
    fireEvent.click(screen.getByText('pick'));
    fireEvent.click(screen.getByRole('button', { name: 'Why is this row not arbitrage?' }));
    expect(screen.getByRole('columnheader', { name: /an outcome not in the set ✗/ })).toBeInTheDocument();
  });
  it('unmeasured_figures_never_read_as_zero', () => {
    mount([episodeSummaryFixture({ net_budget100: null })]);
    fireEvent.click(screen.getByRole('button', { name: 'How can this make money?' }));
    expect(screen.getByRole('complementary', { name: 'explain' })).toHaveTextContent('not measured');
    expect(screen.getByRole('complementary', { name: 'explain' })).not.toHaveTextContent('$0.00');
    fireEvent.click(screen.getByRole('button', { name: 'What did we generate today?' }));
    expect(screen.getByRole('complementary', { name: 'explain' })).toHaveTextContent('not measured');
  });
  it('selection_survives_its_row_leaving_the_list', () => {
    const row = episodeSummaryFixture({ id: 'gone', kind: 'no_basket', s_legs: 4 });
    const { rerender } = mount([row], row);
    fireEvent.click(screen.getByText('pick'));
    rerender(
      <MemoryRouter>
        <ExplainProvider>
          <Radarish open={[]} sel={row} />
          <Explain />
        </ExplainProvider>
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByText('pick'));
    fireEvent.click(screen.getByRole('button', { name: 'How can this make money?' }));
    expect(screen.getByText(/as selected/)).toBeInTheDocument();
    expect(screen.getAllByRole('columnheader').length).toBeGreaterThan(1);
  });
  it('every Radar answer is honest', () => {
    mount([episodeSummaryFixture()], episodeSummaryFixture({ kind: 'yes_basket_incomplete' }));
    fireEvent.click(screen.getByText('pick'));
    const panel = screen.getByRole('complementary', { name: 'explain' });
    for (const b of within(panel).getAllByRole('button').filter((x) => x.className === 'q')) {
      fireEvent.click(b);
      expect(checkHonesty(panel.textContent ?? ''), b.textContent ?? '').toEqual([]);
    }
  });
});

describe('checkHonesty', () => {
  it('flags real-money words and bare profit', () => {
    expect(checkHonesty('We earned $3 of income')).toEqual(['earned', 'income']);
    expect(checkHonesty('profit @ $100 and net profit at the peak')).toEqual([]);
    expect(checkHonesty('a nice profit')).toEqual(['profit']);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npx vitest run src/explain/radar.test.tsx`
Expected: FAIL, "Failed to resolve import './honesty'".

- [ ] **Step 3: Implement**

```ts
// web/src/explain/honesty.ts
// Phase 7 spec §7.2: panel text never claims real money.
const BANNED = /\b(earned|earn|made|income)\b/gi;
const PROFIT_OK = /profit @ \$100|net profit at the peak|profit after|profit before/gi;

export function checkHonesty(text: string): string[] {
  const hits = [...text.matchAll(BANNED)].map((m) => m[0].toLowerCase());
  const bare = text.replace(PROFIT_OK, '').match(/\bprofit\b/gi) ?? [];
  return [...hits, ...bare.map((p) => p.toLowerCase())];
}
```

```ts
// web/src/explain/models.ts
// Structure-only payoff model for the panel (spec §7.3). Money comes from the api, never here.
// Tested against crates/arb-wasm/fixtures/payoff.json so it cannot drift from arb-core.
import type { PayoffModel } from '../schematics/models';

const NAMES = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L'];

export function payoffStructure(kind: 'yes' | 'no', s: number, otherListed: boolean): PayoffModel {
  const cols = [...NAMES.slice(0, s), ...(otherListed ? [] : ['Other'])];
  const rows = NAMES.slice(0, s).map((n) => `${kind === 'yes' ? 'YES' : 'NO'}_${n}`);
  const cells = rows.map((_, i) => cols.map((_, c) => ((kind === 'yes' ? c === i : c !== i) ? '1' : '0')));
  const basket = cols.map((_, c) => String(cells.reduce((a, r) => a + Number(r[c]), 0)));
  const guaranteed = kind === 'yes' ? '1' : String(s - 1);
  return { rows, cols, possible: cols.map(() => true), cells, basket, guaranteed, broken: basket.map((b) => Number(b) < Number(guaranteed)), top_edge_gross: null };
}
```

`Number()` on the integers "0"–"12" is exact, and these values are counts, not money.

```ts
// web/src/explain/copy.ts
// Panel sentences (spec §7.3). Every money figure says what kind of number it is.
import { formatMoneyCents } from '../lib/money';

export const money = (v: string | null): string => (v === null ? 'not measured' : formatMoneyCents(v, { signed: true }));

export const RADAR = {
  makeMoneyYes: 'A complete YES basket holds one YES of every outcome. Exactly one outcome wins, so the basket pays $1 whoever wins. If the whole basket costs less than $1, the difference is the edge, collected at resolution.',
  makeMoneyNo: 'A NO basket holds one NO of each outcome in the set. Converted back to collateral it returns S − 1 dollars straight away, whoever wins. If it costs less than that, the difference is the edge, collected now.',
  onTable: (v: string | null) => `This one: ${money(v)} on the table at $100 spent (net profit at the peak).`,
  notFill: 'On the table means the prices were there. Catching them is another matter: see Edge capture on the Paper page.',
  todayIntro: 'Two different numbers, both paper: what was on the table today, and what a trader at our measured latency would have caught of it.',
  todayTable: (v: string | null) => `On the table today: ${money(v)} (every complete basket, capped at $100 each).`,
  todayCaught: (ms: number, v: string | null) => `Captured at ${ms} ms: ${money(v)} in the paper account.`,
  todayNoTrades: 'No real order was placed. This dashboard does not trade.',
  notArb: 'The listed outcomes do not cover every result. If an outcome outside the set wins, every token bought pays nothing. The price gap is real, the guarantee is not.',
  columns: [
    'S: how many outcomes are in the set bought.',
    'PROFIT @ $100: the net profit at the peak with at most $100 spent.',
    'NET @ DEPTH and GROSS: profit after and profit before taker fees, taking all the depth the books offered.',
    'Q*: that executable size, in shares.',
    'AGE: time since it opened. EDGE: the per-share edge as a bar.',
  ],
};
```

Check that `formatMoneyCents` exists in `src/lib/money.ts` with that signature (it's used by `teaserLine`). If its options differ, adapt the call.

Add the Radar questions to `questions.tsx`:

```tsx
import { PayoffTable } from '../schematics/PayoffTable';
import { money, RADAR } from './copy';
import { payoffStructure } from './models';
import { ON_TABLE } from '../lib/cards';

function radarEpisode(s: ExplainState) {
  return s.selection?.kind === 'episode' ? s.selection.episode : s.data.radar?.open?.[0] ?? null;
}

QUESTIONS.radar = [
  {
    id: 'make-money',
    q: 'How can this make money?',
    Answer: ({ s }) => {
      const e = radarEpisode(s);
      const kind = e?.kind.startsWith('no') ? 'no' : 'yes';
      const n = Math.min(6, Math.max(2, e?.s_legs ?? 3));
      return (
        <>
          {!e && <p><i>example</i></p>}
          <PayoffTable compact model={payoffStructure(kind, n, e?.kind !== 'yes_basket_incomplete')} colLabels={{ Other: 'an outcome not in the set' }} />
          <p>{kind === 'yes' ? RADAR.makeMoneyYes : RADAR.makeMoneyNo}</p>
          {e && <p>{RADAR.onTable(e.net_budget100)}</p>}
          <p>{RADAR.notFill}</p>
        </>
      );
    },
  },
  {
    id: 'generated-today',
    q: 'What did we generate today?',
    Answer: ({ s }) => {
      const edge = s.data.radar?.edgeToday;
      const head = edge?.series.decay?.find((p) => p.headline) ?? null;
      const onTable = edge?.columns.find((r) => r.label === ON_TABLE)?.ours ?? s.data.radar?.today?.edge_budget100 ?? null;
      return (
        <>
          <p>{RADAR.todayIntro}</p>
          <p>{RADAR.todayTable(onTable)}</p>
          <p>{RADAR.todayCaught(head?.net_ms ?? 145, head?.captured ?? null)}</p>
          <p>{RADAR.todayNoTrades}</p>
        </>
      );
    },
  },
  {
    id: 'not-arbitrage',
    q: 'Why is this row not arbitrage?',
    when: (s) => s.selection?.kind === 'episode' && s.selection.episode.kind === 'yes_basket_incomplete',
    Answer: ({ s }) => {
      const e = s.selection?.kind === 'episode' ? s.selection.episode : null;
      return (
        <>
          <PayoffTable compact model={payoffStructure('yes', Math.min(6, Math.max(2, e?.s_legs ?? 2)), false)} colLabels={{ Other: 'an outcome not in the set' }} />
          <p>{RADAR.notArb}</p>
        </>
      );
    },
  },
  {
    id: 'columns',
    q: 'How do I read these columns?',
    Answer: () => (
      <ul>
        {RADAR.columns.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ul>
    ),
  },
];
```

Keep `money` imported only where it's used (the linter flags unused imports).

In `web/src/pages/Radar.tsx`:

1. After `const edgeToday = …`, add:

```tsx
useExplainData('radar', { open: open.data?.items, today: today.data, edgeToday: edgeToday.data?.items?.find((c) => c.card === 'EDGE') });
```

   Check the shape of `api.cards` (`CardsResponse`) in `types.ts` (`grep -n "interface CardsResponse" -A 6 web/src/api/types.ts`) and pick the EDGE card the same way the teaser does in `Radar.tsx:170-180`, reusing that expression.
2. In `episodeColumns`, append a column:

```tsx
{ key: 'explain', label: '', render: (r) => <ExplainButton selection={{ kind: 'episode', episode: r }} /> },
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && npx vitest run && npx tsc --noEmit`
Expected: PASS, including the existing `Radar.test.tsx`. If a Radar test counts columns, update it for the new empty-label column.

- [ ] **Step 5: Commit**

```bash
git add web/src/explain web/src/pages/Radar.tsx web/src/lib/cards.ts
git commit -m "web: explain Radar (make money, today, not arbitrage, columns) with honesty check"
```

---

### Task 13: Explain on Episodes and Episode detail (the one lazy fetch)

**Files:**
- Create: `web/src/explain/episodes.test.tsx`
- Modify: `web/src/explain/questions.tsx`, `web/src/explain/copy.ts`, `web/src/pages/Episodes.tsx`, `web/src/pages/EpisodeDetail.tsx`

**Interfaces:**
- Consumes: `DepthWalk`, `EpisodeTimeline` (Task 7), `FLAG_WORDS` from `src/lib/flags.ts`, `api.cards` and `useApi`.
- Produces:
  - Episodes questions: `what-episode`, `flags-row` (when an episode is selected), `flags`;
  - Episode detail questions: `cost`, `after-open`, `caught` (the only fetching question);
  - `detailWalk(detail: EpisodeDetail): WalkModel` (one step per leg in S, from `vwap` and `q_star`) and `detailTimeline(detail): EpisodeModel` (open, the re-checks as updates, close), both in `models.ts`.

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/explain/episodes.test.tsx
import type { ReactElement } from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { episodeDetailFixture, episodeSummaryFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Explain } from './Explain';
import { ExplainProvider, useExplain, useExplainData } from './ExplainContext';
import { detailTimeline, detailWalk } from './models';

function Detail() {
  useExplainData('episode', { detail: episodeDetailFixture() });
  return null;
}
function List() {
  useExplainData('episodes', { items: [episodeSummaryFixture({ censored: true })] });
  const { select } = useExplain();
  return <button onClick={() => select({ kind: 'episode', episode: episodeSummaryFixture({ censored: true, duration_ms: 3600 }) })}>pick</button>;
}
const mount = (c: ReactElement) => {
  render(<MemoryRouter><ExplainProvider>{c}<Explain /></ExplainProvider></MemoryRouter>);
  fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
};

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal('matchMedia', undefined);
});

describe('Episode detail questions', () => {
  it('the cost answer is the simplified walk with its caption', () => {
    const fetch = mockApi({});
    mount(<Detail />);
    fireEvent.click(screen.getByRole('button', { name: 'What would buying this have cost?' }));
    expect(screen.getByText('simplified: per-level steps are not stored')).toBeInTheDocument();
    expect(fetch).not.toHaveBeenCalled();
  });
  it('What happened after it opened? draws the re-checks', () => {
    mockApi({});
    mount(<Detail />);
    fireEvent.click(screen.getByRole('button', { name: 'What happened after it opened?' }));
    expect(screen.getByLabelText('episode timeline')).toBeInTheDocument();
  });
  it('Would we have caught it? fetches the 24h cards exactly once', async () => {
    const fetch = mockApi({ '/api/cards': { items: [], win: '24h' } });
    mount(<Detail />);
    expect(fetch).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Would we have caught it?' }));
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    expect(String(fetch.mock.calls[0][0])).toContain('/api/cards?win=24h');
    expect(await screen.findByText(/latency is not measured yet/)).toBeInTheDocument();
  });
});

describe('Episodes questions', () => {
  it('explains the selected row flags in words', () => {
    mockApi({});
    mount(<List />);
    fireEvent.click(screen.getByText('pick'));
    fireEvent.click(screen.getByRole('button', { name: /What does "censored", "stale" or "suspect" mean\?/ }));
    expect(screen.getByRole('complementary', { name: 'explain' })).toHaveTextContent(/censored/i);
  });
});

describe('detail models', () => {
  it('detailWalk has one step per leg in S and ends at Q*', () => {
    const d = episodeDetailFixture();
    const w = detailWalk(d);
    expect(w.steps.length).toBe(d.legs.filter((l) => l.in_s).length);
    expect(w.q_star).toBe(d.episode.q_star);
  });
  it('detailTimeline uses observations as updates after the open', () => {
    const d = episodeDetailFixture();
    const t = detailTimeline(d);
    expect(t.open_ms).toBe(d.episode.opened_ms);
    expect(t.updates.length).toBe(1 + d.observations.filter((o) => !o.skipped && o.observed_ms !== null).length);
  });
});
```

Check the `CardsResponse` shape in `types.ts` and make the mocked body match it (e.g. `{ win: '24h', items: [] }` or whatever the type says).

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npx vitest run src/explain/episodes.test.tsx`
Expected: FAIL, "does not provide an export named 'detailTimeline'".

- [ ] **Step 3: Implement**

Append to `web/src/explain/models.ts` (add the imports at the top of the file):

```ts
import type { EpisodeDetail } from '../api/types';
import type { EpisodeModel, WalkModel } from '../schematics/models';

/** Spec §7.3: simplified, with one bar per leg in S at that leg's VWAP, up to Q*. Per-level steps
 *  are not stored, so the caption says so. Money figures shown are the api's (net, gross). */
export function detailWalk(d: EpisodeDetail): WalkModel {
  const legs = d.legs.filter((l) => l.in_s);
  const payoff = d.episode.kind.startsWith('no') ? String(Math.max(1, legs.length - 1)) : '1';
  return {
    steps: legs.map((l) => ({ q_from: '0', q_to: d.episode.q_star, cost: l.vwap ?? '0', payoff, profitable: true })),
    q_star: d.episode.q_star,
    pi: d.episode.net,
    pi_gross: d.episode.gross,
  };
}

export function detailTimeline(d: EpisodeDetail): EpisodeModel {
  const e = d.episode;
  const obs = d.observations.filter((o) => !o.skipped && o.observed_ms !== null);
  return {
    updates: [{ t_ms: e.opened_ms, net: e.net }, ...obs.map((o) => ({ t_ms: o.observed_ms as number, net: o.still_profitable ? o.pnl : null }))],
    open_ms: e.opened_ms,
    close_ms: e.closed_ms,
    peak_ms: e.peak_ms,
    peak_net: e.net,
    t_x: null,
    captured: null,
    caught: false,
  };
}
```

Add to `copy.ts`:

```ts
export const EPISODES = {
  what: 'An episode is one stretch of time in which a full basket cost less than it pays, after taker fees. It opens on the first order-book update where that is true and closes on the first where it is not.',
  duration: (ms: number | null) => (ms === null ? 'This one is still open.' : `This one lasted ${ms} ms.`),
  flagsIntro: 'Each flag is a reason to trust the row a little less. None of them means the prices were fake.',
  costCaption: 'simplified: per-level steps are not stored',
  cost: (net: string, gross: string) => `At the peak: ${money(net)} net profit at the peak, ${money(gross)} profit before taker fees, taking all the depth (on the table, not captured).`,
  after: 'Each dot after the open is a re-check: the books looked at again shortly after the episode opened. A filled dot means it was still profitable then.',
  caught: (dur: number | null, ms: number | null) =>
    ms === null ? 'Our latency is not measured yet, so we cannot say.' : dur === null ? `Still open. A trader at ${ms} ms would have arrived while it was on.` : dur > ms ? `It lasted ${dur} ms; a trader at ${ms} ms would have arrived in time (whether they got filled is another question).` : `It lasted ${dur} ms; a trader at ${ms} ms would have arrived after it closed.`,
};
```

Add the questions in `questions.tsx` (imports: `DepthWalk`, `EpisodeTimeline`, `FLAG_WORDS`, `useApi`, `api`, `detailWalk`, `detailTimeline`, `EPISODES`):

```tsx
function Caught({ s }: { s: ExplainState }) {
  const e = s.data.episode?.detail?.episode;
  const cards = useApi((sig) => api.cards('24h', sig), []);
  const edge = cards.data?.items?.find((c) => c.card === 'EDGE');
  const ms = edge?.series.measured?.latency_ms ?? null;
  if (cards.loading) return <p>…</p>;
  return (
    <>
      <p>{EPISODES.caught(e?.duration_ms ?? null, ms)}</p>
      {edge?.series.decay && (
        <ul>
          {edge.series.decay.map((p) => (
            <li key={p.scenario}>{p.scenario}: {p.latency_ms} ms, captured {money(p.captured)} (paper)</li>
          ))}
        </ul>
      )}
    </>
  );
}

QUESTIONS.episodes = [
  {
    id: 'what-episode',
    q: 'What is an episode?',
    Answer: ({ s }) => {
      const e = s.selection?.kind === 'episode' ? s.selection.episode : null;
      return (
        <>
          <p>{EPISODES.what}</p>
          {e ? <p>{EPISODES.duration(e.duration_ms)}</p> : <p><i>example</i>: see the math page, section e.</p>}
        </>
      );
    },
  },
  {
    id: 'flags-row',
    q: 'What does "censored", "stale" or "suspect" mean?',
    when: (s) => s.selection?.kind === 'episode',
    Answer: ({ s }) => {
      const e = s.selection?.kind === 'episode' ? s.selection.episode : null;
      const on = e ? (['censored', 'stale', 'suspect'] as const).filter((f) => e[f]) : [];
      return (
        <>
          <p>{EPISODES.flagsIntro}</p>
          <ul>{on.length === 0 ? <li>This row has none of them.</li> : on.map((f) => <li key={f}><b>{f}</b>: {FLAG_WORDS[f]}</li>)}</ul>
        </>
      );
    },
  },
  {
    id: 'flags',
    q: 'What do the flags mean?',
    Answer: () => <ul>{Object.entries(FLAG_WORDS).map(([k, v]) => <li key={k}><b>{k}</b>: {v}</li>)}</ul>,
  },
];

QUESTIONS.episode = [
  {
    id: 'cost',
    q: 'What would buying this have cost?',
    when: (s) => !!s.data.episode?.detail,
    Answer: ({ s }) => {
      const d = s.data.episode?.detail;
      if (!d) return <p>…</p>;
      return (
        <>
          <DepthWalk compact model={detailWalk(d)} caption={EPISODES.costCaption} />
          <p>{EPISODES.cost(d.episode.net, d.episode.gross)}</p>
        </>
      );
    },
  },
  {
    id: 'after-open',
    q: 'What happened after it opened?',
    when: (s) => !!s.data.episode?.detail,
    Answer: ({ s }) => {
      const d = s.data.episode?.detail;
      return d ? (<><EpisodeTimeline compact model={detailTimeline(d)} /><p>{EPISODES.after}</p></>) : <p>…</p>;
    },
  },
  { id: 'caught', q: 'Would we have caught it?', when: (s) => !!s.data.episode?.detail, Answer: Caught },
];
```

Check the keys and shape of `FLAG_WORDS` in `src/lib/flags.ts` (`grep -n "FLAG_WORDS" -A 12 web/src/lib/flags.ts`). If its keys aren't `censored`/`stale`/`suspect`, map the three row booleans to the matching keys it has.

Pages:
- In `Episodes.tsx`, after `const page = useApi(…)`, add `useExplainData('episodes', { items: page.data?.items });` and append the same `explain` column to `columns`: `{ key: 'explain', label: '', render: (r) => <ExplainButton selection={{ kind: 'episode', episode: r }} /> }`.
- In `EpisodeDetail.tsx`, after `const d = useApi(…)`, add `useExplainData('episode', { detail: d.data });`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && npx vitest run && npx tsc --noEmit`
Expected: PASS. Existing Episodes tests may count columns; update them for the new column.

- [ ] **Step 5: Commit**

```bash
git add web/src/explain web/src/pages/Episodes.tsx web/src/pages/EpisodeDetail.tsx
git commit -m "web: explain Episodes and Episode detail (simplified walk, re-check timeline, one lazy cards fetch)"
```

---

### Task 14: Explain on Paper, Papers, Health; the fetch rule and honesty over every page

**Files:**
- Create: `web/src/explain/pages.test.tsx`
- Modify: `web/src/explain/questions.tsx`, `web/src/explain/copy.ts`, `web/src/pages/Paper.tsx`, `web/src/pages/Papers.tsx`, `web/src/pages/Health.tsx`

**Interfaces:**
- Produces:
  - Paper questions `making-money`, `locked` and `policies`;
  - Papers questions `card-tests` and `agree` (both need a selected card, or fall back to the first card);
  - Health question `seeing-everything` (replaces Task 11's stand-in);
  - `daysTo(endDate: string | null, nowMs: number): string` in `copy.ts` ("unknown" for null or unparsable).

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/explain/pages.test.tsx
import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PageData } from './ExplainContext';
import { accountFixture, cardFixture, healthFixture, positionFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { daysTo } from './copy';
import { Explain } from './Explain';
import { ExplainProvider, useExplainData } from './ExplainContext';
import { checkHonesty } from './honesty';
import { QUESTIONS } from './questions';

function Pub<P extends keyof PageData>({ page, data }: { page: P; data: NonNullable<PageData[P]> }) {
  useExplainData(page, data);
  return null;
}

const DATA: { [P in keyof PageData]-?: NonNullable<PageData[P]> } = {
  radar: { open: [], today: undefined, edgeToday: undefined },
  episodes: { items: [] },
  episode: { detail: undefined },
  paper: { accounts: [accountFixture()], positions: [positionFixture(), positionFixture({ end_date: null, market_id: 'm2' })] },
  papers: { cards: [cardFixture()] },
  health: { health: healthFixture(), integrity: { from_ms: 0, to_ms: 1, by_kind: [], total: 20, msgs: 308_000_000, per_million: '0.06' } },
};

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal('matchMedia', undefined);
});

describe('every page, every question', () => {
  for (const page of Object.keys(DATA) as (keyof PageData)[]) {
    it(`${page}: renders, stays honest, and fetches nothing (except the named question)`, () => {
      const fetch = mockApi({ '/api/cards': { win: '24h', items: [] } });
      render(<MemoryRouter><ExplainProvider><Pub page={page} data={DATA[page]} /><Explain /></ExplainProvider></MemoryRouter>);
      fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
      const panel = screen.getByRole('complementary', { name: 'explain' });
      for (const b of within(panel).queryAllByRole('button').filter((x) => x.className === 'q')) {
        if (b.textContent === 'Would we have caught it?') continue;
        fireEvent.click(b);
        expect(checkHonesty(panel.textContent ?? ''), `${page}: ${b.textContent}`).toEqual([]);
      }
      expect(fetch).not.toHaveBeenCalled();
    });
  }
  it('every page has at least one question', () => {
    for (const p of Object.keys(QUESTIONS)) expect(QUESTIONS[p as keyof typeof QUESTIONS].length, p).toBeGreaterThan(0);
  });
});

describe('Paper', () => {
  it('unknown_end_date_reads_unknown', () => {
    expect(daysTo(null, 0)).toBe('unknown');
    expect(daysTo('not a date', 0)).toBe('unknown');
    expect(daysTo('2026-10-29T00:00:00Z', Date.UTC(2026, 9, 27))).toBe('2 days');
  });
  it('Why is money locked? lists positions with days to resolution, paper-labelled', () => {
    mockApi({});
    render(<MemoryRouter><ExplainProvider><Pub page="paper" data={DATA.paper} /><Explain /></ExplainProvider></MemoryRouter>);
    fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Why is money locked?' }));
    const panel = screen.getByRole('complementary', { name: 'explain' });
    expect(panel).toHaveTextContent('unknown');
    expect(panel).toHaveTextContent(/paper/);
  });
});
```

Check that `accountFixture` and `cardFixture` exist in `src/test/fixtures.ts` (`grep -n "export function" web/src/test/fixtures.ts`). Use the names that exist; if there's no card fixture, build one inline from the `CardBody` type with the fields `Papers.test.tsx` already uses. The Health `Integrity` literal must match `types.ts`.

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npx vitest run src/explain/pages.test.tsx`
Expected: FAIL, "does not provide an export named 'daysTo'".

- [ ] **Step 3: Implement**

Add to `copy.ts`:

```ts
export function daysTo(endDate: string | null, nowMs: number): string {
  if (endDate === null) return 'unknown';
  const t = Date.parse(endDate);
  if (Number.isNaN(t)) return 'unknown';
  const d = Math.ceil((t - nowMs) / 86_400_000);
  return d <= 0 ? 'past its end date' : `${d} day${d === 1 ? '' : 's'}`;
}

export const PAPER = {
  making: 'Paper accounts: each starts with a bankroll and trades only on paper, at a simulated latency. Equity is cash plus what open positions would sell for now.',
  account: (policy: string, equity: string | null, bankroll: string, realized: string | null, unrealized: string | null) =>
    `${policy}: equity ${money(equity)} against a ${money(bankroll)} bankroll (paper). Realized ${money(realized)}, unrealized ${money(unrealized)}.`,
  locked: 'A YES basket pays at resolution, so its cost stays locked until the event ends. Money locked for months does nothing else meanwhile.',
  position: (title: string, basis: string, days: string) => `${title}: ${money(basis)} locked (paper), resolves in ${days}.`,
  policies: [
    'P0: NO baskets, converted to collateral straight away.',
    'P1: YES baskets held to resolution, only when the yearly rate clears its hurdle.',
    'P2: the depth-aware variant; it waits for size.',
    'P3: the naive one. It buys incomplete sets too, as a control.',
  ],
};

export const PAPERS = {
  tests: 'Each card takes one claim from a paper and measures it again on our own capture. PAPER is their number, THEIR METHOD is their method on our data, OUR METHOD is ours.',
  agree: (status: string) => `Status: ${status}. A difference is not an error by itself: the markets, the dates and the latency are different.`,
};

export const HEALTH = {
  hearts: 'Ten hearts is a healthy scanner. Each trouble costs hearts and is named next to them.',
  line: (shards: number, parked: number, drops: number, lag: number | null) => `Shards connected: ${shards - parked} of ${shards}. Messages dropped in the last 5 minutes: ${drops}. Feed lag p50: ${lag === null ? 'not measured' : `${lag} ms`}.`,
  integrity: (perMillion: string | null) => `Integrity problems per million messages: ${perMillion ?? 'not measured'}. For scale, Gebele et al. saw 20 in 308 million.`,
};
```

Add the questions (imports: `PAPER`, `PAPERS`, `HEALTH`, `daysTo`; `policyLabel` from `../pages/Paper` would pull a page module into explain. That's allowed on the dashboard, but copy the one-line label logic instead to keep explain independent of pages):

```tsx
QUESTIONS.paper = [
  {
    id: 'making-money',
    q: 'Are we making money?',
    Answer: ({ s }) => (
      <>
        <p>{PAPER.making}</p>
        <ul>
          {(s.data.paper?.accounts ?? []).map((a) => (
            <li key={a.account_id}>{PAPER.account(a.account_id, a.equity, a.bankroll, a.realized, a.unrealized)}</li>
          ))}
        </ul>
      </>
    ),
  },
  {
    id: 'locked',
    q: 'Why is money locked?',
    Answer: ({ s }) => {
      const now = Date.now();
      const open = (s.data.paper?.positions ?? []).filter((p) => !p.returned);
      return (
        <>
          <p>{PAPER.locked}</p>
          <ul>
            {open.length === 0 ? <li>No position is waiting for a resolution.</li> : open.map((p) => (
              <li key={`${p.account_id}|${p.market_id}|${p.outcome}`}>{PAPER.position(p.event_title ?? p.market_id, p.basis, daysTo(p.end_date, now))}</li>
            ))}
          </ul>
        </>
      );
    },
  },
  { id: 'policies', q: 'Why do the policies differ?', Answer: () => <ul>{PAPER.policies.map((l) => <li key={l}>{l}</li>)}</ul> },
];

function pickCard(s: ExplainState) {
  return s.selection?.kind === 'card' ? s.selection.card : s.data.papers?.cards?.[0] ?? null;
}

QUESTIONS.papers = [
  { id: 'card-tests', q: 'What does this card test?', Answer: ({ s }) => { const c = pickCard(s); return <><p>{PAPERS.tests}</p>{c && <p><b>{c.title}</b></p>}</>; } },
  { id: 'agree', q: 'Do we agree with the paper, and why not?', Answer: ({ s }) => { const c = pickCard(s); return c ? <><p>{PAPERS.agree(c.status)}</p><ul>{c.notes?.map((n) => <li key={n}>{n}</li>)}</ul></> : <p>No card loaded yet.</p>; } },
];

QUESTIONS.health = [
  {
    id: 'seeing-everything',
    q: 'Are we seeing everything?',
    Answer: ({ s }) => {
      const h = s.data.health?.health;
      return (
        <>
          <p>{HEALTH.hearts}</p>
          {h && <p>{HEALTH.line(h.shards, h.parked_shards, h.drops_5m, h.lag_p50_ms)}</p>}
          <p>{HEALTH.integrity(s.data.health?.integrity?.per_million ?? null)}</p>
        </>
      );
    },
  },
];
```

Check the `CardBody` field names used here (`title`, `status`, and whatever holds method notes: `grep -n "interface CardBody" -A 16 web/src/api/types.ts`), and use the actual field for notes (e.g. `notes` or `method_notes`).

Pages:
- In `Paper.tsx`, inside `Paper()`, add `useExplainData('paper', { accounts: accounts.data?.items, positions: positions.data?.items });` and append to `positionColumns`: `{ key: 'explain', label: '', render: (p) => <ExplainButton selection={{ kind: 'position', position: p }} /> }`.
- In `Papers.tsx`, add `useExplainData('papers', { cards: cards.data?.items });` (use the actual list field of `CardsResponse`). Where each `CardView` is rendered, render `<ExplainButton selection={{ kind: 'card', card }} label="explain card" />` next to it.
- In `Health.tsx`, add `useExplainData('health', { health: now.data, integrity: integrity.data });`.

- [ ] **Step 4: Run everything**

Run: `cd web && WASM_REQUIRED=1 npx vitest run && npm run build`, then `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all green; the math bundle is still under 350 KB and has no dashboard modules.

- [ ] **Step 5: Manual check (owner, local)**

Run `cd web && npm run dev`, open `http://localhost:5173/` at a width ≥ 1500 px (docked) and at 360 px (drawer, no horizontal scroll), and open `http://localhost:5173/math.html`. Report anything that reads wrong.

- [ ] **Step 6: Commit**

```bash
git add web/src/explain web/src/pages/Paper.tsx web/src/pages/Papers.tsx web/src/pages/Health.tsx
git commit -m "web: explain Paper, Papers and Health; fetch rule and honesty checked on every page"
```

---

## Spec coverage (self-review)

| Spec | Task |
|---|---|
| §3.1 payoff | 1 |
| §3.2 d1_sums, §3.4 walk_steps | 2 |
| §3.3 classify (+ post_end note) | 3, 6 |
| §4.1 facade, limits, errors, teaching model | 4 |
| §4.2 build, tsconfig split, WASM_REQUIRED, Docker cargo-chef | 5, 9 |
| §4.3 parity (strings, native alone + workspace, WASM) | 4, 5 |
| §4.4 size budget, moduleIds guard | 8 |
| §5.1 separate build | 8 |
| §5.2 nginx + gzip (gzip_static chosen: `.gz` written by the size check) | 8, 9 |
| §5.3 Traefik router, limits per IP, CSP | 9 |
| §5.4–5.5 flood test and its limits | 9 (runbook) |
| §6 playground sections a–e | 6, 7, 8 |
| §7.1 panel shape | 11 |
| §7.2 data, fetch rule, honesty | 11–14 |
| §7.3 questions per page | 12, 13, 14 |
| §7.4 Position.end_date | 10 |
| §8 testing | every task |
| §9 deploy runbook | 9 |
