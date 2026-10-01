# The /math page as a story, then a playground (design)

Date: 2026-10-01. Status: design approved in conversation by the owner; this file is the written spec.
Builds on: `2026-09-30-phase7b-math-playground-design.md` (7b) and `2026-09-27` §5–§6. Web only: no change to the engine (`crates/arb-wasm`), the api, the snapshot, nginx, Traefik labels or compose.

## 1. Why

The owner, looking at the live page: "the /math page is not very well explained". Today the page opens on `D1 long 1.01 short 0.99`, uses `Π`, `Q*`, `t_x`, `sp-idle` without saying what they are, names its sections after objects ("payoff tables", "depth walk") and gives the reader controls before any reason to move them.

What the owner asked for, in their words: "it doesn't need to be for an absolute beginner, but as you scroll it need to create a history and at the end the actual playground".

- **Reader:** curious and reasonably technical, new to Polymarket and to this project.
- **Success:** a reader who scrolls to the bottom and touches nothing can say what the scanner looks for and why it does not pay on paper. A reader who wants to check can then play with the real engine.

Owner's choices in the brainstorm: story first and playground last; chapters stacked down the page (no scroll effects); each chapter's drawing has one before/after switch.

## 2. What stays as it is (binding)

- Every number on the page comes from the engine (`call(engine, '<export>', input)`) or from the public snapshot. The page computes no money. Decimals stay strings; `Number()` only for drawing geometry.
- Every `$` figure carries "on paper" or "on the table" in its row or caption. Story prose carries no figure of its own: a payout is written "one dollar" in prose, and any number a sentence needs is taken from the engine's output or built from integer ticks.
- A sentence that states an outcome (caught, missed, risk-free, crossed, filled) is chosen from the engine's output, never fixed in the copy.
- Public-page rules: no `/api/` string, no dashboard module, no inline style, no `dangerouslySetInnerHTML`, no link out; one snapshot request; no timers beyond the one 7b allows.
- 360 px without horizontal scroll; controls at least 44 px; labels in HTML beside the SVGs.
- The bundle budget (350,000 B gzip) and the bundle check.
- Sections a–f keep their ids (`#a`…`#f`), their controls, their behaviour and their tests.
- Drawings over text: a chapter is one drawing and at most three short sentences.

## 3. Page structure

```
header        isoca-radar · math
hook          two sentences (§5.0)
STORY         chapters 1–7, each a `section.m-ch` with id s1…s7
PLAYGROUND    `section#play`: heading, one sentence, the word list (§6), then sections a–f
footer        unchanged
```

The live strip ("today (UTC) on the live scanner") moves from under the header into chapter 3. The live ladder moves from the end of section f into chapter 7. Each appears once on the page.

### 3.1 A chapter

```
<section id="s3" class="m-ch" aria-labelledby="m-s3">
  <h2 id="m-s3"><span class="m-ch-n">3</span> When is that free money?</h2>
  <p class="m-ch-lead">…one to three sentences…</p>
  <div class="seg" role="group" aria-label="…">  two buttons, aria-pressed  </div>
  …the drawing (an existing schematic, read-only)…
  <p class="m-ch-take">…the takeaway for the state shown…</p>
