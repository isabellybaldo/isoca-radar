# The /math page as a story, then a playground (design)

Date: 2026-10-01. Status: design approved in conversation by the owner; this file is the written spec, amended after an independent review that ran every preset through the real engine.
Builds on: `2026-09-30-phase7b-math-playground-design.md` (7b) and `2026-09-27` §5–§6. Web only: no change to the engine (`crates/arb-wasm`), the api, the snapshot, nginx, Traefik labels or compose.

## 1. Why

The owner, looking at the live page: "the /math page is not very well explained". Today the page opens on `D1 long 1.01 short 0.99`, uses `Π`, `Q*`, `t_x`, `sp-idle` without saying what they are, names its sections after objects ("payoff tables", "depth walk") and gives the reader controls before any reason to move them.

What the owner asked for, in their words: "it doesn't need to be for an absolute beginner, but as you scroll it need to create a history and at the end the actual playground".

- **Reader:** curious and reasonably technical, new to Polymarket and to this project.
- **Success:** a reader who scrolls to the bottom and touches nothing can say what the scanner looks for and what can go wrong between seeing an edge and holding it. A reader who wants to check can then play with the real engine.

Owner's choices in the brainstorm: story first and playground last; chapters stacked down the page (no scroll effects); each chapter's drawing has one before/after switch.

## 2. What stays as it is (binding)

- Every number on the page comes from the engine (`call(engine, '<export>', input)`) or from the public snapshot. The page computes no money. Decimals stay strings; `Number()` only for drawing geometry.
- Every `$` figure carries "on paper" or "on the table" in its row or caption. Story prose carries no figure of its own: a payout is written "one dollar" in prose, and a number a label needs is built from integer ticks.
- A sentence that states an outcome (caught, missed, risk-free, crossed, filled) is chosen from the engine's output, never fixed in the copy. A fixed sentence must be true in every state of its chapter and whether or not the live data is there.
- The words the honesty check bans (`checkHonesty`: "made", …) stay banned.
- Public-page rules: no `/api/` string, no dashboard module, no inline style, no `dangerouslySetInnerHTML`, **no `<a>` and no `<nav>`** (the page test asserts none), one snapshot request, no timers beyond the one 7b allows.
- 360 px without horizontal scroll; controls at least 44 px; labels in HTML beside the SVGs.
- The bundle budget (350,000 B gzip) and the bundle check.
- Sections a–f keep their ids (`#a`…`#f`), their controls and their behaviour.
- Drawings over text: a chapter is one drawing and at most four short sentences of lead.

## 3. Page structure

```
header        h1  isoca-radar · math
hook          four sentences (§5.0)
STORY         chapters 1–7, each a `section.m-ch` (ids s1…s7), each with an h2
PLAYGROUND    `section#play`: h2 "Try it yourself", one line, the word list (§6), then sections a–f (each an h3)
footer        unchanged
```

The live strip ("today (UTC) on the live scanner") moves from under the header into chapter 3. The live ladder moves from the end of section f into chapter 7. Each appears once on the page.

### 3.1 A chapter

```
<section id="s3" class="m-ch" aria-labelledby="m-s3">
  <h2 id="m-s3"><span class="m-ch-n">3</span> When is that free money?</h2>
  <p class="m-ch-lead">…the lead…</p>
  <div class="seg" role="group" aria-label="…">  two buttons, aria-pressed  </div>
  …the drawing…
  <p class="m-ch-take">…the takeaway for the state shown…</p>
</section>
```

- The switch is the existing `Seg` control (two options, `aria-pressed`), 44 px high. The first option is selected on load.
- The drawing is the same schematic the playground section uses, fed by `call(engine, export, preset[state])`. No sliders, no reset. A fold that is part of a schematic (the mirrored book's "every level", the ladder's folds) stays, closed.
- The takeaway is chosen from the engine's output for the state shown (§5). If the engine answers with an error, the chapter shows the engine's sentence (the existing `Out`) and no takeaway.
- No chapter owns a timer or a request.

### 3.2 Engine and snapshot states

- **Prose always.** Every chapter's heading and lead, the playground's heading, its line and the word list render at once, with or without the engine, so the story reads where WebAssembly fails.
- **Engine loading:** the page's one loading line, once, above the story. Chapters 1–6 show no switch, drawing or takeaway. Sections a–f are not rendered.
- **Engine failed:** the one `ENGINE_FAILED` sentence, once, above the story. Chapters 1–6 show prose only. Sections a–f are not rendered.
- **Snapshot loading:** chapter 3 shows no strip and chapter 7 no ladder; nothing else changes.
- **Snapshot missing, or a live part threw:** the existing `live numbers unavailable` line stands in for the strip in chapter 3 and for the ladder in chapter 7, so it can appear twice. This is intended, and the tests that pinned "exactly once" change to "once in each of the two places".
- **Snapshot stale:** unchanged (`the live writer is behind`).
- **Cold link** (`/math#d`, `/math#play`): the page scrolls to the target once, after the engine is ready **and** the snapshot has settled (ok or missing), so the strip and the ladder arriving late do not push the target away. `#s1`–`#s7` are not link targets (no cold scroll for them).

