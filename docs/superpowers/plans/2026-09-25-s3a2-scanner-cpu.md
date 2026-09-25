# S3a-2 · scanner CPU: the spec's detector threshold gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the engine from depth-walking both baskets on every frame. It will run a detector only when the top-of-book sums cross that detector's threshold (spec §5.4, §8.2), with byte-identical outputs, so the 2-vCPU VPS keeps up with the full universe.

**Architecture:**
- `arb-core` gets two pure O(legs) gates, `yes_basket_may_signal` and `no_basket_may_signal`. The plan proves (and a property test checks) that a false gate means the detector returns `None`.
- `Engine::evaluate` calls a detector only when its gate is true or an episode is open for that key. The tracker still gets every state, so censoring and closes don't change.
- A `#[ignore]`d bench measures engine µs/frame with the gate off (the old behaviour) and on, in one binary. It runs over one synthetic high-rate script.

**Tech Stack:** Rust 1.98.1 (edition 2024), `rust_decimal`, `proptest` (already an arb-core dev-dependency), `sha2` (already a scanner dependency). No new crates.

**Spec:** `docs/superpowers/specs/2026-09-22-arb-scanner-design.md`: §5.4 ("It's only run when the running top-of-book sums cross a fee-adjusted threshold (§8.2)"), §8.2 (engine: "groups: running ΣA, ΣB, Σφ per event (O(1) update); threshold crossed → detectors"), §9.3 (the engine is pure, replay is deterministic), §15 (boundaries: "ΣA = 1.000000 → none; 0.999999 → yes; … ΣB = 1.000000 → none"). It builds on `docs/superpowers/plans/2026-09-25-s3a-scanner-hardening.md`, which is implemented.

**Evidence (VPS soak run 2, 2 vCPU, full universe of ~56k tokens, ~4.3k frames/s):**
- scanner CPU 190–290 %, load average 3–4;
- 14 `drops` in 105 min. A drop is `pm_api::ApiError::Dropped`: the per-shard channel is full because the engine loop is behind on the shared 50 000-slot channel, so a frame is lost and the connection is cut and resubscribed;
- p99 lag spikes of 20–36 s.

**Root cause:** `Engine::evaluate` (`bins/scanner/src/engine.rs`, `pub(crate) fn evaluate`) runs `detect_yes_basket` **and** `detect_no_basket` in full on every frame that touches a group. Each call clones every live leg's whole ladder (`asks_best_first` / `no_asks_best_first`) and walks it twice (fee walk + zero-fee walk, `arb-core/src/detect.rs::run`). That is the deviation from §5.4/§8.2.

## Global Constraints

