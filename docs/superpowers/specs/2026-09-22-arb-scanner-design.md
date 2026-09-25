# cute-trading v2 — Polymarket arbitrage lab (read-only, paper trading)

**Status:** design v2, awaiting review. No code written.
**Date:** 2026-09-22. Every "current API" fact was re-checked against live docs or live endpoints on this date. Sources are at the end.
**Hard constraint:** paper trading only. No order placement, no wallet, no signing keys or signing code, no exchange credentials. Public read-only data only.

---

## 0. TL;DR

A read-only lab that:

1. **detects** NegRisk and combinatorial arbitrage live, depth-aware and net of 2026 fees;
2. **paper-trades** it with a realistic execution model (latency, taker delay, partial fills, leg risk, conversion, settlement);
3. **reproduces** the statistics of the three papers side by side with ours;
4. uses **Laya** (a local, open, calibrated classifier; Python, offline, on your machine) *only* to rank combinatorial-pair candidates and to make completeness decisions more conservative, never to create or approve an opportunity;
5. shows all of it in a **cute pixel frontend** with deadpan copy.

What the research changed:

- **Single-condition arbitrage can't happen on a consistent book.** The YES and NO books are mirrors (Lemma L1, verified live on 117 legs at every level). That check becomes an integrity canary.
- **Your correction to the paper is right:** `(n−1) − ΣNO` (Lemma L5), verified numerically.
- **Only the NO side is executable before settlement**, via the NegRisk Adapter (Gebele et al.). The YES side must be held to resolution.
- **Fees are real now:** `shares × rate × (p(1−p))^e` per market.
- **The biggest trap is incomplete sets.** Augmented events show "+5% to +10%" YES baskets today that are just the price of the unlisted outcomes (§5.9).
- **Websocket event-time capture is required**, because REST polling misses the 3.6 s median episode.
- **No Polymarket SDK:** the official Rust crate compiles in signing code, and the ecosystem has a confirmed typosquat.

---

## 1. Scope and decomposition

This is several subsystems. Each gets its own implementation plan, built in dependency order:

```
                       ┌──────────────────────────┐
                       │ S1  arb-core (pure math) │  lemmas, fees, books, fill walk, detectors,
                       │     + WASM build         │  episodes. Everything else depends on it.
                       └────────────┬─────────────┘
              ┌─────────────────────┼──────────────────────┬─────────────────────┐
              ▼                     ▼                      ▼                     ▼
 ┌────────────────────┐ ┌──────────────────────┐ ┌───────────────────┐ ┌──────────────────────┐
 │ S2 scanner         │ │ S3 paper trader      │ │ S4 replication    │ │ S5 oracle (Laya)     │
 │ live + replay,     │ │ policies, execution  │ │ paper cards:      │ │ pair screening,      │
 │ store (Postgres),  │ │ model, ledger,       │ │ Cheng, Gebele,    │ │ conservative checks, │
 │ raw capture        │ │ settlement           │ │ Saguillo (lab)    │ │ calibration          │
 └─────────┬──────────┘ └──────────┬───────────┘ └─────────┬─────────┘ └──────────┬───────────┘
           └───────────────────────┴───────────┬───────────┴──────────────────────┘
                                               ▼
                               ┌──────────────────────────────┐
                               │ S6 api + S7 pixel frontend   │  radar, episodes, paper trading,
                               │                              │  paper cards, math playground, review
                               └──────────────────────────────┘
   deploy (§17) is small and rides along with S2/S6.
```

Build order: **S1 → S2 → S3 → S6/S7 (radar + episodes + paper) → S4 → S5 → S7 (review, math playground)**. **Phase A runs everything on your machine, dockerized (compose); nothing is online.** Deploy (phase B) reuses the same images and moves only postgres/scanner/api/analyze/web to the VPS. Laya always stays local. **Rust for all performance-sensitive backend; Python only for Laya (batch, offline).**

---

## 2. Repo review and planned changes

**Today:** a Python/React dashboard (~2,100 lines): FastAPI + httpx, SQLAlchemy/Postgres, Redis, React 19/Vite, docker compose, Traefik labels for `isoca.space/polymarket`. Features: a Gamma market list, CLOB price history, and a "smart money" wallet tracker. **No Rust, no order placement or signing code.**

**Keep:**
- the pixel design tokens (`frontend/src/index.css`: `--mint #ddfff7`, `--teal #93e1d8`, `--salmon #ffa69e`, `--rose #aa4465`, `--deep #861657`, Press Start 2P + VT323, `.pixel-box` / `.pixel-btn` with the 4 px hard shadow);
- the Polymarket API notes in `CLAUDE.md` (JSON-encoded `clobTokenIds` / `outcomes` / `outcomePrices`; token ids, not `conditionId`);
- the Traefik label pattern;
- the nginx CSP headers.

**Drop:** the backend, the old Postgres schema, Redis, the trader tracker, `PROJECT.md` phases 3–4 (bots and execution conflict with paper-only), and the unused trading-related config fields.

**Planned changes:**
1. Tag `main` as `legacy-python-v0`.
2. Build on branch `rewrite/arb-lab`.
3. Carry the tokens into `web/src/pixel.css`.
4. Rewrite `CLAUDE.md` for the new stack.
5. Replace the deploy workflow (§17).

---

## 3. Current Polymarket API (verified 2026-09-22)

| Topic | Fact | Consequence |
|---|---|---|
| Platform | CLOB V2 since 2026-04-28; collateral **pUSD** (6 dp); fees set at match time | Read endpoints unchanged; paper data predates V2 |
| Auth | Public market data needs no credentials | Nothing to authenticate |
| Gamma | `/events/keyset` (cursor, ≤100/page). Events: `negRisk`, `enableNegRisk`, `negRiskAugmented`, `negRiskMarketID`. Markets: `negRiskOther`, `feeSchedule{rate,exponent,takerOnly,rebateRate}`, `feeType`, `orderPriceMinTickSize`, `orderMinSize`, `secondsDelay`, `acceptingOrders`, `enableOrderBook`, `umaResolutionStatuses`, `endDate`, `outcomePrices` | Discovery source; `feeType` doubles as category |
| CLOB REST | `GET /book`, `POST /books` (≤500 tokens); bids ascending / asks descending (**best = last**); `hash`, `tick_size`, `min_order_size`, `neg_risk`, ms `timestamp`; `GET /ok` | Re-checks, resyncs, health |
| Websocket | `wss://ws-subscriptions-clob.polymarket.com/ws/market`, public; `book` snapshot, `price_change` (new aggregate size, 0 = remove, carries best bid/ask), `last_trade_price`, `tick_size_change`, plus `best_bid_ask`, `new_market`, `market_resolved` with custom features; `PING` every 10 s; **no subscription cap** | Primary feed |
| Rate limits | Cloudflare per IP: `/books` 500/10 s, `/book` 1,500/10 s, Gamma `/events` 500/10 s; over-limit requests are **queued, not rejected** | Measure latency; drop late checks |
| Fees | `fee = C · rate · (p(1−p))^exponent`, taker only, 5 dp, min 0.00001. Live: 0.05 (149,823 markets), 0.04 (16,881), 0.07 (5,859), 0.03 (3,535), 0 (18,909) | Net-of-fee everything |
| Delays | Crypto taker delay 50 ms (changelog); `secondsDelay` = "delay, in seconds, before a newly placed marketable order can match" (market-details docs): 1 on 143k markets, 3 on 603 | First executable moment ≥ delay |
| Restarts | HTTP 425 during restarts, then **2 min post-only** | Flag episodes in that window |
| NegRisk | Adapter V2 `convertPositions`: NO on S → `(|S|−1)` collateral + YES on the rest (minus `feeBips`); no reverse path | Asymmetric executability |
| Augmented | Placeholders ("Person O…Z", `bid 0 / ask 1`) and an explicit **Other** | Named-leg YES baskets are not complete |
| Rust clients | Official `polymarket_client_sdk_v2` 0.8.0 always compiles `alloy` + `hmac`; typosquat `polymarkets-rs-clob-client` (RUSTSEC-2026-0016) | Write our own types, ban signing crates |

**Live census:** 18,957 active events; 196,039 live markets (≈392k tokens); **7,030 NegRisk events** (2,511 augmented, 53,678 live legs; legs per event p50 = 3, p90 = 17, max = 100). Most multi-market events are *not* NegRisk. Round trip from São Paulo: ≈0.29 s.

---

## 4. The papers: findings and what we reproduce

| | Saguillo et al. 2025 (2508.03474) | Gebele et al. 2026 (2608.00666) | Cheng et al. 2026 (2605.00864) |
|---|---|---|---|
| Data | On-chain `OrderFilled`, Apr 2024–Apr 2025, block-level VWAP | L2 books, Apr–May 2026, hour-stratified; on-chain conversions | 75M `GET /book` snapshots, 173 NBA games, Feb–Mar 2026 |
| Key result | ~$40M realized; $10.6M single-condition; $29M NegRisk rebalancing; 13 dependent pairs (LLM-found) | 2,098 YES-side vs **36** NO-side positive episodes; median 16.15 s / 7.99 s; $1.086M converter vs $32k settlement | 7 single-market episodes (median **3.6 s**); 290 ML⊇spread episodes (median 16 s, 101 bps); 76.9% depth-capped |
| Method we adopt | Definitions (§3.2.1–3.2.2), $100 cap | Episode definition, censoring, depth walk across legs, taker fees, subset identity | Hash dedup, mirror dedup, one-shot peak, $10 bottleneck, post-resolution exclusion |
| Method we reject | Omitting <2% legs as "safe"; the `n − ΣNO` left-hand side; zero fees | — | Polling-bounded durations; zero fees (sports is 0.05 now) |
| Reproduced in S4 | S-cards via HF dataset (lab) | G-cards from our capture | C-cards from our capture (NBA season starts Oct 2026) |

---

## 5. The math

### 5.1 Model and notation

A NegRisk event has a finite outcome set `Q`, `n = |Q|`, and **exactly one** outcome `w ∈ Q` resolves YES (the NegRisk guarantee). For `i ∈ Q`:

- YES token payoff `Yᵢ = 𝟙{w = i}`; NO token payoff `Nᵢ = 1 − Yᵢ` (collateral identity `Yᵢ + Nᵢ = 1`).
- The YES ask ladder is a non-decreasing step function `Aᵢ(q)`: the price of the q-th share bought, walking asks from best. The YES bid ladder `Bᵢ(q)` is non-increasing. Best prices are `Aᵢ = Aᵢ(0⁺)` and `Bᵢ = Bᵢ(0⁺)`.
- Per-share taker fee at price p: `φᵢ(p) = rᵢ · (p(1−p))^{eᵢ}`, with `rᵢ = feeSchedule.rate` and `eᵢ = feeSchedule.exponent`.
- `L ⊆ Q` = **live named legs**: active, not closed, accepting orders, order book enabled, not a placeholder, not `negRiskOther`.
- Every edge is **strictly** positive to count. An edge of exactly 0 is not an opportunity (it occurs live: Pro Football ΣB = 1.000).

### 5.2 Lemmas

**L1 (mirror).** The NO book is the reflection of the YES book: `NO_asks = {(1−p, s) : (p, s) ∈ YES_bids}` and `NO_bids = {(1−p, s) : (p, s) ∈ YES_asks}`.
*Why:* Polymarket runs one unified book per condition, and complementary orders are matched by CTF split/merge (Gebele §2.2, Fig. 1; Cheng §2.4). *Evidence:* live `POST /books` today, 117/117 legs, every level; Gebele App. A: 308,416,666 messages with 20 isolated glitches; independently measured in polybots (Down_bid = 1 − Up_ask, 2026-07-15), where the feed subscribes to one side only.
*Use:* subscribe to YES only and derive NO. A mismatch is an integrity event.