## 4. Presets

Two engine inputs per chapter, in `web/src/math/story/presets.ts`, built from the existing `defaults.ts` values and input builders (`MIRROR_START`, `crossed`, `payoffInput`, `classifyInput`, `depthInput`, `episodeInput`, `basketInput`, `drain`), exported from where they live today. A test runs every preset through the real engine and asserts the "must hold" column. The review ran all of them: each holds as written. If a later engine disagrees, the preset changes until the property holds; the property is never weakened.

| Ch | Export | State 1 | State 2 | Must hold (asserted on engine output) |
|---|---|---|---|---|
| 1 | `mirror_json` | `MIRROR_START` | `crossed(MIRROR_START)` | 1: `crossed` false; 2: `crossed` true |
| 2 | `payoff_json` | YES, S 3, `other_listed: true`, no prices | the same with `other_listed: false` | 1: no `broken` column; 2: at least one |
| 3 | `classify_json` | three live legs, asks 0.33 / 0.33 / 0.34 (bids one tick below), not augmented | asks 0.30 / 0.32 / 0.35 | 1: kind null; 2: kind `yes_basket`, realization `settlement`, net Π > 0 |
| 4 | `depth_walk_json` | `DEPTH_START` | each leg's two cheapest levels drained to 25% of their size (the existing `drain` helper), the third level unchanged | `q_star` and `pi` positive in both, both smaller in 2; and the drawing differs (the Q* line's position is not the same) |
| 5 | `episode_json` | `EPISODE_START` (sp-idle) | the same scenario, update times `[0, 400, 450, 500, 550]` | 1: opened and `caught`; 2: opened and not `caught` |
| 6 | `basket_json` | `F_START` | `F_BURST` | 1: word `filled`; 2: word `caught` |
| 7 | none (snapshot) | — | — | — |

Quartering every size in chapter 4 was rejected: the stairs scale to their last step, so the picture would not change.

Switch labels:

| Ch | Option 1 | Option 2 |
|---|---|---|
| 1 | a healthy book | a crossed book |
| 2 | every outcome is listed | one outcome is not listed |
| 3 | prices add to `<Σ>` | prices add to `<Σ>` — both sums built from the preset's integer ticks (`1.00`, `0.97`), never typed, and without a `$` |
| 4 | a deep book | a thin book |
| 5 | it outlasts the order | it closes first |
| 6 | every leg fills | one leg misses |

`episodeInput` gains the update times as a parameter (default `EPISODE_T`).

Q* on this page is `q_star` (`depth_walk_json`, `classify_json`). `basket_json.q_open` is not Q* and is never called that.

## 5. Copy

Drafts, checked by the review against the engine and the code. An implementer may change a sentence only to keep it true of the engine's output or to pass the honesty checks, and says so in the report. No exclamation marks; the page's dry tone.

### 5.0 Hook (replaces the tagline; `math.html`'s meta description says the same in one sentence)

> Sometimes the prices of every outcome of an event add up to less than the one dollar the winner pays. In theory, that gap is free money. This page shows how a scanner finds it, sizes it and times it, and what can go wrong between seeing it and holding it. Every drawing runs the scanner's own Rust code, compiled to WASM.

### 5.1 Chapters

1. **What is being traded?**
   Lead: "A market asks one yes-or-no question. A YES share pays one dollar if the answer is yes, a NO share pays one dollar if it is no. So an offer to sell YES at a price p (an ask) is the same offer as a bid for NO at 1 − p: one list of bids and asks, a book, drawn twice."
   The drawing here hides its `D1 long … short …` headline (§7): that line is the playground's.
   Takeaway, not crossed: "One book, two views." Crossed (the drawing already prints its own crossed-book alert): "A bid above the ask would be free money inside one market. The scanner distrusts its feed instead."