- PAPER TRADING ONLY. No order placement, no wallet, no keys, no signing code.
- Money and prices are never f32/f64. The workspace clippy config denies `float_arithmetic`, `unwrap_used` and `expect_used`, so there is no `unwrap`/`expect` outside `#[cfg(test)]` code and test files. Time is integer ms/µs, and amounts are `Decimal`. The bench prints ns/µs with integer division only.
- **The engine is pure** (spec §9.3). `engine.rs` never reads a clock and never does IO. The new gate and counter are functions of the engine's inputs, so live and replay stay identical. `Instant` is used only in the bench test file.
- **Outputs are byte-identical.** Every existing test in `bins/scanner/tests/{engine,engine_checks,replay}.rs` and `crates/arb-core/tests/*.rs` passes **unchanged**. "Byte-identical" is checked on `format!("{:?}")` of each `Out`, because `Decimal`'s `PartialEq` ignores scale (0.50 == 0.5) but its `Debug` doesn't.
- Disk budget: the project must stay under 30 GB (`target/debug` is ~3.1 GB today). Use debug `cargo test`/`clippy` only, with **one** exception: the single release build in Task 3 Step 6, which the owner allows. Delete it afterwards with `cargo clean --release`. No `docker build`.
- Commit messages are plain, with **no** `Co-Authored-By` or any attribution trailer. Stage only the files you touch (never `PROJECT.md` / `README.md`).
- Bash prefix for every command: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`
- Coordination: the S3c plan also edits `engine.rs` (`open_episode` → `on_open`). This plan changes only the top of the `for det in …` loop in `evaluate`, plus two fields and two accessors. If S3c lands first, apply the same hunk on top of it. The gate doesn't touch `open_episode`.

## Review Focus

1. **An open episode whose state turns non-positive** must close at that frame's own ts, exactly as before. The gate must never be the reason a close is late or missing. In `evaluate`, an open key always runs its detector, and the tracker gets `None` either way. Pinned in Task 2 (`the_threshold_gate_skips_detectors_until_the_top_sums_cross`, third frame).
2. **Fully gated frames must still reach the tracker.** `EpisodeTracker::on_state` marks the key as seen even on `None`. Without that, the next Open would be wrongly `censor_left = true`. Pinned by the existing `yes_basket_opens_then_closes_with_exact_timing` (`!r.censor_left`, since its seed frame is now fully gated) and by Task 2's equivalence test (disconnects every 4 000 frames reset first sight).
3. **Six-decimal boundaries** (spec §15): ΣA = 1.000000 → no YES basket, 0.999999 → yes; ΣB = 1.000000 → no NO basket, 1.000001 → yes. Pinned in Task 1 (`gate_boundaries_match_the_detectors_at_six_decimals`).
4. **A gross-positive, net-non-positive basket** (fees eat the edge) must still reach the detector. The detectors emit gross signals (`Signal.gross` feeds paper comparisons), so the gate is the detectors' own gross threshold, not a fee-adjusted one. Pinned in Task 1 (`gate_uses_the_gross_threshold_so_fee_eaten_baskets_still_reach_the_detector`).
5. **A crossed book with Σ asks < 1** opens the gate, but D1 still refuses it. The gate only ever skips work; it can never create a signal. Pinned in Task 1 (`a_crossed_book_opens_the_gate_but_the_detector_still_refuses`, plus the proptest's crossed cases) and by the existing `crossed_book_is_flagged_and_never_an_opportunity`.

## File Structure

```
crates/arb-core/src/detect.rs          MODIFY: yes_basket_may_signal, no_basket_may_signal (+ the proof as doc comments)
crates/arb-core/tests/detect.rs        MODIFY: boundary/edge unit tests + gate proptest
bins/scanner/src/engine.rs             MODIFY: threshold_gate + detector_calls fields, set_threshold_gate, detector_calls(), gated evaluate loop
bins/scanner/tests/common/mod.rs       MODIFY: deterministic synthetic high-rate script (Synth, synth_engine, drive)
bins/scanner/tests/engine.rs           MODIFY: gate counter test + gate on/off equivalence test
bins/scanner/tests/bench_engine.rs     NEW: #[ignore]d µs/frame bench, gate off vs on, digest-equal
bins/scanner/src/capture.rs            MODIFY: ZSTD_LEVEL const (stays 3) carrying the ruling
config/scanner.toml                    MODIFY: comment on engine.channel_capacity (value unchanged)
```

---

### Task 1: arb-core — the D2a/D2b threshold gates, proved and property-tested

**Files:**
- Modify: `crates/arb-core/src/detect.rs` (add two pub fns after `detect_no_basket`)
- Test: `crates/arb-core/tests/detect.rs` (append)

**Interfaces:**
- Consumes: `Group`, `GroupLeg`, `LegStatus`, `Book::top()`, `detect_yes_basket`, `detect_no_basket` (existing).
- Produces:
  - `pub fn yes_basket_may_signal(g: &Group) -> bool`
  - `pub fn no_basket_may_signal(g: &Group) -> bool`
  - Contract: `false` ⇒ the matching detector returns `None`. `true` on an undecided group (no `ClosedYes` leg) whose relevant legs are uncrossed ⇒ it returns `Some`.

**Why this gate is exact (the proof; it also goes into the doc comments).**

*Facts used.*
- (F1) `Price` and `Shares` have ≤ 6 dp. A group has ≤ 100 legs. So every sum and difference of prices below has ≤ 6 dp and magnitude ≤ 101, far inside `Decimal`'s 28 digits: `+`/`−` are exact, and the usual algebra holds.
- (F2) A `Book` holds only sizes > 0 (`apply_snapshot` skips 0; `apply_delta` removes on 0; `Shares` is ≥ 0).
- (F3) `asks_best_first()[0] == top().best_ask`, and `no_asks_best_first()[0] == top().best_bid.complement()`.
- (F4) `detect_*` returns `Some` only if `gross.pnl_gross > 0`, where `gross` is `walk` over the **same legs with `FeeSchedule::ZERO`** (`detect.rs::run`). So φ ≡ 0 in that walk.
- (F5) `walk`'s first loop iteration computes m = payoff − Σ prices − Σ φ over the active legs at their first level. If m ≤ 0 it `break`s before taking anything, and then payout = cost = 0, so `pnl_gross = 0`.

*D2a.* `detect_yes_basket` returns `None` before walking unless all of these hold: the group is undecided, ≥ 1 leg is live, no live leg is crossed, and every live leg has an ask. In that case `AllLegs { payoff: 1 }` starts with all n live legs active at their best asks (F3). By F4, m = 1 − ΣA, where ΣA is the sum of the live legs' best asks, exact by F1.
- If ΣA ≥ 1: m ≤ 0, so by F5 `pnl_gross = 0` and the detector returns `None`.
- `yes_basket_may_signal` returns false exactly when there is no live leg, when some live leg has no ask (both make the detector return `None` too), or when ΣA ≥ 1. So **false ⇒ `None`**.
- Conversely, if the pre-checks pass and ΣA < 1: the first take is min(first-level sizes) > 0 (F2). Every segment the walk takes has m_gross = m > 0, so `pnl_gross` = Σ m_s·take_s > 0 and the detector returns **`Some`**.

*D2b.* `detect_no_basket` returns `None` unless the group is undecided, the "used" legs (live, with a bid) number k ≥ 2, and none of them is crossed. `NestedNo` starts with all k used legs active (each has ≥ 1 NO level). Its payoff is k − 1, and by F3 its prices are 1 − b_i. So m = (k − 1) − Σ(1 − b_i) = ΣB − 1, exact by F1, where ΣB is the sum of the used legs' best bids.
- If ΣB ≤ 1: m ≤ 0, so `pnl_gross = 0` and the detector returns `None`.
- `no_basket_may_signal` returns false exactly when k < 2 (the detector's own `None`) or ΣB ≤ 1. So **false ⇒ `None`**.
- The converse follows as for D2a, since each taken NestedNo segment has m > 0.

*Crossed or decided groups.* The gate ignores them on purpose. There it may return true, the detector runs, and it returns `None` as it always did. The gate never produces a signal itself: the engine either calls the unchanged detector or uses `None`.

- [ ] **Step 1: Write the failing tests.** Append to `crates/arb-core/tests/detect.rs`. The file already imports `arb_core::detect::*`, `Book`, `FeeSchedule`, `Price`, `Shares`, `proptest::prelude::*`, `Decimal` and `dec`, and it defines `live(bid, ask)`.
```rust
/// A live, zero-fee leg with at most one level per side, size 10.
fn leg6(bid: Option<&str>, ask: Option<&str>) -> GroupLeg {
    let s = Shares::parse("10").unwrap();
    let bids: Vec<(Price, Shares)> = bid.map(|p| (Price::parse(p).unwrap(), s)).into_iter().collect();
    let asks: Vec<(Price, Shares)> = ask.map(|p| (Price::parse(p).unwrap(), s)).into_iter().collect();
    let mut b = Book::default();
    b.apply_snapshot(&bids, &asks, 0);
    GroupLeg {
        status: LegStatus::Live,
        book: b,
        fee: FeeSchedule::ZERO,
    }
}

fn grp(legs: Vec<GroupLeg>) -> Group {
    Group {
        legs,
        augmented: false,
    }
}

#[test]
fn gate_boundaries_match_the_detectors_at_six_decimals() {
    // Spec §15: ΣA = 1.000000 → none; 0.999999 → yes.
    let at = grp(vec![leg6(None, Some("0.400000")), leg6(None, Some("0.600000"))]);
    assert!(!yes_basket_may_signal(&at));
    assert!(detect_yes_basket(&at).is_none());
    let under = grp(vec![leg6(None, Some("0.399999")), leg6(None, Some("0.600000"))]);
    assert!(yes_basket_may_signal(&under));
    assert!(detect_yes_basket(&under).is_some());
    // ΣB = 1.000000 → none; 1.000001 → yes.
    let at = grp(vec![leg6(Some("0.400000"), None), leg6(Some("0.600000"), None)]);
    assert!(!no_basket_may_signal(&at));
    assert!(detect_no_basket(&at).is_none());
    let over = grp(vec![leg6(Some("0.400001"), None), leg6(Some("0.600000"), None)]);
    assert!(no_basket_may_signal(&over));
    assert!(detect_no_basket(&over).is_some());
}

#[test]
fn gate_is_false_when_a_leg_is_missing_its_side_or_nothing_is_live() {
    // YES: a live leg with no ask can't be in a complete basket, however cheap the others are.
    let g = grp(vec![leg6(None, Some("0.10")), leg6(Some("0.05"), None)]);
    assert!(!yes_basket_may_signal(&g));
    assert!(detect_yes_basket(&g).is_none());
    // YES: no live leg at all.
    let mut dark = grp(vec![leg6(None, Some("0.10"))]);
    dark.legs[0].status = LegStatus::Placeholder;
    assert!(!yes_basket_may_signal(&dark));
    assert!(detect_yes_basket(&dark).is_none());
    // NO: only one live leg has a bid (D2b needs ≥ 2), even at 0.99.
    let g = grp(vec![leg6(Some("0.99"), None), leg6(None, Some("0.50"))]);
    assert!(!no_basket_may_signal(&g));
    assert!(detect_no_basket(&g).is_none());
    // Non-live legs never count: live asks 0.30 + 0.30 < 1 with a ClosedNo leg at 0.50.
    let mut g = grp(vec![
        leg6(None, Some("0.30")),
        leg6(None, Some("0.30")),
        leg6(None, Some("0.50")),
    ]);
    g.legs[2].status = LegStatus::ClosedNo;
    assert!(yes_basket_may_signal(&g));
    assert!(detect_yes_basket(&g).is_some());
}