**L2 (no single-condition arbitrage).** If a book is uncrossed (`B < A`), then `A + (1−B) = 1 + (A−B) > 1` and `B + (1−A) = 1 − (A−B) < 1`. So footnote 6's long and short conditions both require `B > A`, a crossed book. ∎
*Use:* detector D1 is an integrity canary.

**L3 (YES basket).** For `C ⊆ Q`, `Σ_{i∈C} Yᵢ = 𝟙{w ∈ C}`. It equals 1 in every state iff `C = Q`, after removing legs already resolved NO. ∎
*Use:* D2a counts as arbitrage only on complete sets. Otherwise it's `…_incomplete`.

**L4 (NO basket, any subset).** For any `S ⊆ Q`, `Σ_{k∈S} N_k = |S| − 𝟙{w ∈ S} ≥ |S| − 1`. ∎
*Use:* D2b needs **no** completeness, and augmentation only adds upside (it pays |S| when the winner is outside S).

**L5 (corrected short profit).** Buying one NO on each `k ∈ S` costs `Σ_S (1 − B_k)` (by L1). By L4 the guaranteed profit is `(|S|−1) − Σ_S(1−B_k) = Σ_S B_k − 1`. For `S = Q` this is `(n−1) − ΣN`. Saguillo §3.2.1 prints `n − Σval(Nᵢ) = Σval(Yᵢ) − 1`: the right-hand side (the profit they state) is correct, and the left-hand side is off by one (`n − ΣN = ΣY`). The equation is internally inconsistent; whether it affected their dollar figures is unknown. ∎

**L6 (converter family collapses).** Gebele's convert-and-sell edge `(|S|−1) + Σ_{j∉S} Bⱼ − Σ_{k∈S}(1−B_k) = Σ_{i∈Q} Bᵢ − 1`, independent of S. With depth, the identity holds level by level because the ladders mirror exactly and `φ(p) = φ(1−p)`. ∎
*Use:* no 2ⁿ search is needed for detection.

**L7 (mutual exclusion).** On uncrossed books, `Σ_Q A ≥ Σ_Q B ≥ Σ_S B`. So `Σ_Q A < 1` (D2a) and `Σ_S B > 1` (D2b) cannot both hold. ∎

**L8 (fee-inclusive monotonicity).** Buy-side cost `c(p) = p + φ(p)` and sell-side proceeds `v(b) = b − φ(b)` are strictly increasing on [0,1] when:
- `e = 1, r < 1`: `c′ = 1 + r(1−2p) ≥ 1 − r > 0`;
- `e = 2, r < 5.196`: `c′ = 1 + 2r·p(1−p)(1−2p) ≥ 1 − 2r·(√3/18) > 0`, since `max |p(1−p)(1−2p)| = √3/18 ≈ 0.0962` (at `p = (3−√3)/6`).

The same bounds give `v′ > 0`. All live schedules (`r ≤ 0.25`) satisfy these conditions; also checked exactly on a 0.001 price grid. ∎
*Use:* walking levels best-to-worst yields **non-increasing** marginal edge (L9). Rounding fees to 5 dp breaks smoothness only at the 1e-5 scale; we round against ourselves.

**L9 (optimal size).** For a basket with payoff `P` per unit (`P = 1` for D2a, `|S|−1` for D2b), the marginal edge `m(q) = P − Σ_legs c(price_leg(q))` is a non-increasing step function (by L8 and ladder monotonicity). So `Π(q) = ∫₀^q m` is concave and piecewise linear, and it's maximized at `Q* = sup{q : m(q) > 0}`. A greedy walk that stops at the first non-positive segment is optimal. ∎

**L10 (implication).** If `A ⇒ B`, then `N_A + Y_B ≥ 1` in every feasible state, and the profit is `B_A − A_B − φ` (D3). ∎

**L11 (the nested-S walk is optimal).** Buy K NO baskets where each basket may use a *different* subset: basket k holds one NO on every leg that still has bid depth at its k-th share. Each basket over a subset `S_k` is risk-free by L4 and nets `Σ_{i∈S_k} v(Bᵢ(k)) − 1`. For any collection of baskets, each leg contributes at most one share per basket, and its best shares are its first ones. So the maximum for K baskets is `Π(K) = Σᵢ Σ_{k ≤ min(K, dᵢ)} v(Bᵢ(k)) − K`, with marginal `Π(K) − Π(K−1) = Σ_{i : dᵢ ≥ K} v(Bᵢ(K)) − 1`. That marginal is non-increasing: each term is non-increasing (L8), and terms drop out as legs exhaust. So the greedy walk that lets legs **exit as they exhaust** is exactly optimal. ∎
*Checked:* it equals the exhaustive optimum over all basket collections in 400/400 random cases (exact fractions). The reviewer's counterexample (bids 0.60×100, 0.45×100, 0.10×50) gives 10.0, versus 7.5 for the best single fixed S. It also equals Gebele's convert-and-sell edge (their eq. 3 with the YES sale) at depth: selling `YES_j` at its bid and buying `NO_j` at its ask consume the same mirrored ladder, with the same fee (φ symmetric). So D2b and Gebele's NO-side edge are the same quantity, and our D2b never needs the post-conversion YES sale or its settlement-delay leg risk.

### 5.3 Detectors as formulas

| | Kind | Condition (marginal, walked by L9) | Payoff/unit | Realization |
|---|---|---|---|---|
| D1 | integrity | `B > A` on any single condition | — | resync, never profit |
| D2a | `negrisk_yes_basket` | `m(q) = 1 − Σ_{i∈Q} c(Aᵢ(q)) > 0`, **only if** Q complete (L3) | 1 | settlement |
| D2a′ | `negrisk_yes_basket_incomplete` | same over tradeable legs; uncovered legs logged | 0 if uncovered wins | **not arbitrage** |
| D2b | `negrisk_no_basket` | `m(q) = Σ_{i : dᵢ ≥ q} v(Bᵢ(q)) − 1 > 0`, legs exit as they exhaust (L5, L6, L11) | ≥ \|S_k\|−1 per basket | converter (now) |
| D3 | `combo_implication` | `m(q) = v(B_A(q)) − c(A_B(q)) > 0` for curated `A ⇒ B` | ≥ 1 | settlement |

Each is also computed **gross** (φ ≡ 0), to compare with fee-free papers and measure how much the fees remove.

### 5.4 Depth walk (L9 made concrete)

```
walk_basket(legs, payoff P, fee schedules, limits):
  cursors ← each leg at its best level; q ← 0; Π ← 0; cost ← 0; fees ← 0
  loop:
    seg ← min over legs of remaining size at the current level     (merged breakpoints)
    m   ← P − Σ c_leg(price_leg)            (D2b/D3 use v for bid legs)
    if m ≤ 0: break
    if a leg is exhausted: D2a/D3 → break (the set must stay complete);  D2b → drop that leg and continue (L11)
    take ← seg, clipped by budget (cost + take·Σprice ≤ $100 → partial, for the capped figure only)
    fees_seg ← Σ fee_conservative(price, take)      (per level fill, §5.4 note)
    Π += (P − Σprice)·take − fees_seg; cost += Σprice·take; fees += fees_seg; q += take
    (m uses unrounded φ only to decide whether to continue; Π books the rounded fees)
    advance every leg with zero remaining at its level
  return {Q* = q, Π, Π_gross, cost, fees, per-leg VWAP, levels used, bottleneck leg}
```

Complexity is O(total levels touched), with k ≤ 100 legs. It's only run when the running top-of-book sums cross a fee-adjusted threshold (§8.2). Implemented (S3a-2) as the detectors' own GROSS first-segment test (ΣA < 1, ΣB > 1), recomputed per touched group in O(legs). That is exact (gate false ⇒ no signal), whereas a fee-adjusted or O(1) cached version could hide gross-only signals or drift; see plan 2026-09-25-s3a2 rulings 1–2.

### 5.5 Statistics (exact definitions used by the analyzer and replication)

- **State sequence** per group g: `(t_k, Π_k)` evaluated **once per websocket frame** touching g, after all of the frame's `price_changes` items are applied (a frame can move several legs at once; per-item evaluation would create intermediate states that never existed). Time is server ms.
- **Episode:** a maximal run `k₀…k₁` with `Π_k > 0`. `t_open = t_{k₀}`, `t_close = t_{k₁+1}`, `D = t_close − t_open` (Gebele §4.3).
- **Censoring:** `left` (positive at scanner start or after a reconnect), `right` (scanner stop or shard drop), `gap` (feed gap > 30 s inside). Censored episodes keep their flags and never get an invented duration. How they enter the statistics: `right` → standard KM right-censoring; `left` → **excluded** from duration estimates (length-biased: long episodes are more likely to straddle a start) and reported as a count; `gap` → interval-censored, so reported separately with bounds, not fed to KM.
- **Flicker:** Π can dip to ≤ 0 for a single frame and come back, which splits one dislocation into many episodes. Headline counts use the raw definition (Gebele-comparable), and a **merge-gap sensitivity** reports counts and durations when positive runs separated by ≤ {0, 250 ms, 1 s} are merged.
- **One-shot profit:** `max_k Π_k` (Cheng A.5). Never summed across states.
- **Capped profit:** `Π` at cost ≤ $100 (Cheng, Saguillo).
- **Yield:** `10⁴ · Π / cost` bps. **Profit per dollar:** `Π / cost`.
- **Time in arbitrage:** `Σ D / observed group-time`.
- **Survival:** Gebele's empirical `S(τ) = K⁻¹ Σ 𝟙{D_k > τ}` over uncensored episodes (for comparability), and Kaplan–Meier `Ŝ(τ) = Π_{t_j ≤ τ}(1 − d_j/n_j)` using censored ones (our headline).
- **Survival at Δ:** `P(Π(t_open + Δ) > 0)` from re-checks.
- **Lock-up (settlement kinds):** `days_to_end` is an **estimate**, not a bound: events can resolve before `endDate`, and UMA can resolve after it. We report the simple return `Π/cost` as the headline and annualized `(1 + Π/cost)^{365/max(days, 7)} − 1` only as a secondary figure, with days floored at 7. Without the floor, a 0.5% edge annualizes to 83% at 3 days and 3,700% at 0.5 days.
- **Polling emulation:** resample the event-time states at cadence `c` (e.g. 3.6–5.5 s, Cheng), recompute episodes, and report missed-episode share and duration bias.

### 5.6 Paper-trading accounting (S3)

One execution model, used identically by live paper trading, replay, and tests (`sim::ExecutionModel`).

- **Clock:** everything is anchored on **our receive time** of the opening frame, `r_open` (not the server timestamp), because that's when we could have acted.
- **Execution time:** `t_x = r_open + L_proc + L_net + L_match + max_legs secondsDelay`, where:
  - `L_proc` is our detect→decision time (measured in-process);
  - `L_net` is the one-way transport to the CLOB (measured RTT/2 in `health`; ≈145 ms from São Paulo);
  - `L_match` is the venue's matching-engine time for a marketable order, **excluding** `secondsDelay`, which is added separately (it's documented as a delay before a marketable order can match);
  - **priors are labelled third-party and crypto-specific.** polybots measured ≈300 ms "taker" POST latency idle and 0.9–4.6 s *end-to-end fire latencies* in bursts (2026-07-07, BTC up/down). A 250 ms crypto taker delay was in force until 2026-08-17 (changelog), so a large part of that 300 ms was probably the delay itself (inference). Hence `L_match` defaults to a **sensitivity band**, not a point estimate: {50, 150, 300} ms idle, burst multiplier {×1, ×3, ×10} conditioned on the recent message rate;
  - **scenarios:** colocated-idle, SP-idle, SP-burst. The headline is the **P&L-vs-latency curve**.