</section>
```

- The switch is the existing `Seg` control (two options, `aria-pressed`), 44 px high. The first option is selected on load.
- The drawing is the same schematic component the playground section uses, fed by `call(engine, export, preset[state])`. No sliders, no reset.
- The takeaway is chosen from the engine's output for the state shown (§5).
- No chapter owns a timer or a request.

### 3.2 Engine and snapshot states

- Prose (heading, lead) renders at once, with or without the engine, so the story reads on a browser where WebAssembly fails.
- While the engine loads: the page's one loading line stays where it is today (once, above the story); a chapter shows no switch, drawing or takeaway.
- Engine failed: the one `ENGINE_FAILED` sentence, once; chapters show prose only; the playground section shows nothing but its heading and that sentence is not repeated.
- Snapshot missing or a live part threw: the existing `live numbers unavailable` line takes the place of the strip in chapter 3 and of the ladder in chapter 7. It may therefore appear twice, once in each place; the prose around it must still read true (it never says "above" or "below" about numbers that may be absent).
- Snapshot stale: unchanged (`the live writer is behind`).

## 4. Presets

Two engine inputs per chapter, in `web/src/math/story/presets.ts`, built from the existing `defaults.ts` values and input builders (`MIRROR_START`, `payoffInput`, `classifyInput`, `depthInput`, `episodeInput`, `basketInput`), exported from where they live today. A test runs every preset through the real engine and asserts the property the chapter's words rely on (the "must hold" column). If the real engine disagrees with this table, the engine wins and the preset is changed until the property holds; the property is never weakened.

| Ch | Export | State 1 | State 2 | Must hold (asserted on engine output) |
|---|---|---|---|---|
| 1 | `mirror_json` | `MIRROR_START` | the same book crossed (the playground's `crossed`, exported) | 1: no crossed alert; 2: the crossed alert |
| 2 | `payoff_json` | YES, S 3, `other_listed: true`, no prices | the same with `other_listed: false` | 1: no broken column; 2: at least one broken column |
| 3 | `classify_json` | three live legs, asks 0.33 / 0.33 / 0.34 (bids one tick below), not augmented | asks 0.30 / 0.32 / 0.35 | 1: no signal; 2: a YES-basket signal whose realization is arbitrage, net Π > 0 |
| 4 | `depth_walk_json` | `DEPTH_START` | the same prices, every level's size a quarter | Q* and Π both positive in each; both smaller in 2 than in 1 |
| 5 | `episode_json` | `EPISODE_START` (sp-idle) | the same scenario, update times `[0, 400, 450, 500, 550]` | 1: opened and caught; 2: opened and not caught |
| 6 | `basket_json` | `F_START` | `F_BURST` | 1: word `filled`; 2: word `caught` |
| 7 | none (snapshot) | — | — | — |

Switch labels:

| Ch | Option 1 | Option 2 |
|---|---|---|
| 1 | a healthy book | a crossed book |
| 2 | every outcome is listed | one outcome is not listed |
| 3 | prices add to `<Σ>` | prices add to `<Σ>` — both sums built from the preset's integer ticks (`1.00`, `0.97`), never typed |
| 4 | a deep book | a thin book |
| 5 | the edge stays open | it closes first |
| 6 | every leg fills | one leg misses |

`episodeInput` gains the update times as a parameter (default `EPISODE_T`), so chapter 5's second state needs no second builder.

## 5. Copy

Drafts. An implementer may change a sentence only to keep it true of the engine's output or to pass the honesty scan, and says so in the report. US spelling as on the rest of the page; no exclamation marks; the page's dry tone.

### 5.0 Hook (replaces the tagline)

> Sometimes the prices of every outcome of an event add up to less than the one dollar the winner pays. On paper, that gap is free money. This page shows how a scanner finds it, sizes it and times it, and why it is usually gone before an order can land. Every number comes from the scanner's own Rust code, compiled to WASM.

### 5.1 Chapters

1. **What is being traded?**
   Lead: "A market asks one yes-or-no question. A YES share pays one dollar if the answer is yes, a NO share pays one dollar if it is no. So an offer to sell YES at a price p is the same offer as a bid for NO at 1 − p: one book, drawn twice."
   Takeaway, healthy: "One book, two views." Crossed: the engine's alert sentence as the playground shows it, then "If both views look like free money at once, the feed is wrong, not the market."

2. **What happens with several outcomes?**
   Lead: "Think of an election with three candidates, A, B and C. Each has its own market, and exactly one of them wins. Hold one YES share of each, a basket, and you are paid one dollar whoever wins."
   Takeaway, no broken column: "The basket pays the same in every column. That is what risk-free means here." Broken: "Someone outside the basket can win, and that column pays nothing. Not an arbitrage: see ✗."

3. **When is that free money?**
   Lead: "The basket always pays one dollar. When its prices add up to less, the difference is the edge, written Π: money on the table before anyone has traded."
   After the drawing: "It is not rare." then the live strip.
   Takeaway, no signal: "At exactly one dollar there is nothing to take." Signal: "The scanner runs this check on every price update of every event."

4. **How much of it is there?**
   Lead: "Each price is good for a few shares only. Buy more and you reach the next, worse price. The walk stops where the next basket would cost more than it pays; the size it stops at is Q*."
   Takeaway (both states): "The edge is Π at Q*, not Π forever. A thinner book ends the walk sooner."

5. **How long does it last?**
   Lead: "An edge opens on one price update and closes on a later one: an episode. An order needs time to be decided, to travel and to be matched; it lands at t_x. If the episode has closed by then, there is nothing left to take."
   Takeaway: the playground's own lines (`E_QUIPS.caught` / `E_QUIPS.missed`), with "section f" in the caught line reading "the next chapter" here.

6. **What if you get only part of it?**
   Lead: "A basket is several orders, one per outcome: its legs. If one leg has no shares left at t_x, the others still fill. Those extra shares are not a basket any more. They are a bet, and the way out is to sell them back at the bid: an unwind."
   Takeaway: `F_QUIPS[word]` without its "Drain one at t_x…" instruction (there is no slider here): filled "Every leg filled the same amount, so nothing is left over."; caught as in the playground.

7. **So does it pay?**
   Lead: "The live scanner keeps paper accounts at several latencies, from zero to the one measured from this server, and settles every trade by the rules of chapter 6. This is their ledger."
   The live ladder (unchanged component, its own headline and folds).
   Takeaway (fixed, true whatever the data): "Every figure here is on paper. No order was ever sent."
   Under the ladder's axis, one dim line naming the accounts: "0 to 300: accounts that differ only in latency (ms). extras: the same rules with a burst of traffic, or with the market's delay ignored." The implementer checks this against `config/policies.toml` and the ladder's own labels and corrects it to what the accounts are.

### 5.2 Playground

Heading "Try it yourself". Line: "The same six drawings, with every control. Nothing you do here is recorded or sent anywhere."
Section titles become the chapter's subject, keeping the letter: `a · the book, twice`, `b · the basket's payoff`, `c · is this update an edge?`, `d · how much: the depth walk`, `e · how long: the episode`, `f · one leg misses`. Each section's first line under its title is a dim back-reference: "chapter 1" … "chapter 6" as a same-page link to `#s1`…`#s6` (the one kind of link the page has; `publicSafe` allows fragment links only).