#[test]
fn gate_uses_the_gross_threshold_so_fee_eaten_baskets_still_reach_the_detector() {
    // m3's books: gross edge 0.20 > 0, net exactly 0. The detector reports the gross signal
    // (Signal.gross feeds paper comparisons), so the gate must not hide it.
    let f = FeeSchedule::new(dec!(0.5), 1).unwrap();
    let mut a = live("0.10", "0.20");
    let mut b = live("0.50", "0.60");
    a.fee = f;
    b.fee = f;
    let g = grp(vec![a, b]);
    assert!(yes_basket_may_signal(&g));
    let s = detect_yes_basket(&g).unwrap();
    assert!(!s.net_positive());
}

#[test]
fn a_crossed_book_opens_the_gate_but_the_detector_still_refuses() {
    // Σ asks = 0.50 < 1, but leg 0 is crossed (bid 0.40 > ask 0.30): D1 wins.
    let g = grp(vec![leg6(Some("0.40"), Some("0.30")), leg6(None, Some("0.20"))]);
    assert!(yes_basket_may_signal(&g));
    assert!(detect_yes_basket(&g).is_none());
}

fn arb_leg() -> impl Strategy<Value = GroupLeg> {
    let status = prop_oneof![
        6 => Just(LegStatus::Live),
        1 => Just(LegStatus::Placeholder),
        1 => Just(LegStatus::ClosedNo),
        1 => Just(LegStatus::ClosedYes),
        1 => Just(LegStatus::Halted),
    ];
    let side = || prop::collection::vec((1u32..=99, 1u32..=50), 0..=3); // (cents, shares)
    (status, side(), side(), 0u32..=25, 1u8..=4).prop_map(|(status, bids, asks, rate, exp)| {
        let lv = |v: &[(u32, u32)]| -> Vec<(Price, Shares)> {
            v.iter()
                .map(|&(c, s)| {
                    (
                        Price::new(Decimal::new(c.into(), 2)).unwrap(),
                        Shares::new(Decimal::from(s)).unwrap(),
                    )
                })
                .collect()
        };
        let mut book = Book::default();
        book.apply_snapshot(&lv(&bids), &lv(&asks), 0);
        GroupLeg {
            status,
            book,
            fee: FeeSchedule::new(Decimal::new(rate.into(), 2), exp).unwrap(),
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// S3a-2: a false gate ⇒ the detector returns None (the engine may skip it); a true gate on
    /// an undecided group with uncrossed relevant legs ⇒ Some (the gate is exact, not loose).
    #[test]
    fn gate_false_means_no_signal_and_gate_true_is_exact(
        legs in prop::collection::vec(arb_leg(), 1..=6),
        augmented in any::<bool>(),
    ) {
        let g = Group { legs, augmented };
        let decided = g.legs.iter().any(|l| l.status == LegStatus::ClosedYes);

        let yes = detect_yes_basket(&g);
        let live_crossed = g
            .legs
            .iter()
            .any(|l| l.status == LegStatus::Live && l.book.is_crossed());
        if !yes_basket_may_signal(&g) {
            prop_assert!(yes.is_none(), "YES gate false but the detector signalled");
        } else if !decided && !live_crossed {
            prop_assert!(yes.is_some(), "YES gate true, undecided, uncrossed, yet no signal");
        }

        let no = detect_no_basket(&g);
        let used_crossed = g.legs.iter().any(|l| {
            l.status == LegStatus::Live && l.book.top().best_bid.is_some() && l.book.is_crossed()
        });
        if !no_basket_may_signal(&g) {
            prop_assert!(no.is_none(), "NO gate false but the detector signalled");
        } else if !decided && !used_crossed {
            prop_assert!(no.is_some(), "NO gate true, undecided, uncrossed, yet no signal");
        }
    }
}
```

- [ ] **Step 2: Run the tests and check that they fail.**
Run: `cargo test -p arb-core --test detect gate`
Expected: compile error `cannot find function `yes_basket_may_signal` in this scope` (and the same for `no_basket_may_signal`).

- [ ] **Step 3: Implement the gates.** In `crates/arb-core/src/detect.rs`, insert right after `detect_no_basket` (before `detect_implication`):
```rust
/// Spec §5.4/§8.2 threshold for D2a: can `detect_yes_basket(g)` return `Some`? O(live legs),
/// reads only each live leg's top of book, allocates nothing.
///
/// Contract: `false` ⇒ `detect_yes_basket(g)` is `None`, so a caller may skip the depth walk.
/// `true` on an undecided group with no crossed live leg ⇒ it is `Some`.
///
/// Proof: the detector walks only when every live leg (≥ 1) has an ask. Its zero-fee walk
/// (`run(…).1`, the one `pnl_gross` comes from) then starts with every live leg at its best ask
/// (`asks_best_first()[0] == top().best_ask`), so its first-segment edge is m = 1 − ΣA, exact in
/// `Decimal` (≤ 6 dp prices, ≤ 100 legs). `walk` breaks before taking anything when m ≤ 0, which
/// leaves `pnl_gross = 0`, so the detector returns `None`. Conversely m > 0 takes a first segment
/// of positive size (books hold only sizes > 0), and every segment walked has m > 0, so
/// `pnl_gross > 0`. Decided and crossed groups are left to the detector, which rejects them itself.
/// The threshold is the detector's own GROSS one on purpose: D2a reports gross-positive signals
/// (`Signal.gross`) even when fees make them net-negative.
pub fn yes_basket_may_signal(g: &Group) -> bool {
    let mut sum_ask = Decimal::ZERO;
    let mut any_live = false;
    for leg in g.legs.iter().filter(|l| l.status == LegStatus::Live) {
        let Some((ask, _)) = leg.book.top().best_ask else {
            return false; // an incomplete basket: the detector returns None too
        };
        sum_ask += ask.get();
        any_live = true;
    }
    any_live && sum_ask < Decimal::ONE
}

/// Spec §5.4/§8.2 threshold for D2b: can `detect_no_basket(g)` return `Some`? O(live legs).
///
/// Contract: as `yes_basket_may_signal`. Proof: the detector's zero-fee `NestedNo` walk starts
/// with the k ≥ 2 live legs that have a YES bid, each at its best NO ask 1 − b_i
/// (`no_asks_best_first()[0] == top().best_bid.complement()`), paying k − 1, so its first edge is
/// m = (k − 1) − Σ(1 − b_i) = ΣB − 1, exact in `Decimal`. m ≤ 0 ⇒ `pnl_gross = 0` ⇒ `None`;
/// m > 0 ⇒ `pnl_gross > 0` ⇒ `Some` on an undecided group with no crossed used leg.
pub fn no_basket_may_signal(g: &Group) -> bool {
    let mut sum_bid = Decimal::ZERO;
    let mut used = 0usize;
    for leg in g.legs.iter().filter(|l| l.status == LegStatus::Live) {
        if let Some((bid, _)) = leg.book.top().best_bid {
            sum_bid += bid.get();
            used += 1;
        }
    }
    used >= 2 && sum_bid > Decimal::ONE
}
```

- [ ] **Step 4: Run the tests and check that they pass.**
Run: `cargo test -p arb-core --test detect`
Expected: PASS, including every pre-existing test in the file (they're unchanged).

- [ ] **Step 5: Run lint and the whole crate.**
Run: `cargo fmt --all --check && cargo clippy -p arb-core --all-targets -- -D warnings && cargo test -p arb-core`
Expected: no diff, no warnings, all tests pass. If `fmt --check` shows a diff, run `cargo fmt --all` and restage only the two files above.

- [ ] **Step 6: Commit.**
```bash
git add crates/arb-core/src/detect.rs crates/arb-core/tests/detect.rs
git commit -m "arb-core: D2a/D2b top-of-book threshold gates (spec 5.4/8.2), proved exact and property-tested"
```

---

### Task 2: engine — gate the detectors, same outputs

**Files:**
- Modify: `bins/scanner/src/engine.rs` (the `use arb_core::detect::{…}` import, the `Engine` struct, `Engine::new`, two new pub fns, the top of the `for det in [Det::Yes, Det::No]` loop in `evaluate`)
- Modify: `bins/scanner/tests/common/mod.rs` (append the synthetic script)
- Test: `bins/scanner/tests/engine.rs` (append two tests)

**Interfaces:**
- Consumes: `arb_core::detect::{yes_basket_may_signal, no_basket_may_signal}` (Task 1).
- Produces (Task 3 uses all of these):
  - `Engine::set_threshold_gate(&mut self, on: bool)` (default on; only tests and the bench turn it off)
  - `Engine::detector_calls(&self) -> u64`
  - in `tests/common`: `pub struct Synth { pub universe: Universe, pub conn_of: HashMap<String, usize>, .. }`, `Synth::new(groups: usize, seed: u64) -> Synth`, `Synth::script(&mut self, n: usize) -> Vec<SynthFrame>`, `pub struct SynthFrame { pub conn: usize, pub recv_ms: i64, pub events: Vec<MarketEvent> }`, `pub fn synth_engine(s: &Synth, gate: bool) -> Engine`, `pub fn drive(e: &mut Engine, frames: &[SynthFrame], drop_every: usize, sink: &mut dyn FnMut(Out))`

**Why the outputs can't change.** Look at what `evaluate` does with `sig`, per (group, det) key:
- `pnl = sig.filter(net_positive).map(quote.pnl)`.
- `tracker.on_state(&key, ts, pnl)` is **always** called, whether or not the detector ran.
- `sig` itself is read only inside `EpisodeEvent::Open` / `Peak`, which the tracker emits only when `pnl` is `Some`, and in the `n_updates` bump, which also requires `pnl.is_some()`.

When the gate is false, Task 1 proves `sig` would have been `None`. Skipping the detector therefore feeds the tracker the same `None` and changes nothing downstream. As a second guard, a key with an open episode (`open_by_key`) always runs its detector, so an episode's close never depends on the gate. `on_state(None)` with nothing open only records first sight (`seen`), and it's still called. `rest_snapshot` and `check_book` re-evaluations go through the same `evaluate`, so they're gated the same way.

**Recommended form (ruling):** recompute the sums in O(legs) with `Book::top()` on each evaluation, and don't keep an O(1) running cache. Books change through five paths: `apply_snapshot`, `apply_delta`, `trim_to_top`, `rest_snapshot` replacing a book, and `apply_universe` carrying or resetting books and leg statuses. A running ΣA/ΣB would have to be patched correctly in each one, and one missed path silently hides signals. The O(legs) gate is a handful of B-tree edge reads per leg, next to the ladder clones and two `Decimal` walks it replaces.

- [ ] **Step 1: Add the synthetic script to `bins/scanner/tests/common/mod.rs`.** This is a test fixture, not the code under test. Append:
```rust
// ---- S3a-2: deterministic synthetic high-rate script (equivalence test + bench) ----

/// Deterministic 64-bit LCG (MMIX constants). Fixtures only: no rand crate, no clock.
pub struct Lcg(u64);

impl Lcg {
    /// Uniform-enough in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).unwrap() % n
    }
}

fn cents(c: i64) -> Price {
    Price::new(Decimal::new(c, 2)).unwrap()
}

/// Levels in 1..=99 cents, 100 shares each.
fn ladder(cs: impl Iterator<Item = i64>) -> Vec<(Price, Shares)> {
    cs.filter(|c| (1..=99).contains(c))
        .map(|c| (cents(c), Shares::new(dec!(100)).unwrap()))
        .collect()
}

fn level(token: &str, ts: i64, side: Side, c: i64, size: Shares) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side,
        price: cents(c),
        size,
        best_bid: None,
        best_ask: None,
    }
}