- **Per-leg order:** a FAK limit at the price level detected at open, walked against the books at `t_x`. Fill `f_leg = min(q_target, available_leg)`, each leg independently, where `available_leg` is the depth at ≤ limit **minus liquidity this policy already consumed** (below). Venue constraints: min 5 shares; a FAK buy needs `price × size` with ≤ 2 decimals, so sizes are rounded down.
- **Consumed liquidity (no double counting):** our fills aren't real, so the orders we "took" are still on the book in the feed. Each policy keeps a ledger of consumed `(token, side, price level) → size`. A level's consumed amount only clears when the feed shows that level's aggregate size **drop** by at least that amount, or the level disappears; a trade or cancel by someone else then plausibly took it. Otherwise the same resting orders could be "filled" by every flicker of the same episode.
- **Basket size:** `q_eff = min over basket legs of f_leg` (for D2b, per nested basket, L11). Excess leg inventory is **leg risk**: unwound by selling at the bid at `t_x + ε` (same latency model), with fees, and the loss is realized.
- **NO basket:** convert `q_eff` after an on-chain **settlement delay** (bought tokens aren't spendable for ~1–3 s: polybots' "not enough balance" right after a buy). The payoff is locked at fill, so the delay affects capital recycling, not risk. Cash `+= (|S|−1)·q_eff·(1 − feeBips)`. The returned YES on `Q∖S` is **marked at 0** until resolution (a windfall is booked only if it wins), so it never inflates equity.
- **YES basket / D3:** hold until resolution. Locked capital = cost. Settlement uses the market's **payout vector** from `market_resolved` / Gamma `outcomePrices` (not an assumed {0,1}; see Q19).
- **Equity:** `cash + Σ open positions × current bid`, except complement YES from conversions (marked 0, as above). We track realized and unrealized P&L, drawdown, capital utilization, and fees paid.
- **Adverse selection (markouts):** for every fill, record each leg's mid at `t_x` + {1 s, 5 s, 30 s}. polybots' cross-venue run lost money because "the dislocation that fires the signal *is* a market move in progress" (the missing leg ran away in 83% of cases, which is a small sample: 24 signals, 6 fills). Markouts measure whether that happens here.
- **Sizing:** one shot per episode. `q_target = min(Q*(open), budget_per_trade / cost_per_unit, free_cash / cost_per_unit)`.

### 5.7 Schematics

**(a) One book, shown twice.** Live "Fed Decision in October? · No change", 2026-09-22 (same sizes, prices reflected `p ↦ 1 − p`, same timestamp):

```
        YES token book                                  NO token book  (= mirror, L1)
  asks  0.58 ×   5,637.30                         asks  0.49 ×  28,133.09   (1 − 0.51)
        0.57 ×  23,963.36                               0.48 ×  42,239.53   (1 − 0.52)
        0.56 × 252,281.27                               0.47 ×  85,887.16   (1 − 0.53)
        0.55 ×  32,356.03  ◄ best ask  A                0.46 ×   5,603.74   (1 − 0.54)  ◄ best NO ask
  ──────────── spread 0.01 ────────────           ──────────── spread 0.01 ────────────
  bids  0.54 ×   5,603.74  ◄ best bid  B          bids  0.45 ×  32,356.03   (1 − 0.55)  ◄ best NO bid
        0.53 ×  85,887.16                               0.44 × 252,281.27   (1 − 0.56)
        0.52 ×  42,239.53                               0.43 ×  23,963.36   (1 − 0.57)

  D1 long : A + (1 − B) = 1 + spread = 1.01 ≥ 1        D1 short: B + (1 − A) = 1 − spread = 0.99 ≤ 1
  Both fire only if B > A (crossed) → a feed/state bug (L2).
```

**(b) Payoff tables** (rows = holdings, columns = the winner):

```
 D2a  YES basket, {A,B,C} + uncovered "Other"          D2b  NO basket over S = {A,B,C}
 ┌─────────┬────┬────┬────┬───────┐                   ┌─────────┬────┬────┬────┬───────┐
 │ winner→ │ A  │ B  │ C  │ Other │                   │ winner→ │ A  │ B  │ C  │ Other │
 ├─────────┼────┼────┼────┼───────┤                   ├─────────┼────┼────┼────┼───────┤
 │ YES_A   │ 1  │ 0  │ 0  │  0    │                   │ NO_A    │ 0  │ 1  │ 1  │  1    │
 │ YES_B   │ 0  │ 1  │ 0  │  0    │                   │ NO_B    │ 1  │ 0  │ 1  │  1    │
 │ YES_C   │ 0  │ 0  │ 1  │  0    │                   │ NO_C    │ 1  │ 1  │ 0  │  1    │
 ├─────────┼────┼────┼────┼───────┤                   ├─────────┼────┼────┼────┼───────┤
 │ basket  │ 1  │ 1  │ 1  │  0 ✗  │                   │ basket  │ 2  │ 2  │ 2  │  3    │
 └─────────┴────┴────┴────┴───────┘                   └─────────┴────┴────┴────┴───────┘
  L3: risk-free only if nothing uncovered             L4: ≥ |S|−1 in every column, any S
  realize: hold to resolution                         realize now: convertPositions(S)
  profit = 1 − Σ c(A)                                 profit = Σ_S v(B) − 1   (L5; paper's LHS off by 1)

 D3  implication A ⇒ B
 ┌─────────┬───────┬────────┬─────────┐
 │ state→  │ A∧B   │ ¬A∧B   │ ¬A∧¬B   │   (A∧¬B impossible)
 ├─────────┼───────┼────────┼─────────┤
 │ NO_A    │  0    │  1     │  1      │
 │ YES_B   │  1    │  1     │  0      │
 ├─────────┼───────┼────────┼─────────┤
 │ total   │  1    │  2 ★   │  1      │   ≥ 1 always (L10); ★ bonus never counted (Cheng: 0/290)
 └─────────┴───────┴────────┴─────────┘
```

**(c) Classification of one NegRisk update:**

```
 leg top-of-book changed
   │
   ├─ event decided / UMA in progress / past endDate ─► keep, flag post_end
   ├─ D2b  nested S: legs with a YES bid, each exiting as its depth runs out (optimal, L11)
   │        Σ_S v(B) − 1 > 0 ? ─► negrisk_no_basket · converter        (valid for ANY S, L4)
   └─ D2a  every outcome in Q has a YES ask, no placeholder/Other/halted leg?
            ├─ yes: 1 − Σ c(A) > 0 ? ─► negrisk_yes_basket · settlement
            └─ no : same test over tradeable legs ─► negrisk_yes_basket_incomplete (NOT arbitrage)
        (+ Laya DC3 may downgrade "complete" to incomplete. It can never upgrade, §13.)
 D2a and D2b are never both positive (L7).
```

**(d) Depth walk (L9):**

```
 per-basket cost incl. fees
  1.01  ┤                                   ┌───────────────  (unprofitable from here)
  1.00  ┤━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━  payoff P = 1
        │░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
  0.99  ┤░░░░░░░░░░░░░░░░░░┌────────────────┘                  ░ = Π = area between the
        │░░░░░░░░░░░░░░░░░░│                                      cost step and the payoff
  0.97  ┤──────────────────┘
        └──────────────────┬────────────────┬──────────►  q (baskets)
                           q₁               Q*
  steps = union of all legs' level boundaries; cost non-decreasing (L8) ⇒ stop at first loss
```

**(e) Episode, survival checks, and paper execution:**

```
 update:        u1        u2          u3          u4           u5
 net Π:         ≤ 0       +0.40       +1.10       +0.20        ≤ 0
                          ├──────────── episode ───────────────┤  D = t(u5) − t(u2)
                                        ▲ peak: one-shot Π
 re-checks:               │+Δd         │+1 s        │+2 s        │+5 s     (ws; REST at 1 s, 2 s)
 paper trade:             └─ t_x = r_open + L_proc + L_net + L_match + secondsDelay ─► FAK per leg (minus consumed liquidity)
                             q_eff = min fills; leftovers unwound (leg risk); convert or hold
```

### 5.8 Checked against the papers