## 6. The word list

A `<dl class="m-words">` at the top of the playground, one line each, in this order: YES / NO share; book, bid, ask; basket; leg; edge (Π); Q*; episode; t_x; unwind; on the table; on paper. Definitions are the story's own sentences shortened; "on the table" = "an edge the books show right now, before any order", "on paper" = "what a simulated account would have made or lost; no order was sent". No term appears in the playground's visible text without being in this list or in a chapter.

## 7. Code

New, under `web/src/math/story/`:
- `Chapter.tsx`: the shell of §3.1 (number, question, lead, optional switch, children, takeaway).
- `presets.ts`: §4's twelve inputs and the two tick-built labels.
- `Story.tsx`: the seven chapters, each a small component holding one `useState<0 | 1>`; takes `engine: Engine | null`, and the snapshot parts for chapters 3 and 7.
- `Words.tsx`: §6.

Changed:
- `copy.ts`: hook, chapter copy, word list, new section titles.
- `MathPage.tsx`: the order of §3; the strip and the ladder move into the story; `useSectionLink` also accepts `#s1`–`#s7` and `#play`.
- `sections/*.tsx`: export what the presets need (`crossed`, the input builders); `episodeInput` takes the times; `LegMissSection` loses its `children` slot; each section renders its back-reference.
- `math.css`: `.m-ch`, `.m-ch-n`, `.m-ch-lead`, `.m-ch-take`, `.m-words`, `#play`; and the live ladder's "the numbers" table keeps its first column on one line (`white-space: nowrap`: seen on the live page at 360 px, rung names broke mid-word).
- Schematics: no change expected. If a drawing needs one to be read-only friendly, it is a prop with a default that keeps today's output.

No new dependency. No change outside `web/` except this spec, the plan and a runbook.

## 8. Tests

- `presets.test.ts` (real WASM, `describeWasm`): every preset satisfies its "must hold" row; chapter 3's labels equal the sums of the preset's ticks.
- `Story.test.tsx` (real WASM): each chapter renders its heading, lead and first state; clicking the second option changes the drawing and the takeaway to the second state's; the switch is a group of two buttons with `aria-pressed`; engine `null` renders prose and no switch.
- Honesty scan (`MathPage.test.tsx`): runs over the whole page with every chapter in state 1, then in state 2, and with a market cell tapped; no unframed `$`.
- `MathPage.test.tsx`: order (story, then `#play`, then a–f); the strip is inside `#s3` and the ladder inside `#s7`, each once; snapshot missing shows the unavailable line in both places and nothing untrue; cold `#s5`, `#play` and `#d` scroll to their targets; one `h1`, every chapter and section an `h2`.
- `math.css.test.ts` / `theme.test.ts`: the new classes have no `minmax(<N>px`, the first-column `nowrap`, 44 px controls.
- `publicSafe.test.ts`: fragment-only links allowed, anything else still refused.
- Existing section tests keep passing with only the title strings and the moved ladder updated; no assertion is weakened.
- `npm run build`: the bundle check passes; the total is reported.
- The controller looks at the built page in headless Chrome at 360 and 1440 px, every chapter in both states, before the owner does.

## 9. Deploy

Web image only, by a short runbook in the owner's format (`docs/superpowers/runbooks/math-story-web-vps.md`): PC build and image checks, bundle, ship, rollback tag `cute-trading/web:pre-story` from the running container's image ID, load, `compose.yml` and `compose.web.yml` identical to the VPS copies, recreate `web` only, the public / gated / traversal probes of runbook 7b-B, Chrome at 1440 and 360. The scanner, api and oauth2-proxy are not touched.

## 10. Not in this change

Scroll-driven animation; real market names (needs a whitelist change); self-hosted fonts; a concrete named example beyond chapter 2's one sentence; translation; any change to the engine, the snapshot or the routes; the Sweet spots redesign.