struct SynthLeg {
    token: String,
    a0: i64,
    b0: i64,
    ask: i64,
    bid: i64,
}

pub struct SynthFrame {
    pub conn: usize,
    pub recv_ms: i64,
    pub events: Vec<MarketEvent>,
}

pub struct Synth {
    pub universe: Universe,
    pub conn_of: HashMap<String, usize>,
    legs: Vec<Vec<SynthLeg>>,
    rng: Lcg,
    ts: i64,
}

impl Synth {
    /// `groups` events. Event g has 2 + g % 11 legs (2..=12), and every third event has a 2 %
    /// taker fee. Even events hover around ΣA = 1 (YES baskets open now and then), odd ones
    /// around ΣB = 1 (NO baskets). Each token has a static ladder (≤ 15 levels a side) that never
    /// moves, plus one dynamic top level per side. The dynamic ask stays in [a0 − 2, a0 + 1] and
    /// the dynamic bid in [b0 − 1, b0 + 2], cents. These ranges don't overlap the static levels
    /// (asks ≥ a0 + 2, bids ≤ b0 − 2) or each other (b0 + 2 < a0 − 2), so no book is ever crossed
    /// and, with no server top on the deltas, no top check ever fires.
    pub fn new(groups: usize, seed: u64) -> Synth {
        let mut metas = Vec::with_capacity(groups);
        let mut legs = Vec::with_capacity(groups);
        for g in 0..groups {
            let n = 2 + g % 11;
            let event = format!("s{g}");
            let mut m = meta(&event, n);
            if g % 3 == 0 {
                for l in &mut m.legs {
                    l.fee_rate = dec!(0.02);
                }
            }
            let n_i = i64::try_from(n).unwrap();
            let yes_side = g % 2 == 0;
            let total = if yes_side { 100 + n_i } else { 100 - n_i }; // Σ a0 or Σ b0, cents
            let ls = (0..n_i)
                .map(|i| {
                    let share = total / n_i + i64::from(i < total % n_i);
                    let (a0, b0) = if yes_side {
                        (share, share - 5)
                    } else {
                        (share + 5, share)
                    };
                    SynthLeg {
                        token: format!("{event}-y{i}"),
                        a0,
                        b0,
                        ask: a0,
                        bid: b0,
                    }
                })
                .collect();
            metas.push(m);
            legs.push(ls);
        }
        let conn_of = metas
            .iter()
            .enumerate()
            .map(|(g, m)| (m.event_id.clone(), g % 40))
            .collect();
        Synth {
            universe: Universe {
                taken_ms: 0,
                groups: metas,
                canary: vec![],
            },
            conn_of,
            legs,
            rng: Lcg(seed),
            ts: 1_000,
        }
    }