| Claim | Paper basis | Status |
|---|---|---|
| YES basket pays 1, NO basket pays n−1 | Saguillo §3.2.1 text; Gebele eq. (2) | agree |
| `(n−1) − ΣNO` | Saguillo prints `n − ΣN = ΣY − 1` (LHS off by 1, RHS correct); Gebele uses `(|Q|−1)` | **inconsistency confirmed** (L5) |
| Footnote 6 arbitrage | Gebele App. A; Cheng §2.4 | integrity only (L1, L2) |
| NO→YES pre-settlement only | Gebele §2.3, §3.3, Alg. 1 | agree (Q5 may change) |
| Subset identity | Gebele eq. (1), (3) | agree; collapse L6 is ours, checked |
| Placeholders in the adapter's set | Gebele §6 | agree; drives L3 handling |
| Episode, censoring, survival | Gebele §4.3 | adopted |
| One-shot, $10 bottleneck, mirror dedup, post-resolution exclusion | Cheng A.3–A.5 | adopted; our trust ceiling is 30 s (ours, not Cheng's: his ceilings were 300 s in-game / 1,800 s otherwise, sized for polling) |
| Implication primitive | Cheng §4.1; Saguillo Def. 4 | generalized (L10) |
| Omit <2% legs | Saguillo §6.2, §7.3 | **rejected** as risk-free; labelled risk class |
| Saguillo realized profit: fees ignored, bids < $2 dropped, 950-block window | Saguillo §7.1 | **not reproduced** (it also needs split events, which the HF dataset lacks); S5 is the fill-only approximation |

### 5.9 Numeric verification (throwaway scripts, 2026-09-22)

**Synthetic, exact decimals, 15/15 pass:**
- **fees:** the docs' tables reproduce (0.07@0.50 → 1.75; 0.04@0.30 → 0.84; 0.05@0.95 → 0.24; e=2, r=0.25 @0.50 → 1.5625), and φ(p) = φ(1−p);
- **L5:** holds for n = 2…19;
- **L9 walk:** equals unit brute force on 3,000 random mirrored books (D2a and D2b);
- **L1 derivation:** equals direct NO-ask computation (3,000/3,000);
- **L7:** 0 violations;
- **L6:** holds over all subsets of 2,000 events;
- **L2** and **L10:** hold.

**Live, top-of-book, NegRisk events among today's top 40 by volume:**

| event | live legs | aug. | \|S\| (has bids) | D2a net | D2b gross | D2b net |
|---|---:|:-:|---:|---:|---:|---:|
| Fed Decision in October? | 5 | no | 5 | −0.0327 | −0.016 | −0.0416 |
| Brazil Presidential Election | 19 | yes | 5 | *incomplete* (−0.0535) | **+0.005** | −0.0148 |
| Republican Presidential Nominee 2028 | 42 | yes | 33 | *incomplete* (**+0.0975**) | −0.166 | −0.1878 |
| UEFA Champions League 2027 | 36 | yes | 36 | *incomplete* (−0.1410) | **+0.002** | −0.0243 |
| Democratic Presidential Nominee 2028 | 51 | yes | 39 | *incomplete* (**+0.0540**) | −0.153 | −0.1834 |
| Pro Football 2027 Champion | 32 | yes | 32 | *incomplete* (−0.0885) | **0.000** | −0.0280 |
| Presidential Election Winner 2028 | 53 | yes | 32 | *incomplete* (**+0.0541**) | −0.148 | −0.1785 |
| Next French Presidential Election | 41 | yes | 20 | *incomplete* (−0.0630) | −0.031 | −0.0609 |
| Elon Musk # tweets Sep 18–25 | 19 | no | 10 | −0.0763 | −0.027 | −0.0630 |
| Balance of Power: 2026 Midterms | 5 | no | 5 | −0.0566 | −0.002 | −0.0219 |

What it shows:
- The mirror held at every level on all 117 legs.
- Gross NO-side violations exist right now (+0.005, +0.002), and fees remove them.
- Every positive YES number is on an augmented event: the uncovered-outcome trap (L3).
- The exact-1.000 boundary occurs in the wild.

---

## 6. What the current API makes impossible, or only partly possible

| # | Limitation | Handling |
|---|---|---|
| X1 | Single-condition arbitrage (L2) | Integrity canary |
| X2 | YES-basket exit before settlement | Settlement kind, lock-up metrics; Q5 |
| X3 | Complete YES sets in augmented events | `…_incomplete`, never risk-free |
| X4 | Queue position and competing takers are invisible | P&L is an upper bound; paper trader models latency, delay, partial fills, leg risk |
| X5 | `secondsDelay` (documented matching delay for marketable orders) | Logged per leg; execution waits for it |
| X6 | Fee on buys: USDC vs shares | Assume USDC (docs); open |
| X7 | Adapter `feeBips` needs an on-chain read | Config, default 0 |
| X8 | Engine post-only windows are only inferable | `engine_suspect` flag |
| X9 | No websocket sequence numbers; book `hash` algorithm undocumented | REST drift check + canary |
| X10 | Throttling queues silently | Latency tracked; late checks dropped |
| X11 | Full-universe REST sweep ≈ 16 s | Websocket primary; REST for NegRisk only ≈ 2.2 s as fallback |
| X12 | No L2 history from Polymarket | Own raw capture; pmxt archive unverified; on-chain trades via HF dataset (for Saguillo only) |

---

## 7. Capture approach

| | **A. Websocket event-time + REST re-check** (chosen) | B. REST polling (Cheng) | C. Archive replay only |
|---|---|---|---|
| Catches 3.6 s episodes | yes | partly | depends |
| Durations | exact | upper bound | exact if L2 |
| Live answer | yes | partly | no |

The same `arb-core` runs A, B (emulated), and C.

- **Universe:** refresh every 5 min from Gamma **`/events/keyset` paginated with exactly `after_cursor`**. The natural names `cursor` / `next_cursor` silently return page 1 again (polybots: 104,800 rows read, 100 unique ids). Stop when a page brings no new ids; never use offset paging, which silently caps at 2,100 and lost polybots 21% of the universe. A market absent from the latest *complete* scan becomes `gone` (otherwise closed markets stay "open" forever). Also refresh on `new_market` / `market_resolved`. NegRisk groups with legs classified `live | placeholder | other | closed_no | closed_yes | halted`; curated pair legs; a canary sample of 200 random binary markets (both tokens, redrawn daily).
- **Subscriptions:** YES only for NegRisk legs (~54k tokens; NO derived via L1); both tokens for the canary and pairs. Resubscribe with `initial_dump`.
- **Sharding is event-affine.** polybots' `wsdual` canary found that Polymarket's websocket publisher **queues per connection under bursts** (0.3–1 s stalls, growing with message rate). If one event's legs are split across connections, a stall on one makes its legs stale next to the others, and that manufactures **phantom baskets**: the desync artifacts Gebele and Cheng warn about. So:
  - all legs of an event (and both sides of a pair) go on the **same** connection;
  - **hot/cold tiering under a connection budget** (config `max_ws_connections`, start 16, because a per-IP cap is believed to exist):
    - *hot* events (top by recent message rate / 24 h volume) get small shards (≤ 500 tokens) and optionally a **duplicate connection**, deduplicated by frame hash, keeping the earliest;
    - the quiet long tail is bin-packed into large shards. Stalls grow with message rate, so quiet shards stall less;
    - tiers are recomputed at each universe refresh, and moving an event re-subscribes it with a fresh snapshot;
  - phase 0 measures stall vs shard size and the connection cap, and the budget is set from that.
- **Transport freshness:** every frame logs `lag = recv_ts − server_ts`. An episode whose opening message had `lag` above a threshold (start 250 ms, calibrated in phase 0) gets the flag `stale_transport`, and headline stats exclude it (like polybots' `max-trigger-age`).
- **Integrity:**
  - canary (L2);
  - **per-message top check**: each `price_changes[]` item carries the server's `best_bid` / `best_ask`. After applying it, our reconstructed top must equal them, otherwise it's an integrity event plus resnapshot. This is continuous and free, and it catches drift the REST check would find 60 s later. (Parse the `price_changes` **array**, where one message can touch several assets. The legacy single-asset shape was dead code in polybots for months.)
  - a rolling REST level-by-level drift check every 60 s;
  - drops → resnapshot.
- **Raw capture:** every frame goes to `capture/YYYY-MM-DD/HH.jsonl.zst` `{recv_ts_us, shard, raw}` for replay (volume measured in phase 0; Gebele's ≈510 msg/s is a reference point).

---

## 8. System architecture

### 8.1 Infra schematics

**Phase A (now): fully local, dockerized.** Everything runs in Docker Desktop (WSL2 backend) on your machine. Nothing is online. The same `compose.yml` becomes the phase B deploy with a small override file, so what you measure locally is what runs later.

Rust handles everything performance-sensitive. Laya stays in Python: it's the reference implementation, and it runs as an offline batch job where speed doesn't matter. The browser frontend is TypeScript.

```
        ┌──────────────────────────── Polymarket (public, no auth) ─────────────────────────────┐
        │ gamma-api  GET /events/keyset   ·   clob  POST /books, GET /ok   ·   wss /ws/market   │
        └───────────▲──────────────────────────▲─────────────────────────────▲──────────────────┘
                    │ 5 min                    │ ≤ 40 req/s                  │ event-affine shards
 ┌── your machine · Docker Desktop (WSL2) · compose project "cute-trading" ──┼─────────────────────┐
 │                                                                            │                    │
 │  profile: core (always on)                                                 │                    │
 │  ┌─────────────────┴──────────────────────────┴─────────────────────────────┴───────────────┐   │
 │  │ scanner  [rust]  universe · feed · engine(arb-core) · re-checks · paper trader(sim)      │   │
 │  │                  NOTIFY episode ─────────────────────────┐        raw frames ─► capture  │   │
 │  └───────┬───────────────────────────────────────────────────┼──────────────────────────────┘   │
 │          │ sqlx (role scanner_rw)                            │                                  │
 │  ┌───────▼───────────────────────────────────────────────────▼───────────┐  ┌───────────────┐   │
 │  │ postgres [postgres:17, pinned digest]   volume: pgdata                │  │ volume: data  │   │
 │  │ NUMERIC money · roles · LISTEN/NOTIFY · migrations (sqlx)             │  │ capture/*.zst │   │
 │  │ TCP on the internal network + Unix socket on volume: pgsock           │  │ reports/      │   │
 │  └──▲───────────────▲───────────────────▲─────────────────────▲──────────┘  │ models/laya   │   │
 │     │ api_rw        │ analyze_ro        │ lab_rw              │ oracle      │ exports/ (→   │   │
 │     │ (LISTEN)      │                   │                     │ (socket     │  Windows)     │   │
 │  ┌──┴────────────┐ ┌┴──────────────────┐│                     │  only)      └───────────────┘   │
 │  │ api [rust]    │ │ analyze [rust]    ││  profile: lab       │                                 │
 │  │ axum · JSON · │ │ duckdb: postgres  │┌┴────────────────────┐┌┴───────────────────┐┌─────────┐│
 │  │ SSE (from     │ │ scanner + Parquet ││ lab [rust]          ││ oracle [python,    ││ fetch-  ││
 │  │ NOTIFY) ·     │ │ + capture → cards,││ candidates ·        ││ CUDA on RTX 3060]  ││ model   ││
 │  │ review        │ │ KM, exports       ││ calibrate · papers  ││ network_mode: none ││ one-shot││
 │  └──▲────────────┘ └───────────────────┘│ (HF via duckdb      ││ queue: SKIP LOCKED ││ sha256  ││
 │     │                                   │ httpfs) · replay    ││ via Unix socket    ││         ││
 │  ┌──┴──────────────────────────────┐    └─────────────────────┘└────────────────────┘└─────────┘│
 │  │ web [nginx] SPA + WASM, /api →  │◄── browser  http://127.0.0.1:8080 (only published port)    │
 │  └─────────────────────────────────┘                                                            │
 └─────────────────────────────────────────────────────────────────────────────────────────────────┘
   everyday:  docker compose --profile core up -d
   screening: docker compose --profile lab run --rm lab candidates   →   run --rm oracle   →   run --rm lab ingest
```

- **One database, no sync.** Every service talks to the same Postgres under its own role (§10). Approval is still human-only (Review page → `pairs`).
- **Named volumes** (`pgdata`, `data`, `pgsock`) live inside the WSL2 VM. Database files on a Windows bind mount are slow and fragile. To get data out to Windows: `docker compose run --rm analyze export` writes Parquet/CSV to the bind-mounted `./exports`; nightly `pg_dump` goes there too.
- **Oracle still has no network.** It reaches Postgres only through the Unix socket on the shared `pgsock` volume (Unix sockets work with `network_mode: none`), as role `oracle`, which can touch only the two queue tables.
- **GPU:** `oracle` reserves the RTX 3060 (compose `deploy.resources.reservations.devices: [{driver: nvidia, count: 1, capabilities: [gpu]}]`). The GPU and `network_mode: none` are compatible. Laya needs ~0.8 GB in fp16/bf16 out of 12 GB, and Ampere supports bf16 (Laya's configured `amp_dtype`). Prerequisites: a current NVIDIA Windows driver; `nvidia-smi` must work inside WSL2.
- **Laya isolation is real now.** The `oracle` container runs with `network_mode: none`, so it can't reach the network even if it wanted to. Weights arrive through a separate one-shot `fetch-model` service (network on, runs once, verifies sha256 against the pinned revision, then is never needed again).
- **Local-machine caveats (Q17):**
  - When the machine sleeps or reboots, capture stops. That produces censored episodes plus `health` gap rows, never invented durations.
  - The WSL2 VM clock can drift after host sleep. Durations use Polymarket's server timestamps, so only `detect_lag_ms` is affected. `health` records the offset so lag stats can be filtered.
- **Dev loop:** `cargo test` / `cargo clippy` run natively on Windows for fast feedback. The **run path is always compose**, so every measured number comes from the pinned images. `npm run dev` in `web/` is fine for UI work (it proxies to the api container).

**Phase B (later): deploy.** The same images and the same `compose.yml`, plus `compose.vps.yml` (Traefik labels, no published ports, no `lab` profile):

```
 VPS (small):   profiles core + analysis  ── named volume data
 your machine:  profile lab (lab + oracle + fetch-model) against a local volume
      lab ── SSH tunnel ──► VPS Postgres as role lab_rw  (no custom sync API; the SSH key stays on your machine)
      oracle stays network-less: it uses a small LOCAL Postgres (queue tables only) on your machine;
      lab relays requests/answers between the local queue and the VPS over the tunnel
      capture slices for replay: rsync over the same SSH
```

### 8.1.1 Images and reproducibility

| Image | Built from | Contains | Pinned by |
|---|---|---|---|
| `cute-trading/rust` | multi-stage: `rust:<ver>` builder with **cargo-chef** (cached dependency layer) + BuildKit cache mounts → `gcr.io/distroless/cc` runtime | `scanner`, `api`, `analyze`, `lab` binaries (one image, several entrypoints) | `rust-toolchain.toml`, `Cargo.lock`, base image **digests** |
| `cute-trading/web` | `node:<ver>` build (Vite + `wasm-pack` for `arb-wasm`) → `nginx:alpine` | static SPA + WASM, CSP headers | `package-lock.json`, base digests |
| `postgres:17` (official) | upstream image | Postgres 17; init scripts create roles + grants; `sqlx migrate` owns the schema | image **digest**; `migrations/` in git |
| `cute-trading/oracle` | `python:3.12-slim` + uv (`uv sync --frozen`), **CUDA torch wheel** (pinned; runs on your RTX 3060 via Docker Desktop's WSL2 GPU support). A `cpu` build arg gives a CPU-only fallback image | vendored Laya inference, question schemas | `uv.lock`, base digest; weights by revision + sha256 |

- Builds set `SOURCE_DATE_EPOCH`, so rebuilding the same commit gives the same binaries (best effort).
- Every row the scanner writes carries `scanner_version` = git sha + image digest.
- Every paper card carries its reproducibility hash (code + config + data range).
- `docker compose build` is the single entry point. The rust image's first build is slow; cargo-chef keeps rebuilds to our own crates.

### 8.1.2 Resilience: restarts without crash loops

Restarts are allowed, but a crash loop must never turn into a reconnect storm against Polymarket (IP throttling or a ban), a flood of writes into Postgres, or a disk filled with capture files. There are three layers, from innermost out:

```
 ┌─ 1. inside the process: failures stay local ─────────────────────────────────────────────┐
 │  • external input never panics: parse error → integrity_event + skip the frame            │
 │    (clippy deny unwrap_used / expect_used in pm-api, scanner; parsers fuzzed, §15)        │
 │  • each shard reconnects on its own: exponential backoff + jitter (1 s → 60 s cap);       │
 │    the backoff resets ONLY after a session lived > 30 s (polybots: resetting on connect   │
 │    made a flapping link hammer the server)                                                │
 │  • one global token bucket for ALL connection attempts + resnapshots across shards        │
 │    (e.g. ≤ 2 connects/s, ≤ 20/min), so a mass disconnect can't become a storm             │
 │  • a supervisor task restarts a crashed shard/task, not the process; K task crashes in    │
 │    W minutes → that shard is parked (degraded) and reported                               │
 ├─ 2. across restarts: a crash-loop breaker (the app decides, not only Docker) ─────────────┤
 │  • every start writes a `runs` row (start, version, config hash); every exit writes the   │
 │    reason. On boot the scanner counts starts in the last 10 min:                          │
 │      ≥ 5  →  SAFE MODE: no Polymarket connections, no writes except `runs`; exit 0        │
 │  • `restart: on-failure:5` in compose: Docker retries crashes (non-zero exit) with its    │
 │    own growing delay, gives up after 5, and a safe-mode exit 0 is NOT restarted. The loop │
 │    ends in a stopped, explained state, not a spinning one                                 │
 │  • warm start: the universe snapshot is cached in Postgres; a restart reuses it if it's   │
 │    < 5 min old instead of re-paging Gamma (~190 requests)                                 │
 │  • reset is manual and explicit: `docker compose run --rm scanner breaker reset`          │
 ├─ 3. resources: failing quietly beats failing loudly and often ────────────────────────────┤
 │  • disk guard: capture pauses (the scanner keeps running) below 10 GB free on the data    │
 │    volume; resumes at 15 GB; logged in `health` and censoring                             │
 │  • Docker log rotation (json-file max-size 10m, max-file 5) on every service              │
 │  • Postgres: statement_timeout, connection pool caps per role                             │
 └───────────────────────────────────────────────────────────────────────────────────────────┘
 visibility: the radar's HEALTH strip shows restarts, breaker state, parked shards, disk;
             `runs` + `health` make every gap explainable afterwards (censoring stays honest)
```

Other services use the same rules: `api` and `analyze` get `on-failure:5`; `oracle`, `lab` and `fetch-model` are one-shot (`restart: "no"`); `postgres` and `web` get `unless-stopped`, because they don't talk to Polymarket.

### 8.2 Inside the scanner

```
 ┌───────────┐ Universe(ArcSwap) ┌────────────────┐ raw ─► capture writer (zstd, hourly)
 │ universe  ├──────────────────►│ ws shards × K  │
 │ (5 min)   │── SubCmd ────────►│ PING·reconnect │
 └───────────┘                   └───────┬────────┘
                                         │ WireEvent{token, server_ts, recv_ts, kind}
                                         ▼ bounded mpsc (drop ⇒ counted + resnapshot)
                   ┌─────────────────────────────────────────────────────────┐
                   │ engine (single owner of all books, no locks)            │
                   │  books.apply(ev) → top-of-book changed?                 │
                   │  groups: running ΣA, ΣB, Σφ per event (O(1) update)     │
                   │  threshold crossed → detectors (walk_basket, L9)        │
                   │  EpisodeTracker → Open / Peak / Close events            │
                   └──────┬───────────────────────┬────────────────┬────────┘
                          │ episode events        │ schedule       │ resync
                          ▼                       ▼                ▼
                 ┌────────────────┐   ┌────────────────────┐   feed
                 │ store writer   │◄──┤ re-checker          │
                 │ (Postgres, txn │   │ DelayQueue: +Δd,    │
                 │ per 100 ms,    │
                 │ NOTIFY)        │   │ 1s, 2s, 5s; REST    │
                 └───────▲────────┘   │ token bucket        │
                         │            └─────────┬──────────┘
                 ┌───────┴────────┐             │ book states at t_x
                 │ paper trader   │◄────────────┘
                 │ (sim, per      │  policies decide at Open; fill at t_x;
                 │  policy)       │  settle on market_resolved
                 └────────────────┘
                 health task: /ok 5 s · RTT · msg rates · lag · clock offset
```

### 8.3 Same core, several drivers

```
 live websocket ─┐                        ┌─► scanner (live)
 raw capture ────┼─► WireEvent stream ─►  │   arb-core + sim  ─►  same Postgres schema
 pmxt archive ───┤    (adapter/source)    ├─► scanner replay (lab or VPS)
 polling emul. ──┘                        └─► web playground (arb-core compiled to WASM)
```

---

## 9. Code structure

### 9.1 Repository tree

```
cute-trading/
├── Cargo.toml                    workspace; [workspace.lints] deny f32/f64 in arb-core, sim
├── deny.toml                     bans alloy*, ethers*, k256, secp256k1, web3*, polymarket*, *clob-client*
├── crates/
│   ├── arb-core/                 PURE (no IO, no clock, no tokio). #![forbid(unsafe_code)]
│   │   └── src/
│   │       ├── money.rs          Price, Shares, Usd (rust_decimal newtypes, checked ctor, parse from str)
│   │       ├── fee.rs            FeeSchedule, φ, c(p), v(b), round5 (L8)
│   │       ├── book.rs           Ladder, Book, apply_snapshot/apply_delta, best, MirrorView (L1)
│   │       ├── universe.rs       Event, Leg, LegStatus, Completeness, GroupKey
│   │       ├── fill.rs           walk_basket, BasketQuote, WalkLimits (L9, §5.4)
│   │       ├── detect/
│   │       │   ├── mod.rs        Detector trait, Signal, Kind, Realization, Flags
│   │       │   ├── canary.rs     D1 (L2)
│   │       │   ├── yes_basket.rs D2a (L3)
│   │       │   ├── no_basket.rs  D2b (L4–L6, L11 nested-S walk)
│   │       │   └── implication.rs D3 (L10)
│   │       ├── episode.rs        EpisodeTracker, Episode, Censor (§5.5)
│   │       ├── stats.rs          KM, empirical survival, yields, polling emulation (§5.5)
│   │       └── wire.rs           WireEvent (normalized, source-agnostic)
│   ├── sim/                      PURE paper trader: Policy, ExecutionModel, Ledger, Portfolio (§5.6, §11)
│   ├── pm-api/                   IO, read-only: gamma, clob, ws (serde wire types + clients)
│   ├── store/                    Postgres via sqlx (compile-checked queries, offline `.sqlx/`), typed repositories
│   │   └── migrations/           versioned schema (sqlx migrate); roles/grants in docker/postgres/init/
│   └── arb-wasm/                 wasm-bindgen facade over arb-core for the web playground
├── bins/
│   ├── scanner/                  `run` · `replay <src>` · `census` · `resync-check`
│   ├── api/                      axum: JSON + SSE for the frontend; review writes (auth at Traefik)
│   ├── analyze/                  G/C cards, KM, reports → Parquet (DuckDB `postgres` scanner + capture files)
│   └── lab/                      Rust CLI: `candidates` · `oracle` · `calibrate` · `papers` · `replay`
│           ├── candidates.rs     structural pair generator (ladders, deadlines, ML⊇spread), rule compression
│           ├── oracle_queue.rs   enqueues typed requests (Postgres), ingests answers, validates schema/revision
│           └── calibrate.rs      temperature fit, ECE, reliability, threshold search (Rust, on stored answers)
├── oracle/                       Python 3.12, managed by uv (uv.lock pinned)
│   ├── oracle/vendor/            audited Laya inference code @1c5edc1 (rl_common.py, rl_agent_api.py), unmodified
│   ├── oracle/main.py            batch: claim pending rows (FOR UPDATE SKIP LOCKED, Unix socket) → Laya → answers → exit
│   ├── oracle/questions.py       DC1–DC4 typed-question schemas (§13)
│   ├── tests/                    schema + queue round-trip tests (pytest)
│   ├── fetch_model.py            one-shot download of revision 1c5edc1 + sha256 verify (fetch-model service)
│   └── pyproject.toml            torch, transformers, safetensors, numpy, psycopg (pinned); NOT the `laya` PyPI package
├── web/                          React + Vite + TS; pixel.css; SVG charts (crispEdges); uses arb-wasm
├── config/                       scanner.toml, policies.toml, pairs.seed.toml (hand-curated seed → `pairs` table)
├── compose.yml                   profiles: core · analysis · lab (phase A and B)
├── compose.vps.yml              phase B override: Traefik labels, no published ports, no lab profile
├── docker/                       rust.Dockerfile (cargo-chef), web.Dockerfile, oracle.Dockerfile
├── rust-toolchain.toml
├── deploy/                       phase B only: update.sh, systemd units
└── docs/superpowers/specs/       this file
```

### 9.2 Crate dependency graph

```
            arb-core  ◄──────────── arb-wasm ◄── web
             ▲    ▲
             │    └──────── sim
             │               ▲
 pm-api      │     store     │
   ▲         │       ▲       │
   └──── scanner ────┴───────┘        api ──► store      analyze ──► store (+ duckdb)
                                      lab ──► arb-core, sim, store (+ duckdb httpfs) ──queue tables──► oracle (Python)
 store ──► sqlx (Postgres). oracle: psycopg over Unix socket, torch, local weights, network_mode: none.
 External network: pm-api (Polymarket) and duckdb httpfs (HF dataset, read-only).
```

### 9.3 Key types and contracts (sketch)

```rust
// arb-core::money: invariants enforced at construction; no f64 anywhere
pub struct Price(Decimal);          // 0 ≤ p ≤ 1
pub struct Shares(Decimal);         // ≥ 0
pub struct Usd(Decimal);

// arb-core::fee
pub struct FeeSchedule { pub rate: Decimal, pub exponent: u8, pub taker_only: bool }
impl FeeSchedule {
    pub fn phi(&self, p: Price) -> Decimal;                 // r·(p(1−p))^e
    pub fn buy_cost(&self, p: Price) -> Decimal;            // c(p) = p + φ(p)
    pub fn sell_proceeds(&self, b: Price) -> Decimal;       // v(b) = b − φ(b)
    pub fn fee_venue(&self, p: Price, q: Shares) -> Usd;        // docs' 5 dp rule (for comparison)
    pub fn fee_conservative(&self, p: Price, q: Shares) -> Usd; // ceil to 5 dp: what Π books
}

// arb-core::book
pub enum Side { Bid, Ask }
pub struct Book { bids: Ladder, asks: Ladder, server_ts: Millis }
impl Book {
    pub fn apply_snapshot(&mut self, bids: &[(Price, Shares)], asks: &[(Price, Shares)], ts: Millis);
    pub fn apply_delta(&mut self, side: Side, price: Price, new_size: Shares, ts: Millis) -> TopChange;
    pub fn no_view(&self) -> MirrorView<'_>;                // L1: NO asks = 1 − YES bids
}

// arb-core::detect
pub trait Detector {
    fn kind(&self) -> Kind;
    fn evaluate(&self, group: &Group, books: &Books, fees: FeeMode) -> Option<Signal>;
}
pub struct Signal { pub kind: Kind, pub realization: Realization, pub quote: BasketQuote,
                    pub gross: BasketQuote, pub s: Vec<LegIdx>, pub flags: Flags }

// arb-core::episode
pub enum EpisodeEvent { Open(EpisodeId, Signal), Peak(EpisodeId, Signal), Close(EpisodeId, Millis, Censor) }
impl EpisodeTracker { pub fn on_state(&mut self, g: GroupKey, t: Millis, s: Option<Signal>) -> SmallVec<[EpisodeEvent; 2]>; }

// sim
pub trait Policy { fn name(&self) -> &str;
                   fn decide(&self, open: &Signal, pf: &Portfolio, ctx: &MarketCtx) -> Option<Intent>; }
pub struct ExecutionModel { pub proc: LatencyDist, pub net: LatencyDist, pub matching: LatencyDist,
                            pub burst: BurstModel, pub honor_seconds_delay: bool, pub settle_delay: Millis,
                            pub unwind_leg_risk: bool }
pub struct ConsumedLiquidity { /* (token, side, price) → size; cleared only when the feed shows the level shrink */ }
pub enum LedgerEntry { Fill{..}, Unwind{..}, Convert{..}, Settle{..}, Mark{..} }  // append-only
```

**Contract rules:**
- `arb-core` and `sim` are deterministic functions of their input events (time arrives on the events), so live, replay, and tests agree byte for byte.
- **Egress is enumerated, not implied** (§16): `pm-api` (Polymarket REST, market websocket, Sports websocket), `lab`'s DuckDB `httpfs` (HF dataset, read-only), `fetch-model` (one-shot, HF), and the optional `lab feebips` Polygon client (JSON-RPC restricted to an **allowlist of `eth_call` and `eth_chainId`**, and anything else refused in code). DuckDB extensions are **baked into the image** at build time, never auto-downloaded at runtime.
- `store` owns the schema, and nothing else writes SQL.

---

## 10. Storage

**Postgres 17** (container, `pgdata` volume) is the operational store. **DuckDB** does the analytics: it reads Postgres through its `postgres` scanner, and it reads Parquet (the HF dataset, our exports). Raw websocket capture stays in zstd JSONL files, never in the database. (polybots' raw books ran ~127 msg/s per token and needed continuous aggregates just to stay usable.)

Why Postgres over SQLite here. Not raw speed: for our low write volume an in-process SQLite is at least as fast. What Postgres adds:
- **Concurrent writers** across containers: scanner, lab, api reviews, oracle queue.
- **`NUMERIC`** money columns that map exactly onto `rust_decimal` through sqlx, which is typed and exact end to end.
- **Roles:** `scanner_rw`, `api_rw` (reviews/pairs only), `analyze_ro`, `lab_rw`, and `oracle` (queue tables only).
- **`LISTEN/NOTIFY`:** the scanner notifies `episode`, and the api turns it into SSE for the radar, with no polling.
- **`FOR UPDATE SKIP LOCKED`** queues.
- **Phase B:** the same container runs on the VPS, and `lab` connects over an SSH tunnel, with no custom sync code.

Writes are batched: one transaction per ~100 ms, `COPY` for `health` and `observations`. Money uses `NUMERIC(20,6)` (pUSD has 6 dp), prices use `NUMERIC(8,6)`, and timestamps use `BIGINT` ms/µs plus a generated `timestamptz` column for humans. Partitioning or Timescale are only considered if a table outgrows plain indexes (YAGNI). **Zero is a measurement:** `NULL` means "not measured", `0` means "measured zero". Rust uses `Option<Decimal>` for anything that may be unmeasured, and serialization never skips zeros. (polybots' `omitempty` dropped every losing payout, so win rate was 100% by construction.)

**Fees are versioned in time.** Each leg's `feeSchedule` is stored with the episode, as seen at capture. Replays and backfills use a `fee_rules` table with `[start, end)` validity, never today's schedule. Example: sports went from 0.03 to 0.05 in 2026, and the Saguillo era (2024–25) was fee-free.

| Table | Writer | Contents |
|---|---|---|
| `events`, `legs` | scanner | Universe snapshots (status, fee schedule, tick, min size, `secondsDelay`, endDate) |
| `episodes` | scanner | One row per episode: see the column list below |
| `episode_legs` | scanner | Per leg at peak: token, action (buy_yes / buy_no), best price, VWAP, levels, size at best, fees, in_S |
| `observations` | scanner | Re-checks: offset, source (ws/rest), skipped + reason, edge, Q\*, Π, still_profitable, fill_at_original_size |
| `integrity_events` | scanner | crossed_book, mirror_mismatch, drift, drop, reconnect_gap |
| `runs` | every long-running service | Start/exit rows (version, config hash, exit reason). Feeds the crash-loop breaker and censoring |
| `health` | scanner | Per minute: tokens, shards, msgs, bytes, drops, reconnects, REST p50/p99, lag, clock offset, `/ok` |
| `paper_accounts`, `paper_intents`, `paper_fills`, `paper_ledger`, `paper_positions` | scanner (sim) | Per policy: §11 |
| `oracle_requests`, `oracle_answers` | lab / oracle | Batch queue for DC1–DC4; role `oracle` can read and write only these two tables |
| `pair_candidates` | lab | Proposed pairs with DC1/DC2 probabilities, model revision, calibration version |
| `leg_flags` | lab | DC3 downgrade flags (leg looks like placeholder/Other). Can only make events *less* complete |
| `pairs` | api (on human approval) | Approved implication pairs; scanner hot-reloads |
| `reviews` | api | Human decisions on pair candidates (approve / reject, note, who, when) |
| `paper_cards` | analyze, lab | Replication results (§12) |
| `fee_rules` | lab / migrations | Time-versioned fee schedules `[start, end)` for replays and backfills |

**`episodes` columns:**
- identity and context: `episode_id` (ULID), `kind`, `realization`, `event_id`, `event_slug`, `event_title`, `neg_risk_market_id`, `pair_id`, `category`, `n_outcomes`, `n_legs`, `legs_dropped`, `augmented`, `incomplete_set`, `uncovered_legs`;
- timing: `opened_server_ms`, `closed_server_ms`, `duration_ms`, `censor`, `detect_lag_ms`, `n_updates`, `peak_server_ms`;
- the peak: `peak_edge_top`, `peak_edge_top_gross`, `peak_max_size`, `peak_cost_usd`, `peak_payout_usd`, `peak_fees_usd`, `peak_pnl_net_usd`, `peak_pnl_gross_usd`, `peak_profit_per_dollar`, `pnl_net_budget100_usd`, `bottleneck_leg_idx`, `bottleneck_notional_usd`;
- flags and extras: `below_min_size`, `max_seconds_delay`, `days_to_end`, `annualized_return`, `post_end`, `engine_suspect`, `stale_transport`, `open_lag_ms`, `oracle_downgraded`;
- provenance: `scanner_version`, `config_hash`.

---

## 11. Paper trader (S3)

**Policies**, configured in `policies.toml`; each runs with its own virtual bankroll (default $1,000):

| Policy | Takes | Why it exists |
|---|---|---|
| `P0-converter` | Net-positive `negrisk_no_basket` | The executable strategy. Expectations from Gebele §5.3: converter-enabled strategies were 97% of *their* realized profit, but **full-set NO→collateral (what P0 models) was only $205,531 = 18.9%** of the $1.086M; the rest came from partial conversions and recycling returned YES inventory, and ~80% of conversions were maker-assisted (all-taker ≤ 40%). The median converter profit per trade fell to ≈ **$0.08** by early 2026. A taker-only P0 should expect little |
| `P1-settlement` | Complete `negrisk_yes_basket` with annualized ≥ 10% and days_to_end ≤ 30 | Measures lock-up economics |
| `P2-combo` | `combo_implication` on approved pairs | D3 in practice |
| `P3-naive` | Everything, including `…_incomplete`, ignoring fees (Saguillo-style assumptions) | **Control group**: shows what the naive reading loses when an unlisted outcome wins |

**Execution model:** exactly §5.6: latency from our receive time (processing + network + matching band + `secondsDelay`), per-leg FAK against books at `t_x` minus already-consumed liquidity, leg risk unwound with fees, NO baskets converted after the settlement delay, YES/D3 held to resolution with the payout vector.

**Outputs per policy:** equity curve, realized vs unrealized P&L, fees paid, locked capital, drawdown, hit rate (Π at `t_x` > 0), average slippage vs the detected Π, leg-risk cost, markouts, and the **P&L-vs-latency curve**.

**Pre-registered headline hypothesis (from polybots' audit of the same paper):** *H0: the windows where ΣYES ≠ 1 are exactly the adverse-selection windows.* If H0 holds, detection works but paper P&L at realistic latency is ≤ 0, and that's a result, not a failure. Thresholds, policies and metrics are frozen in `policies.toml` and hashed **before** each evaluation run.

**Out of scope:** maker (resting) policies. If ever added, fill models must be bracketed: a floor with same-leg flow only and a ceiling with mirrored flow. polybots found live maker fills ≈ 0.23× the mirror-inclusive ceiling, and front-of-queue assumptions inflate *n* 6–9× without changing EV/sh.

**Replay:** `scanner replay --policies all` re-runs every policy on any capture. Tuning happens offline and deterministically.

---

## 12. Paper replication (S4): "paper cards"

Each card shows **the paper's number, our number, the method difference, the data window, and a reproducibility hash** (config + code version + data range).

| Card | Reproduces | Data | Runs |
|---|---|---|---|
| C1 | Cheng single-market episodes (7, median 3.6 s) → expected ≈ integrity events only (L1/L2) | Our capture with the **NBA tier** (below), 2026–27 season | local (VPS in phase B) |
| C2 | Cheng ML⊇spread: episodes/game, median duration, 101 bps, 76.9% depth-capped, 14.8 shares; gross and **net** (sports fee 0.05). Pairs come from the ML⊇spread generator under a **rule-class approval** (below), not pair-by-pair review | Our capture + curated/generated ML⊇spread pairs | local (VPS in phase B) |
| C3 | Cheng's polling bias: resample our NBA-tier capture at 3.6–5.5 s, then report missed share and duration inflation | Our capture | local (VPS in phase B) |
| G1 | Gebele side asymmetry: positive YES-side vs NO-side episodes | Our capture | local (VPS in phase B) |
| G2 | Gebele durations: empirical S(τ) (their form) plus KM; medians vs 16.15 s (YES side, n=253) / 7.99 s (NO side, **n=5**, which Gebele says can't support a distributional comparison) | Our capture | local (VPS in phase B) |
| G3 | Gebele hour-stratified sampling emulation: window-spanning share vs 58.7% | Our capture | local (VPS in phase B) |
| G4 | Gebele Fig. 2: net-edge distribution by side | Our capture | local (VPS in phase B) |
| S1 | Saguillo NegRisk rebalancing opportunities with **their exact filters** (§6: per-block VWAP; carry the last price forward ≤ 5,000 blocks, **else price 0**; skip moments with any token > $0.95; keep ≥ $0.05 per dollar; 2¢ plot threshold; no fees; <2% omission) | HF `Polymarket_data` (Apr 2024–Apr 2025) | lab |
| S2 | Saguillo single-condition opportunities (their method) vs mirror-consistent reality, including **how many involve a zeroed or stale leg** (the "else price 0" rule fabricates cheap baskets; a candidate explanation for their "all long, median sum ≈ 0.60") | HF dataset | lab |
| S3 | Saguillo profit per dollar by topic, $100-capped totals | HF dataset | lab |
| S4 | Saguillo §5 LLM dependency detection vs **Laya** (DC1) on their 13 pairs + our labelled set | Paper App. F + ours | lab |
| S5 | Saguillo §7 forensics, post-paper, **fill-only**: who completes YES/NO or full-NO baskets *through CLOB fills* in 2025–26, their P&L and concentration. Labelled as a lower bound on competition: the HF dataset has fills only, so it can't see NegRisk Adapter conversions or CTF splits/merges (where Gebele found most converter profit) | HF dataset (to Jul 2026) | lab |

**What the C-cards need (added after review):**
- **NBA subscription tier:** during the season, every market of every NBA game event (moneyline, spreads, totals, props; Cheng: ~3,042 markets over 173 games), both tokens, from `gameStartTime − 2 h` to game end + 30 min. It's a separate hot-tier group in the connection budget.
- **Game phase:** `gameStartTime` (Gamma, documented) marks the start. The game **end** comes from Polymarket's public Sports websocket (`wss://sports-api.polymarket.com/ws`, no auth; polybots validated its `live` / `ended` fields; that endpoint is added to the pm-api route table, §16). Cheng excludes **post-game** states (after the physical end, before resolution), not post-resolution; we use the same definition for C-cards, and `post_end` elsewhere.
- **Rule-class approval:** structural generator rules with a proof (e.g. `Spread_A(h ≥ 1) ⇒ ML_A` inside one game event, same resolution source) can be approved **once as a rule** on the Review page, after spot-checking a sample. Pairs produced by an approved rule then go to D3 without per-pair review. Laya plays no part in rule-class approval.

**Honesty rule:** a card never "fixes" a paper's method silently. It shows *their* method on *our* data, *our* method on our data, and the gap between them.

**Premises rule (from polybots):** "a verdict is only valid under the premises it was drawn from." Each card stores its premises (code version range, data range, fee rules, config hash). When a bug fix or new data source touches a premise, the affected cards flip to **`under_review`** automatically and are re-run. They're never silently inherited.

---

## 13. Laya decision checks (S5)

**What Laya is** (from its card and an audit on 2026-09-22):
- a 421M-parameter ModernBERT-large **classifier**, not a generator; Apache-2.0;
- returns calibrated probabilities for typed questions (`noul` yes/no, `choice`, `score`) in one forward pass (~33–40 ms on a T4 GPU, 193–464 ms on CPU);
- English context is 512 tokens; the multilingual checkpoint has 1,024 (up to 8k);
- its own card says: base checkpoints are **near chance zero-shot** on its typed-decisions benchmark, it **ships over-confident** (ECE 0.466 → 0.081 only after refitting temperatures on your data), and ordinal `score` is its weakest question type;
- its English XNLI (entailment) score is 0.86, which is the closest public evidence for our main use: "does A imply B?"

**Supply-chain posture:**
- The Hugging Face repo code was audited (no network, subprocess, eval, or pickle; weights in safetensors).
- It's a 4-day-old project with anomalous popularity signals (0 downloads vs 2,541 likes on the Hugging Face model; 16.5k GitHub stars in 4 days).
- So:
  - **pin revision `1c5edc17a7acd8701df6fc341c0d179f1c62c982`** and verify the sha256 of `model.safetensors` and `tokenizer.json` at load;
  - **vendor the audited inference files unmodified** into `oracle/vendor/` and run them with pinned torch/transformers (uv.lock);
  - **don't install the `laya` PyPI package** (separate, unaudited code);
  - run in a container with **`network_mode: none`**, plus `HF_HUB_OFFLINE=1` and `TRANSFORMERS_OFFLINE=1`, loading weights from the volume (placed by the one-shot `fetch-model` service after the sha256 check).
- **Why Python here (and nowhere else):** Laya is the reference implementation, so there's no parity risk. It runs as an offline batch job over at most thousands of pairs, where Rust's speed doesn't matter. The boundary is a narrow, typed queue (two Postgres tables, reached over a Unix socket) that `lab` validates on both sides. A Rust port (candle has ModernBERT) remains a later option if the Python dependency ever hurts.

**The rule that makes it safe to use:**

> Laya may **rank** candidates for a human, or make an automatic decision **more conservative**. It may never create, approve, or upgrade an opportunity. All risk-free math stays deterministic (§5).

| Check | Question to Laya | Input state | Effect |
|---|---|---|---|
| DC1 pair screening | `choice`: A implies B / B implies A / equivalent / mutually exclusive / unrelated; `noul`: "must B resolve YES whenever A does?" | Both questions plus compressed rules (source, deadline, thresholds extracted deterministically) | Sorts the **review queue**. Only human-approved pairs reach the `pairs` table / D3 |
| DC2 rule compatibility | `noul`: "same resolution source and deadline?" | Both rule texts (compressed) | Low probability ⇒ candidate auto-rejected (conservative) |
| DC3 leg role sanity | `choice`: named outcome / placeholder / catch-all "Other" | Leg title + description | Disagreeing with structural flags ⇒ event **downgraded** to incomplete + alert. It can never upgrade |
| DC4 category | `choice` over the fee categories | Event title + tags | Only when `feeType` is missing; analysis label only |

**Candidate generator** (feeds DC1/DC2): deterministic structural rules propose pairs:
- price ladders ("hit ≥ X": higher ⇒ lower, same asset and window);
- deadline ladders ("by d₁" ⇒ "by d₂ > d₁");
- sports moneyline ⊇ spread (Cheng);
- same-entity across events (title similarity);
- **duplicate listings** (same question re-listed under another slug, e.g. `…-august-20-august-22` vs `…-2026`): equivalence pairs, the cheapest real arbitrage polybots found;
- **nested crypto up/down windows** (15-minute vs 5-minute digitals on the same close; the strike ordering decides the implication): polybots' `nestedarbpaper` E1 estimated ~$15.60/day across 6 coins. Only after v1: it needs a strike proxy at window boundaries and the Chainlink resolution source.

Laya scores them, and humans review them in the frontend.

**Calibration** (lab):
- Build a labelled set: Saguillo's 13 pairs; auto-labelled structural ladders (true implications); random cross-event pairs (negatives); every human review from the frontend (a set that grows over time).
- Fit a temperature per (question type, option count), as the card recommends.
- Report accuracy, ECE, and a reliability diagram.
- Choose thresholds for **precision ≥ 0.95** on "implies". Recall is secondary, because humans review anyway.
- Compare the English and multilingual checkpoints by measured precision, not by vibes. **Fine-tuning** on the 3060 (the card's notebook targets 2× T4, and 12 GB fits a 421M model with small batches, bf16 and gradient checkpointing) becomes a real option once the labelled set is big enough (target: a few hundred reviewed pairs). It's evaluated strictly on **held-out** reviews against the zero-shot baseline, and a fine-tuned checkpoint is used only if it beats that baseline's precision at the same threshold. Fine-tuned weights are versioned like the pinned base (hash + training-set hash).

---

## 14. Frontend (S7): cute, pixelated, deadpan

**Stack:** React + Vite + TypeScript (your stack), `pixel.css` from the legacy tokens, and hand-made SVG charts with `shape-rendering: crispEdges` so charts look like pixels too. Data comes from the api (JSON + SSE). The math playground runs the **same `arb-core` code** via WASM, so the explainer can't disagree with the scanner.

**Pages:**

```
 ┌─ RADAR ──────────────────────────────────────────────────────────────┐
 │ ♥♥♥♥♥♥♥♥♥♥♡  shards up 10/11    msgs/s 512    lag 180 ms   /ok ✓     │
 │ ┌ open episodes ────────────────────────────────────────────────────┐ │
 │ │ NO-basket  Brazil Pres.  S=5  +$0.41 net  Q*=83  00:04.2  ▓▓▓░ │ │
 │ └───────────────────────────────────────────────────────────────────┘ │
 │ today: 3 gross, 0 net.  "Fees: 3. Us: 0."                             │
 └──────────────────────────────────────────────────────────────────────┘
 ┌─ EPISODES ───────────────┐  filters: kind · category · net/gross · flags
 │ table → detail: legs, S, VWAPs, re-check timeline (+Δd, 1s, 2s, 5s), │
 │ mini mirrored book at peak (schematic a, live data)                   │
 └──────────────────────────┘
 ┌─ PAPER TRADING ──────────────────────────────────────────────────────┐
 │ equity curves per policy (P0 P1 P2 P3), locked capital bar,          │
 │ positions awaiting resolution, ledger. P3-naive usually loses:        │
 │ "The naive strategy is doing great. Until 'Other' wins."              │
 └──────────────────────────────────────────────────────────────────────┘
 ┌─ PAPERS ─────────────────────────────────────────────────────────────┐
 │ one card per C/G/S metric: [paper] vs [us] · method diff · data hash │
 └──────────────────────────────────────────────────────────────────────┘
 ┌─ MATH ───────────────────────────────────────────────────────────────┐
 │ interactive schematics a–e: drag book levels, watch Q* and Π move;   │
 │ toggle fees; "exactly 1.00" button. Powered by arb-core (WASM).      │
 └──────────────────────────────────────────────────────────────────────┘
 ┌─ REVIEW (auth) ──────────────────────────────────────────────────────┐
 │ pair candidates sorted by Laya P(implies), rules side by side,       │
 │ [approve] [reject] [unsure]. Reliability diagram of Laya vs reviews. │
 └──────────────────────────────────────────────────────────────────────┘
 ┌─ HEALTH ─┐ capture stats, integrity events per million msgs (Gebele: 20 per 308M)
```

**Copy voice:** deadpan, as in the examples above. Empty radar: *"0 opportunities. The market is efficient, or we are slow. Probably both."*

---

## 15. Testing strategy

All in pure crates, against synthetic books.

- **Money/parse:** exact decimal parsing; > 6 dp rejected; lints forbid f32/f64 in `arb-core` and `sim`.
- **Zero survives:** a measured 0 (payout, fill, P&L) round-trips through Postgres (`NUMERIC`, `NULL` ≠ 0) and JSON; `None` stays distinct.
- **Store:** sqlx compile-time query checks in CI (offline mode); migrations apply cleanly from empty; a role test proves `oracle` can't read `episodes` and `analyze_ro` can't write; integration tests run against a throwaway Postgres container.
- **Keyset paging:** a fake Gamma that ignores the wrong cursor name must stop the loop (no new ids), not spin.
- **Top check:** a synthetic `price_changes` message whose `best_bid` disagrees with our book raises an integrity event.
- **Resilience:**
  - breaker: 5 starts within 10 min → safe mode, exit 0, no connections attempted (fake clock);
  - backoff resets only after a 30 s healthy session (a flapping fake server can't pull the backoff down);
  - the global connect budget holds under a simulated mass disconnect of every shard;
  - warm start reuses a fresh universe snapshot (zero Gamma calls);
  - parsers are fuzzed (`cargo fuzz`) with captured frames as seeds, and a malformed frame yields an integrity event, never a panic.
- **Architecture boundaries:** a test over `cargo metadata` asserts the crate graph in §9.2 (e.g. `arb-core` and `sim` have no IO dependencies), like polybots' import-linter.
- **Fees (golden, from the docs):** 0.07@0.50 → 1.75; 0.04@0.30 → 0.84; 0.05@0.95 → 0.2375 exact (the docs table shows 0.24 in cents); e=2, r=0.25 @0.50 → 1.5625; p ∈ {0,1} → 0. Two rounding functions are tested separately: `fee_venue` (5 dp; below 0.00001 → 0, per docs; mode otherwise unverified) and `fee_conservative` (always round **up** to 5 dp, never 0 for a positive fee). Π always books `fee_conservative`.
- **One test per lemma:**
  - L1 mirror view equals direct computation;
  - L2 canary iff crossed;
  - L3 incomplete never classified as arbitrage (includes the live-style augmented trap);
  - L4 subset payoff ≥ |S|−1 over all winners;
  - L5 pinned: `(n−1)−ΣNO = ΣYES−1` and `n−ΣNO = ΣYES` (the paper's LHS);
  - L6 subset collapse (proptest);
  - L7 exclusion (proptest);
  - L8 monotonicity over the price grid for all live schedules;
  - L9 walk equals brute force (proptest);
  - L10 implication payoff;
  - L11 nested-S walk equals the exhaustive optimum on small random books (brute force over basket collections).
- **Boundaries:** ΣA = 1.000000 → none; 0.999999 → yes; net exactly 0 → none; ΣB = 1.000000 → none; B = A → no canary.
- **Episodes/stats:** open/peak/close, censoring, KM against a hand-computed table, polling emulation on a synthetic timeline.
- **Sim:** partial fills create leg risk and unwind correctly; conversion cash; settlement; the equity identity `cash + marks = initial + realized + unrealized`; deterministic replay.
- **Replay golden:** a recorded fixture produces byte-identical `episodes` and `paper_ledger`.
- **Oracle boundary:**
  - Rust `oracle_queue` against a fake answers table: malformed rows, wrong model revision, schema drift;
  - pytest for the question schemas and queue round trip;
  - a compose smoke test: `oracle` has no network (a connect attempt must fail);
  - a DC3 test proving flags can only downgrade;
  - calibration against a toy set;
  - candidate generator rules (ladder/deadline/ML⊇spread) on synthetic markets.
- **Supply chain:** `cargo deny` in CI; a test asserting the `pm-api` route table.

---

## 16. How the paper-only constraint is enforced mechanically

1. No exchange credential fields exist in config. Processes refuse to start if an **exchange or wallet** credential is present: a denylist of env name patterns (`*PRIVATE_KEY*`, `*MNEMONIC*`, `*SEED_PHRASE*`, `POLY_*`, `CLOB_*`, `*API_SECRET*`, `*PASSPHRASE*`). Database role passwords (`POSTGRES_*`, `PG*`) are expected and allowed.
1b. Every outbound destination is listed (§9.3 egress note); anything else is a bug. The Polygon client can only send `eth_call` / `eth_chainId`, so it can't submit a transaction even with a key present.
2. `cargo-deny` bans signing and exchange-client crates.
3. `pm-api` exposes only `GET /events/keyset`, `GET /book`, `POST /books`, `GET /ok`, the market websocket, and the Sports websocket. A test asserts that route table.
4. No code is imported from other repos with trading capability (e.g. polybots has order signing and live executors). Lessons only, re-implemented here.
5. `sim` produces ledger entries only. There's no code path to an exchange, and none can be added without breaking #2/#3.
6. All services run as non-root with a read-only root filesystem (writable volume only). `oracle` has `network_mode: none`. The only published port is `127.0.0.1:8080` in phase A. The oracle never leaves your machine, even in phase B.

---

## 17. Deploy: phase B, later (small, private repo, zero GitHub secrets)

Not needed while everything is local. Recorded here so phase A doesn't paint us into a corner.

- **GitHub:** CI (fmt, clippy, test, deny) → build images (scanner/api/analyze share one Rust image; web) → push to **private GHCR** with the automatic `GITHUB_TOKEN`. No stored secrets, and GitHub never connects to the VPS.
- **VPS:** a systemd timer pulls every 5 min with a **read-only token stored only on the VPS**, pins the digest, runs `up -d`, health-checks, and rolls back automatically; everything is logged.
- Artifact attestations need a public repo or Enterprise Cloud, so integrity rests on repo-only push rights, digest pinning, and rollback.
- **Alternative:** a repo-scoped read-only deploy key with builds on the VPS.
- Review endpoints sit behind Traefik basicauth (htpasswd on the VPS).
- **Postgres on the VPS:** not published to the internet (internal network only). `lab` reaches it with `ssh -L` using your existing key; role passwords live in a VPS-only `.env`. Nightly `pg_dump` goes to the VPS disk, and a copy is pulled to your machine.

---

## 18. Open questions

Status legend: **open**, **narrowed** (strong evidence, still worth confirming), **decided**. "Who" says who can close it fastest.

| # | Item | Status / evidence | Who | Plan |
|---|---|---|---|---|
| Q1 | `secondsDelay` semantics | **decided (conservative, final for v1)**: treated as the per-market **matching delay**; every execution waits `max secondsDelay` over the legs, and a delay-0 sensitivity scenario is reported alongside. Discord history searched 2026-09-22: nothing conclusive | — | Revisit only if observed data contradicts it |
| Q2 | Buy-side fee in USDC vs shares | **decided: USDC** (docs + polybots' USDC reconciliation from `data-api /trades`). Discord history: nothing contradicting | — | — |
| Q3 | Fee rounding per match vs per order | **decided (conservative)**: round each level fill up to 5 dp. The worst-case error is ≤ 0.00001 × levels per basket, which is negligible against any edge that survives the $10 bottleneck filter | — | — |
| Q4 | Adapter `feeBips` | **narrowed**: `getFeeBips` returned 0 on all 11 sampled markets (read-only `eth_call`, 2026-09-22), but that was the **V1** adapter `0xd91E…35296`, which the changelog says is being retired; the V2 path `0xadA2…6eAab` now handles pUSD actions and exposes no such getter. Whether conversions via V2 charge a fee is unknown | — | Default 0, with a `feeBips` sensitivity (0 / 10 / 50 bps) on P0. Close by reading the V2 contract source (verified on PolygonScan) in phase 3 |
| Q5 | Do `merge_from_yes_basket` / `convert_to_yes_basket` give plain NegRisk YES baskets a pre-settlement exit? | **decided (conservative, final for v1): no.** D2a stays `realization = settlement` (collateral-return plans are built from Combo positions only; Discord history had nothing conclusive). Watch item: card G1 tracks the YES/NO violation ratio over time | — | Revisit only if G1 shows the YES side shrinking |
| Q6 | `min_order_size` units | **decided: 5 shares** (polybots: `orderMinSize = 5` shares in every market checked; the CLOB book field agrees) | — | — |
| Q7 | Websocket limits, message rate, bytes/day | **narrowed**: your friend believes a **per-IP connection cap** exists (not quantified); polybots measured per-connection stalls under bursts. Design: a **connection budget** (config, start 16) with **hot/cold tiering** (§7) | — | Phase 0: find the cap carefully (step up connections slowly, stop at the first refusal), measure stall vs shard size |
| Q8 | pmxt archive | **dropped**: `archive.pmxt.dev` doesn't open for you either. Replay uses our own capture only | — | — |
| Q9 | VPS RTT to Polymarket | later (phase B) | you, one command on the VPS | `curl -w '%{time_total}' https://clob.polymarket.com/ok` |
| Q10 | GHCR pull with a fine-grained PAT vs classic `read:packages` | later (phase B) | — | Phase B |
| Q11 | ~~VPS headroom for Laya~~ | **decided**: the VPS is small; Laya runs locally, in Python, launched by `lab` | — | — |
| Q12 | Laya context vs long rule texts | open | — | Deterministic rule compression; English vs multilingual chosen by measured precision |
| Q13 | HF `Polymarket_data` coverage and license | **decided**: MIT (GitHub license API); every `OrderFilled` since 2022-11-21, so Apr 2024–Apr 2025 is covered. Caveat: fills only, no split/merge events, so split-based S-card items are labelled approximations | — | — |
| Q14 | *(retired: was the candle/Rust-port question; Laya stays Python)* | — | — | — |
| Q15 | ~~Local hardware for Laya~~ | **decided + verified**: RTX 3060 visible in WSL2 (`nvidia-smi`: driver 591.86, CUDA 13.1, ~10.9 GB free after the desktop's 1.37 GB) → CUDA oracle image, bf16, local fine-tuning feasible | — | Phase 6: confirm inside the container |
| Q16 | Paper-trading defaults | **decided**: P0–P3, $1,000 virtual each, $100 per trade, latency scenarios colocated-idle / SP-idle / SP-burst | — | — |
| Q17 | Local capture quality + restarts | **decided**: restarts allowed, **crash loops prevented** (§8.1.2: in-process failure isolation, a global connection budget, backoff reset only after 30 s of healthy session, a breaker that stops after 5 starts in 10 min with a safe-mode exit, `on-failure:5`, a warm-start universe cache, a disk guard). Gaps become censoring + `runs`/`health` rows | — | — |
| Q18 | Is Postgres reachable by the oracle with `network_mode: none`? | expected yes (Unix socket on a shared volume) | — | Phase 6 compose smoke test |
| Q19 | Can a NegRisk leg resolve other than {0,1} (UMA 50-50 / "unknown", voided or cancelled legs)? A 50-50 on a leg breaks L3/L4 exactness for \|S\| > 2 | **open (unverified)** | — | Settlement always uses the market's actual payout vector (§5.6); phase 3 checks historical NegRisk resolutions in the HF dataset / Gamma for non-binary payouts, and any found become a flagged risk class |

---

## 19. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 0 · spike (throwaway) | 1 h websocket capture of NegRisk YES legs from your machine (in Docker) | Q1, Q6, Q7, Q17 answered; L1 re-checked at scale; WSL2 clock offset measured |
| 1 · S1 | `arb-core` + all lemma/boundary tests + WASM build; `compose.yml` skeleton + rust image | Green, locally and in the builder image |
| 2 · S2 | `pm-api`, `store`, `scanner run/replay/census` | 24 h without drops; replay reproduces episodes |
| 3 · S3 | `sim` + policies P0–P3 | Deterministic replay; equity identity holds |
| 4 · S6/S7a | api + Radar/Episodes/Paper/Health pages (local) | A week of local capture browsable end to end |
| 5 · S4 | G/C cards from local capture; S-cards over the HF dataset | Cards render with hashes |
| 6 · S5 | Python oracle (vendored Laya, CUDA on the 3060) + `lab` client, candidate generator, calibration, Review page; optional fine-tune once labels suffice | Precision ≥ 0.95 at the chosen threshold on held-out **human-reviewed generator candidates** (hard negatives). Auto-labelled ladders and random negatives are excluded from the gate: they're too easy |
| 7 · S7b | Math playground (WASM), Papers page polish | — |
| 8 · deploy (phase B) | VPS: postgres/scanner/api/analyze/web; lab over SSH tunnel; CI → private GHCR → pull-based update (§17) | 14 days unattended; Q9, Q10 answered |

---

## Sources

- Polymarket docs (fetched 2026-09-22): [Fees](https://docs.polymarket.com/trading/fees) · [Market details](https://docs.polymarket.com/market-data/market-details) · [Prices & order books](https://docs.polymarket.com/market-data/prices-order-books) · [Market channel](https://docs.polymarket.com/api-reference/wss/market) · [Rate limits](https://docs.polymarket.com/api-reference/rate-limits) · [Negative risk](https://docs.polymarket.com/concepts/negative-risk) · [pUSD](https://docs.polymarket.com/concepts/pusd) · [Matching engine](https://docs.polymarket.com/trading/matching-engine) · [Collateral return](https://docs.polymarket.com/trading/combos/collateral-return) · [Changelog](https://docs.polymarket.com/changelog/predictions)
- Live endpoints (2026-09-22): Gamma `/events`, `/events/keyset`; CLOB `POST /books`, `/ok`.
- Papers: Saguillo et al. [arXiv:2508.03474](https://arxiv.org/abs/2508.03474) · Gebele et al. [arXiv:2608.00666](https://arxiv.org/abs/2608.00666) · Cheng et al. [arXiv:2605.00864](https://arxiv.org/abs/2605.00864)
- polybots (a friend's local repo, read 2026-09-22; lessons only, no code reused): `CLAUDE.md`, `docs/AUDIT_ARB_2508_03474.md`, `docs/memory/` (polymarket-wss, marketcatalog-starvation-e-keyset, fillsim-calibration-mirror-gap, latency-maker-vs-taker-clob, hora-limpa-arb-sequencial-poly-pf-29jul, fila-q0-infla-poder-nao-edge), `cmd/nestedarbpaper`, `cmd/parityscan`, `cmd/arbscan`, zero-elision tests
- Data: [SII-WANGZJ/Polymarket_data](https://huggingface.co/datasets/SII-WANGZJ/Polymarket_data)
- Laya: [model card](https://huggingface.co/convaiinnovations/laya) · [GitHub](https://github.com/NandhaKishorM/laya)
- Crates / security: [polymarket_client_sdk_v2](https://crates.io/crates/polymarket_client_sdk_v2) · [RUSTSEC-2026-0016](https://rustsec.org/advisories/RUSTSEC-2026-0016.html)
- GitHub: [Artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations)