2. **What happens with several outcomes?**
   Lead: "Think of an election with candidates A, B and C, each with its own market. Hold one YES share of each: a basket, and each market in it is a leg. If one of the three must win, it pays one dollar whoever it is."
   Takeaway, no broken column: "The basket pays the same in every column. That is what risk-free means here." Broken: "Someone outside the basket can win, and that column pays nothing. Not an arbitrage: see ✗."

3. **When is that free money?**
   Lead: "A complete basket always pays one dollar. When its prices add up to less, the difference, on every basket the books can fill, is the edge, written Π: money on the table before anyone has traded. Buying one NO share of each works too: all but one of them pay, whoever wins, and the edge is there when the YES bids add up to more than one dollar."
   Takeaway, no signal: "At exactly one dollar there is nothing to take." Signal: "The scanner runs this check each time a price moves."
   Then, under the takeaway: "The scanner counts each stretch an edge stays open as one episode. Today's count:" followed by the live strip (or the unavailable line; nothing while the snapshot loads, in which case the sentence is not shown either).

4. **How much of it is there?**
   Lead: "Each price is good for a few shares only. Buy more and you reach the next, worse price. Walk down the books basket by basket and stop where the next one would no longer pay more than it costs: the size reached is Q*."
   Takeaway (both states): "The edge has a size. A thinner book reaches it sooner."

5. **How long does it last?**
   Lead: "An edge opens on one price update and closes on a later one: an episode. An order needs time to be decided, to travel and to be matched; it lands at t_x. If the episode has closed by then, there is nothing left to take."
   The drawing is the episode view the playground shows (the verdict word, the captured line, the times line, the update bars and the duration bars), moved to one exported component (§7).
   Takeaway, caught: "The order landed in time. Whether every leg fills is the next chapter." Not caught: "The edge was real. It just did not wait."

6. **What if you get only part of it?**
   Lead: "A basket is several orders, one per leg. If one leg has no shares left at t_x, the others still fill, and those extra shares are a bet, not a basket. The way out is to sell them back at the bid: an unwind. The drawing uses a NO basket, which can be converted back into cash at once."
   Takeaway, word `filled`: "Every leg filled the same amount, so nothing is left over." `caught`: "Some legs filled more than others. The extra shares are now a bet, and the exit is the bid." `nothing filled`: "Nothing filled, so there is nothing to sell back."