    /// One snapshot frame per event at ts 1 000, then `n` delta frames. Each delta frame moves
    /// 1–3 legs of one event (top ask and top bid: add the new level, remove the old one), all
    /// at one server ts. The ts advances 1–3 ms per frame; recv = ts + 150 (under stale_lag_ms).
    pub fn script(&mut self, n: usize) -> Vec<SynthFrame> {
        let mut out: Vec<SynthFrame> = Vec::with_capacity(self.legs.len() + n);
        for (g, ls) in self.legs.iter().enumerate() {
            let events = ls
                .iter()
                .map(|l| MarketEvent::Snapshot {
                    token: l.token.clone(),
                    server_ts_ms: self.ts,
                    bids: ladder(std::iter::once(l.bid).chain((0..15).map(|k| l.b0 - 2 - k))),
                    asks: ladder(std::iter::once(l.ask).chain((0..15).map(|k| l.a0 + 2 + k))),
                    hash: None,
                })
                .collect();
            out.push(SynthFrame {
                conn: g % 40,
                recv_ms: self.ts + 150,
                events,
            });
        }
        for _ in 0..n {
            out.push(self.next_frame());
        }
        out
    }

    fn next_frame(&mut self) -> SynthFrame {
        self.ts += 1 + i64::try_from(self.rng.below(3)).unwrap();
        let ts = self.ts;
        let g = self.rng.below(self.legs.len());
        let moves = 1 + self.rng.below(3);
        let mut events = Vec::new();
        for _ in 0..moves {
            let l = self.rng.below(self.legs[g].len());
            let da = i64::try_from(self.rng.below(4)).unwrap();
            let db = i64::try_from(self.rng.below(4)).unwrap();
            let size = Shares::new(Decimal::from(1 + self.rng.below(200))).unwrap();
            let gone = Shares::new(Decimal::ZERO).unwrap();
            let leg = &mut self.legs[g][l];
            let (na, nb) = (leg.a0 - 2 + da, leg.b0 - 1 + db);
            for (side, old, new) in [(Side::Ask, leg.ask, na), (Side::Bid, leg.bid, nb)] {
                events.push(level(&leg.token, ts, side, new, size));
                if old != new {
                    events.push(level(&leg.token, ts, side, old, gone));
                }
            }
            leg.ask = na;
            leg.bid = nb;
        }
        SynthFrame {
            conn: g % 40,
            recv_ms: ts + 150,
            events,
        }
    }
}

/// A fresh engine (common `ctx()`) on the synthetic universe, with the gate on or off.
pub fn synth_engine(s: &Synth, gate: bool) -> Engine {
    let mut e = Engine::new(ctx());
    e.set_threshold_gate(gate);
    let _ = e.apply_universe(Arc::new(s.universe.clone()), &s.conn_of, 0);
    e
}

/// Feed `frames` to `e` the way replay does (`on_frame`, then `alive`). Every `drop_every`
/// frames (0 = never) conn 0's feed drops (`disconnect`), and `close_all` runs after the last
/// frame. Every output goes to `sink`, in order.
pub fn drive(e: &mut Engine, frames: &[SynthFrame], drop_every: usize, sink: &mut dyn FnMut(Out)) {
    let mut last = 0;
    for (i, f) in frames.iter().enumerate() {
        for o in e.on_frame(f.conn, f.recv_ms, &f.events) {
            sink(o);
        }
        e.alive(f.conn, f.recv_ms);
        if drop_every > 0 && i % drop_every == drop_every - 1 {
            for o in e.disconnect(0, f.recv_ms) {
                sink(o);
            }
        }
        last = f.recv_ms;
    }
    for o in e.close_all(last) {
        sink(o);
    }
}
```

- [ ] **Step 2: Write the failing tests.** Append to `bins/scanner/tests/engine.rs`:
```rust
#[test]
fn the_threshold_gate_skips_detectors_until_the_top_sums_cross() {
    let mut e = engine();
    seed(&mut e); // ΣA = 1.05 and no bids: both gates shut
    assert_eq!(e.detector_calls(), 0);
    // ΣA = 0.95: the YES gate opens (one call). There are still no bids, so D2b is skipped.
    let outs = e.on_frame(
        0,
        2_150,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
    assert_eq!(episodes(&outs).len(), 1);
    assert_eq!(e.detector_calls(), 1);
    // ΣA = 1.00: the gate is shut, but the open YES episode still runs D2a, which closes it at
    // this frame's own ts.
    let outs = e.on_frame(
        0,
        3_600,
        &[
            ask("e1-y1", 3_500, "0.40", "10"),
            ask("e1-y1", 3_500, "0.35", "0"),
        ],
    );
    let closed = episodes(&outs);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].closed_server_ms, Some(3_500));
    assert_eq!(e.detector_calls(), 2);
    // ΣA = 1.01 with nothing open: fully gated. Only the tracker sees this state.
    let outs = e.on_frame(
        0,
        4_600,
        &[
            ask("e1-y2", 4_500, "0.31", "10"),
            ask("e1-y2", 4_500, "0.30", "0"),
        ],
    );
    assert!(episodes(&outs).is_empty());
    assert_eq!(e.detector_calls(), 2);
}

#[test]
fn the_threshold_gate_never_changes_a_single_output() {
    let mut s = Synth::new(200, 42);
    let frames = s.script(20_000);
    let run = |gate: bool| {
        let mut e = synth_engine(&s, gate);
        let mut outs: Vec<Out> = Vec::new();
        drive(&mut e, &frames, 4_000, &mut |o| outs.push(o));
        (outs, e.detector_calls())
    };
    let (on, calls_on) = run(true);
    let (off, calls_off) = run(false);
    // Debug prints Decimal with its scale, so equal strings mean equal bytes, not only equal values.
    let on_txt: Vec<String> = on.iter().map(|o| format!("{o:?}")).collect();
    let off_txt: Vec<String> = off.iter().map(|o| format!("{o:?}")).collect();
    assert_eq!(on_txt.len(), off_txt.len(), "output counts differ");
    for (i, (a, b)) in on_txt.iter().zip(&off_txt).enumerate() {
        assert_eq!(a, b, "output {i} differs with the gate on");
    }
    // The fixture must exercise what it claims to: both basket kinds open, feed drops
    // right-censor, and the gate really skips work.
    let rows = episodes(&on);
    assert!(rows.iter().any(|r| r.kind == "yes_basket"), "no YES basket opened");
    assert!(rows.iter().any(|r| r.kind == "no_basket"), "no NO basket opened");
    assert!(rows.iter().any(|r| r.censor_right), "no right-censored close");
    assert!(
        calls_on < calls_off / 2,
        "gate skipped too little: {calls_on} calls vs {calls_off}"
    );
}
```

- [ ] **Step 3: Run the tests and check that they fail.**
Run: `cargo test -p scanner --test engine threshold_gate`
Expected: compile error `no method named `detector_calls` found for struct `Engine`` (and `set_threshold_gate` in `common/mod.rs`).

- [ ] **Step 4: Implement the gate in `bins/scanner/src/engine.rs`.**

4a. Replace the import
```rust
use arb_core::detect::{
    Group, GroupLeg, Kind, Realization, Signal, detect_no_basket, detect_yes_basket,
};
```
with
```rust
use arb_core::detect::{
    Group, GroupLeg, Kind, Realization, Signal, detect_no_basket, detect_yes_basket,
    no_basket_may_signal, yes_basket_may_signal,
};
```

4b. In `pub struct Engine`, after the `issue_throttle` field, add:
```rust
    /// Spec §5.4/§8.2: skip a detector's depth walk when its top-of-book gate is false and no
    /// episode is open for its key (S3a-2). Always on in live and replay. Tests and the bench turn
    /// it off to prove the outputs are identical.
    threshold_gate: bool,
    /// Detector calls made so far (D2a + D2b). A counter of inputs processed, not a clock: pure.
    detector_calls: u64,
```

4c. In `Engine::new`, after `issue_throttle: HashMap::new(),` add:
```rust
            threshold_gate: true,
            detector_calls: 0,
```

4d. After `pub fn open_episodes(&self) -> usize { … }` add:
```rust
    /// Turn the §8.2 threshold gate off or on. Tests and the bench only: live and replay never call
    /// this. With the gate off, every evaluation runs both detectors, as before S3a-2.
    pub fn set_threshold_gate(&mut self, on: bool) {
        self.threshold_gate = on;
    }

    /// How many detector calls (D2a + D2b) this engine has made.
    pub fn detector_calls(&self) -> u64 {
        self.detector_calls
    }