7. **So does it pay?**
   Lead: "The live scanner keeps six paper accounts that differ only in latency, from the fastest rung to the slowest, and settles every trade by the rules of chapter 6."
   The live ladder (unchanged component: its own headline, drawing and folds), or the unavailable line, or nothing while the snapshot loads.
   Under the ladder, only when the ladder is drawn, one dim line: "extras: one account whose matching is slower and slows further when the feed is busy, and one that ignores a market's own order delay." (Checked against `config/policies.toml`; "six" is `LADDER`'s length there, and the test reads it from the file.)
   Takeaway (fixed, true whatever the data): "Every figure here is on paper. No order was ever sent."

### 5.2 Playground

Heading (h2) "Try it yourself". Line: "The same six drawings, with every control. Nothing you do here is recorded or sent anywhere."
Section titles (now h3) become the chapter's subject, keeping the letter: `a · the book, twice`, `b · the basket's payoff`, `c · is this update an edge?`, `d · how much: the depth walk`, `e · how long: the episode`, `f · one leg misses`. Under each title, a dim back-reference as plain text, not a link: "chapter 1" … "chapter 6".
Section e's caught line keeps saying "section f" (it is next to f there).

## 6. The word list

A `<dl class="m-words">` at the top of the playground, one line each, in this order:

- **YES / NO share**: pays one dollar if the answer is yes / no.
- **book, bid, ask**: the list of offers to buy (bids) and to sell (asks) one share type.
- **basket, leg**: one share of every outcome of an event; each market in it is a leg.
- **NO basket**: one NO share of each; all but one pay.
- **edge (Π)**: what a basket pays over what it costs, on every basket the books can fill.
- **Q\***: how many baskets are worth buying before the next costs as much as it pays.
- **episode**: one stretch during which an edge stays open.
- **t_x**: the moment an order lands at the exchange.
- **unwind**: selling extra shares back at the bid.
- **convert**: turning a NO basket back into cash at the exchange.
- **on the table**: an edge the books show right now, before any order.
- **on paper**: what a simulated account would have gained or lost; no order was sent.

## 7. Code

New, under `web/src/math/story/`:
- `Chapter.tsx`: the shell of §3.1 (number, question, lead, optional switch, children, takeaway).
- `presets.ts`: §4's twelve inputs and chapter 3's two tick-built labels.
- `Story.tsx`: the seven chapters, each a small component holding one `useState<0 | 1>`; takes `engine: Engine | null` and, for chapters 3 and 7, what `MathPage` knows about the snapshot.
- `Words.tsx`: §6.

Changed:
- `copy.ts`: hook, chapter copy, word list, section titles. `math.html`: the meta description.
- `MathPage.tsx`: the order of §3; the strip and the ladder move into the story; the cold-link scroll of §3.2 (`#a`–`#f` and `#play`).
- `sections/*.tsx`: export what the presets need (`crossed`, the builders); `episodeInput` takes the times; the episode's verdict, lines and two bar drawings become one exported `EpisodeView` used by section e and chapter 5; `LegMissSection` loses its `children` slot; `Section` renders an h3 and the back-reference.
- `schematics/MirrorBooks.tsx`: one prop, default as today, that hides the `D1 long … short …` headline. No other schematic changes.
- `math.css`: `.m-ch`, `.m-ch-n`, `.m-ch-lead`, `.m-ch-take`, `.m-words`, `#play`; every rule scoped to `.m-sec` that a chapter also needs (44 px `.seg` buttons and fold summaries, the frameless `.ll-svg` and folds, `scroll-margin-top`) covers `.m-ch` too; the live ladder's "the numbers" table keeps its first column on one line (`white-space: nowrap`: on the live page at 360 px the rung names broke mid-word).

No new dependency. No change outside `web/` except this spec, the plan and a runbook.

## 8. Tests

New:
- `presets.test.ts` (real WASM, `describeWasm`): every preset satisfies its "must hold" row; chapter 3's labels equal the sums of the preset's ticks; "six" equals `LADDER`'s length in `config/policies.toml`.
- `Story.test.tsx` (real WASM): each chapter renders its heading, lead and first state; the second option changes the drawing and the takeaway to the second state's; the switch is a group of two buttons with `aria-pressed`; `engine` null renders prose and no switch; chapter 1 shows no `D1 long` headline; a takeaway never contradicts the engine's flag for the state shown.
- Word list: `checkHonesty` over §5 and §6's text finds nothing.

Changed, each named because an existing assertion moves (none is dropped):
- `MathPage.test.tsx`: "no h2 while loading / when failed" becomes "chapter and playground headings, no drawing, no section a–f"; the h2 count becomes 8 (seven chapters and the playground) and the six sections are h3; `LIVE_UNAVAILABLE` exactly once becomes once inside `#s3` and once inside `#s7`; the ladder is inside `#s7`, not the last child of `#f`; order: story, `#play`, a–f; one h1; still no `a` and no `nav`.
- The honesty scan runs over the whole page with every chapter in state 1, then in state 2, and with a market cell tapped.
- Cold `#d` and `#play` scroll once, after engine and snapshot have settled (a test with a late snapshot).
- `math.css.test.ts` / `theme.test.ts`: the new classes have no `minmax(<N>px`; the first-column `nowrap`; 44 px controls inside `.m-ch`.
- Section tests: title strings, the h3, section f without the ladder.
- `npm run build`: the bundle check passes; the total is reported.
- The controller looks at the built page in headless Chrome at 360 and 1440 px, every chapter in both states, before the owner does.

## 9. Deploy

Web image only, by a short runbook in the owner's format (`docs/superpowers/runbooks/math-story-web-vps.md`): PC build and image checks, bundle, ship, rollback tag `cute-trading/web:pre-story` from the running container's image ID, load, `compose.yml` and `compose.web.yml` identical to the VPS copies, an advisory quiet minute, recreate `web` only, the public / gated / traversal probes of runbook 7b-B, Chrome at 1440 and 360. The scanner, api and oauth2-proxy are not touched.

## 10. Not in this change

Scroll-driven animation; links of any kind; real market names (needs a whitelist change); self-hosted fonts; a named example beyond chapter 2's one sentence; translation; explaining every playground term (D1, Σ, augmented, secondsDelay, q_eff stay as they are); any change to the engine, the snapshot or the routes; the Sweet spots redesign.