```

4e. In `evaluate`, replace the top of the loop:
```rust
        for det in [Det::Yes, Det::No] {
            let sig = match det {
                Det::Yes => detect_yes_basket(&self.groups[g]),
                Det::No => detect_no_basket(&self.groups[g]),
            };
            let key = Self::key(&self.universe.groups[g], det);
```
with
```rust
        for det in [Det::Yes, Det::No] {
            let key = Self::key(&self.universe.groups[g], det);
            let group = &self.groups[g];
            // Spec §5.4/§8.2: walk the book only when the top-of-book sums cross the detector's
            // threshold. A false gate proves the detector returns None (arb-core `*_may_signal`),
            // so skipping it changes nothing. The tracker below still gets `None`, which records
            // first sight exactly as before. A key with an open episode always runs its detector,
            // so a close never depends on the gate.
            let run = !self.threshold_gate
                || self.open_by_key.contains_key(&key)
                || match det {
                    Det::Yes => yes_basket_may_signal(group),
                    Det::No => no_basket_may_signal(group),
                };
            let sig = if run {
                self.detector_calls += 1;
                match det {
                    Det::Yes => detect_yes_basket(group),
                    Det::No => detect_no_basket(group),
                }
            } else {
                None
            };
```
Everything after (`let pnl = …`, `tracker.on_state`, the Open/Peak/Close match, the `n_updates` bump) stays byte-for-byte as it is. The borrows are disjoint fields (`groups`, `open_by_key`, `detector_calls`), and `group`'s last use comes before the `&mut self` calls below, so it compiles as written.

- [ ] **Step 5: Run the new tests and check that they pass.**
Run: `cargo test -p scanner --test engine threshold_gate`
Expected: PASS (2 tests). If `the_threshold_gate_never_changes_a_single_output` fails on an output index, that's a real divergence: stop and report it. Never "fix" it by weakening the comparison.

- [ ] **Step 6: Run everything that pins the old behaviour, unchanged.**
Run: `cargo test -p scanner && cargo test -p arb-core`
Expected: PASS. That includes `replay_reproduces_episodes_and_censoring_byte_for_byte` and every engine/engine_checks test, none of them edited.

- [ ] **Step 7: Run lint.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean. (If fmt reorders the import in 4a, run `cargo fmt --all` and restage only the touched files.)

- [ ] **Step 8: Commit.**
```bash
git add bins/scanner/src/engine.rs bins/scanner/tests/common/mod.rs bins/scanner/tests/engine.rs
git commit -m "scanner/engine: run detectors only when the top-of-book threshold is crossed (spec 5.4/8.2), outputs byte-identical"
```

---

### Task 3: profiling bench, and the two CPU rulings written where they live

**Files:**
- Create: `bins/scanner/tests/bench_engine.rs`
- Modify: `bins/scanner/src/capture.rs` (the two `zstd::Encoder::new(…, 3)` calls in `Writer::write`, plus one const)
- Modify: `config/scanner.toml` (the comment on `channel_capacity` only; the value stays 50000)

**Interfaces:**
- Consumes: `Synth`, `SynthFrame`, `synth_engine`, `drive` (Task 2, `tests/common`); `Engine::detector_calls` (Task 2).
- Produces: nothing other tasks use. The bench prints one line per gate setting, and the controller pastes both lines into the S3a-2 notes.

**What the bench measures.** It measures engine time only: the frames are built before the clock starts, and outputs are pushed into a `Vec` and hashed after it stops. The gate-off line is the pre-S3a-2 cost (both detectors on every evaluation). The gate-on line is the new cost. Both come from the same binary and the same script, so no second build is needed to get a before/after. The test also asserts that both runs' output digests are equal. For scale: at ~4.3k frames/s one core has ~232 µs per frame, and in live that budget also pays for `parse_frame`, which runs on the engine loop (`run.rs`).

- [ ] **Step 1: Create `bins/scanner/tests/bench_engine.rs`.**
```rust
//! S3a-2 profiling (ignored by default). Engine µs/frame with the §8.2 threshold gate off (the
//! pre-S3a-2 behaviour: both detectors on every evaluation) and on, over one synthetic
//! high-rate script. Engine time only: frames are pre-built, and outputs are hashed after the clock stops.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;
use scanner::engine::Out;
use sha2::{Digest, Sha256};
use std::time::Instant;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "profiling: run by hand (S3a-2 plan, Task 3)"]
fn engine_us_per_frame_with_and_without_the_threshold_gate() {
    let groups = env_usize("BENCH_GROUPS", 2_000);
    let n = env_usize("BENCH_FRAMES", 100_000);
    let mut s = Synth::new(groups, 7);
    let frames = s.script(n);
    let mut digests: Vec<Vec<u8>> = Vec::new();
    for gate in [false, true] {
        let mut e = synth_engine(&s, gate);
        let mut outs: Vec<Out> = Vec::with_capacity(frames.len() * 2);
        let t = Instant::now();
        drive(&mut e, &frames, 0, &mut |o| outs.push(o));
        let elapsed = t.elapsed();
        let ns = elapsed.as_nanos() / u128::try_from(frames.len()).unwrap();
        let episode_rows = outs.iter().filter(|o| matches!(o, Out::Episode(_))).count();
        let mut h = Sha256::new();
        for o in &outs {
            h.update(format!("{o:?}").as_bytes());
        }
        digests.push(h.finalize().to_vec());
        println!(
            "gate={gate:<5} groups={groups} frames={} engine={}.{:03} µs/frame detector_calls={} episode_rows={episode_rows}",
            frames.len(),
            ns / 1_000,
            ns % 1_000,
            e.detector_calls(),
        );
    }
    println!("VPS budget at ~4.3k frames/s: ~232 µs/frame on one core, shared with parse_frame");
    assert_eq!(digests[0], digests[1], "the gate changed the outputs");
}
```

- [ ] **Step 2: Check that the test is ignored by default and compiles clean.**
Run: `cargo test -p scanner --test bench_engine`
Expected: `test engine_us_per_frame_with_and_without_the_threshold_gate ... ignored`, `0 passed; 0 failed; 1 ignored`.

- [ ] **Step 3: Record the zstd ruling in code (no behaviour change).** In `bins/scanner/src/capture.rs`, add above `pub struct Writer`:
```rust
/// zstd level for hour files and universe records. Ruling S3a-2 (2026-09-25): it stays 3. Phase 0
/// measured ~123 GB/day raw → ~33 GB/day at level 3 (spikes/probe/RESULTS.md Q7c). That is
/// ~1.4 MB/s of input on the capture thread, around 1 % of one core at level 3 (hundreds of MB/s).
/// Level 1 would save a fraction of that 1 % for bigger files, and it would drop the measured basis
/// behind the retention/disk budget. The CPU pressure is in the engine loop, not here.
const ZSTD_LEVEL: i32 = 3;
```
Then in `Writer::write` replace `zstd::Encoder::new(Vec::new(), 3)?` with `zstd::Encoder::new(Vec::new(), ZSTD_LEVEL)?`, and `zstd::Encoder::new(f, 3)?` with `zstd::Encoder::new(f, ZSTD_LEVEL)?`.

- [ ] **Step 4: Record the channel-capacity ruling in config (value unchanged).** In `config/scanner.toml`, change the line `channel_capacity = 50000` to:
```toml
channel_capacity = 50000        # S3a-2: leave it. At ~4.3k frames/s this is ~12 s of queue. A bigger buffer only turns drops into lag (soak 2: p99 20–36 s), a smaller one turns lag into drops; the fix is engine throughput
```
(The config hash is the sha256 of this file's text, so runs after this change record a new `config_hash`. That's expected: the binary changes in this plan too.)

- [ ] **Step 5: Run the checks and a debug bench.**
Run: `cargo test -p scanner && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS and clean. The capture and replay tests prove level 3 still round-trips.
Then run the debug bench, which needs no extra disk (`target/debug` is reused):
Run: `BENCH_GROUPS=500 BENCH_FRAMES=20000 cargo test -p scanner --test bench_engine -- --ignored --nocapture`
Expected: two `gate=…` lines, and the test passes (the digests are equal). In the `gate=true` line, `detector_calls` is well under half of `gate=false`'s, and µs/frame is lower. Debug `Decimal` is many times slower than release, so read only the **ratio** from this run.

- [ ] **Step 6: The one allowed release build (absolute µs/frame).** This is the owner-approved exception to "debug only". It's local, never on the VPS. First check free space, then run it once, record its size, and delete it:
Run: `df -h . && cargo test --release -p scanner --test bench_engine -- --ignored --nocapture && du -sh target/release && cargo clean --release`
Expected: the same two lines at realistic speed, and the test passes. `target/release` is transient: roughly 1.5–2.5 GB for the scanner's dependency tree. Added to ~3.1 GB of `target/debug`, that stays far below the 30 GB budget, and `cargo clean --release` removes it straight after. Paste both `gate=` lines and the `du` figure into the task report for the controller. Don't commit bench output.

- [ ] **Step 7: Commit.**
```bash
git add bins/scanner/tests/bench_engine.rs bins/scanner/src/capture.rs config/scanner.toml
git commit -m "scanner: ignored engine bench (gate off vs on); record zstd level and channel capacity rulings"
```

---

### Task 4: engine: no O(pending) scans per frame (added from the Task 3 bench)

**Why (bench evidence, Task 3, release build).** The gate cut detector calls to 6.6%, but engine time went only from 372 to 365 µs/frame (2,000 groups, 100k frames), above the VPS's ~232 µs/frame budget. So the detectors were not the main per-frame cost. What remains is two full scans of `pending_tops` (one deferred top-check per token with an unresolved update, i.e. tens of thousands of entries across a full universe):
- **every frame** in `on_frame`: `self.pending_tops.iter().filter(|(_, p)| p.conn == conn && p.ts + 1_000 < ts)` to find stale entries, which is O(pending) per frame;
- **`pending_in_group(g, ts)`**: `pending_tops.iter().any(|(t, p)| p.ts <= ts && token_ix[t].0 == g)`, which is O(pending) per call, called from `close_episode` and from `prune_held` for every held episode (O(held × pending)).

**Files:**
- Modify: `bins/scanner/src/engine.rs` (the `Engine` struct, `Engine::new`, every site that inserts or removes from `pending_tops`, the stale sweep in `on_frame`, `pending_in_group`, `disconnect`, `close_all`, `apply_universe` where group indices are remapped)
- Test: `bins/scanner/tests/engine.rs` (append), `bins/scanner/tests/bench_engine.rs` (re-run only)

**Interfaces:** no public API change. `pending_tops` stays the source of truth (BTreeMap keyed by token). Two private secondary indexes are added and kept consistent with it at EVERY mutation.

**Requirements.**
1. **Stale-sweep index:** `pending_by_conn: HashMap<usize, BTreeSet<(i64, String)>>` (conn → set of (ts, token)), or equivalent. The per-frame sweep collects the tokens whose `ts + 1_000 < frame ts` for this conn with a range query: O(log n + due), not O(pending).
   - **Output order must be IDENTICAL to today:** today's sweep resolves due tokens in `pending_tops`' key order (token-sorted BTreeMap). So collect the due tokens from the index, **sort them by token**, then resolve. Replay stays byte-identical, and every existing test passes unchanged.
2. **Group index:** `pending_by_group: Vec<BTreeMap<i64, u32>>` (group → multiset of pending ts), or equivalent, so `pending_in_group(g, ts)` is `min_ts(g) <= ts`, i.e. O(log n). Canary tokens (not in `token_ix`) are in no group.
3. **Consistency at every mutation site:** insert/replace in the delta path (a same-token replace must remove the old (ts, token) entry first), remove on snapshot, `resolve_pending`, `rest_snapshot` (both the group-leg and canary paths), `disconnect` (`retain(|_, p| p.conn != conn)` must also drop that conn's index entries), `close_all` (clear all), and `apply_universe`. Group indices change on a refresh, so rebuild `pending_by_group` from `pending_tops` there, once per refresh, which is fine. Tokens removed from the universe must disappear from all three structures.
   - Add a `#[cfg(test)]`-only (or `debug_assertions`-only, if cheap enough to leave on) private `fn check_pending_indexes(&self)` that asserts both indexes equal what a full scan of `pending_tops` would give.
   - Call it from the new tests after each operation. Do NOT call it on every frame in normal debug runs, so the existing equivalence test doesn't become O(n²).
4. **Tests (append to tests/engine.rs):**
   - A randomized sequence (a deterministic LCG, like Task 2's script) of deltas across several conns and tokens, snapshots, REST snapshots, disconnects, universe refreshes and close_all. After each step call a test hook that runs `check_pending_indexes`; expose it as `#[doc(hidden)] pub fn debug_check_pending_indexes(&self)` behind `#[cfg(any(test, feature = "..."))]` if integration tests need it. The simplest option is a `pub fn` whose doc says it's for tests, and that's acceptable.
   - Every existing test in tests/{engine,engine_checks,replay}.rs passes UNCHANGED, which proves output order and content are unchanged.
5. **Bench:** re-run the debug bench (`BENCH_GROUPS=500 BENCH_FRAMES=20000 cargo test -p scanner --test bench_engine -- --ignored --nocapture`) and put the numbers in the report.
   - **Only if the debug ratio shows a clear improvement,** run the owner-allowed release bench once: `df -h . && cargo test --release -p scanner --test bench_engine -- --ignored --nocapture && du -sh target/release && cargo clean --release`. Report µs/frame with the gate on, before (365) and after.
   - If the per-frame time is still well above 232 µs, profile with a simple split of timers inside the bench (not in engine.rs) to name the next hotspot in the report. Candidates: `parse_frame` is NOT in the engine bench, `Self::key` format!, the tracker's `seen` insert, `rates()` map. Do not optimize further in this task.

- [ ] Steps: write the index-consistency test first (RED: the hook doesn't exist), implement the indexes and the hook (GREEN), run the full `cargo test -p scanner` (all existing tests unchanged), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, then the benches. Commit: `git commit -m "scanner/engine: index pending top-checks by conn and group (no O(pending) scans per frame)"` (no trailer).

---

## Rulings

1. **The gate is the detectors' own gross threshold.** YES: every live leg has an ask and ΣA < 1. NO: ≥ 2 live legs with a bid and ΣB > 1. Spec §5.4 says "fee-adjusted". But D2a/D2b deliberately return gross-positive signals even when fees make them net-negative (`Signal.gross`, test `m3_…`), so a fee-adjusted gate would be exact only for the engine's net filter, not for the detector contract. A future consumer of gross signals (paper P3-naive, gross-only episodes) would silently lose states. The gross gate is exact in both directions (Task 1 proof and proptest), and the CPU difference is nil: nearly every state fails both thresholds.
2. **O(legs) recompute, no O(1) running sums.** This deviates from §8.2's "O(1) update" on purpose. Five book-mutation paths plus leg-status changes on refresh would each have to keep a cache consistent. The O(legs) top read is negligible next to the ladder clones and `Decimal` walks it gates.
3. **An open episode always runs its detector.** The proof makes this redundant, but it costs one hash lookup and keeps every close independent of the gate. The tracker is called on every state either way.
4. **Byte-identical means Debug-identical.** Output equality is checked on `format!("{:?}")`, because `Decimal`'s `==` ignores scale.
5. **zstd stays at level 3, with no `capture.zstd_level` key.** It's already inside 1–3. Phase 0's 33 GB/day figure was measured at 3, and the capture thread's zstd cost is ~1 % of a core against an engine loop that was saturated. Trade-off recorded: level 1 would buy a fraction of that 1 % of CPU for more disk per day. Not worth it at `max_capture_gb = 10`.
6. **`engine.channel_capacity` stays 50000.** A bounded queue in front of a slow consumer only chooses between drops and lag. At 4.3k frames/s it already holds ~12 s, which matches the 20–36 s p99 spikes. Only engine throughput removes drops.
7. **One release build, local, deleted afterwards** (Task 3 Step 6). Debug numbers give only the ratio.
8. **Next suspect if soak 3 still drops:** `parse_frame` runs on the engine loop (`run.rs`), and the per-evaluation `Self::key` `format!` and `seen` insert allocate. Measure these before moving parsing into the shard tasks. That's out of scope here.

## Self-review notes (for the reviewer)

- **Spec coverage:** §5.4 "only run when … threshold" is Tasks 1 and 2. §8.2 "threshold crossed → detectors" is Task 2 (the "O(1) update" deviation is Ruling 2). §9.3 purity and determinism: the gate and counter are pure, the gate-off path is the old code, and Task 2's equivalence test plus the unchanged replay test cover it. §15 boundaries: Task 1.
- **Controller scope items:** (1) gate, proof, proptest, engine skip with the tracker still called, byte-identical test, crossed books: Tasks 1–2. (2) Profiling with the exact commands and the disk statement: Task 3 Steps 5–6. (3) zstd level checked (it's 3, `capture.rs` `Writer::write`), so no key, with the ruling in code: Task 3 Step 3 and Ruling 5. (4) channel_capacity: left alone, with the ruling in config: Task 3 Step 4 and Ruling 6.
- **Types used across tasks:** `yes_basket_may_signal(&Group) -> bool` and `no_basket_may_signal(&Group) -> bool` (Task 1) are imported by `engine.rs` (Task 2). `Engine::set_threshold_gate(bool)` and `Engine::detector_calls() -> u64` (Task 2) are used by `tests/common::synth_engine` (Task 2) and `bench_engine.rs` (Task 3). `Synth::new(usize, u64)`, `Synth::script(usize) -> Vec<SynthFrame>` and `drive(&mut Engine, &[SynthFrame], usize, &mut dyn FnMut(Out))` are used identically in `tests/engine.rs` and `bench_engine.rs`.
- **Name clash check:** `tests/engine.rs` has its own private `delta(…)` helper. The new common helpers `level`, `ladder` and `cents` are private to `common`, so `use common::*` doesn't import them.
