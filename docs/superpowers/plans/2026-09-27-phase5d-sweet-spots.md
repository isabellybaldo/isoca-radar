# isoca-radar Phase 5d: sweet spots Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A **Sweet spots** box on the Paper page that slices the edge the paper accounts actually *captured* (not the edge on the table), after fees and capital lockup, by category, days to resolution, set size S and hour of day, for every latency scenario, with cluster-robust Bonferroni intervals and labels that never call a slice "profitable".

**Architecture:** One more card family, `sweet`, in the Phase 5a framework. `analyze` reads each executed paper attempt's own ledger rows, its 30 s markout mids and the episode's metadata (days to resolution, S, event tags) through `store`, and the pure `cards` crate turns them into hourly merge-exact cells of additive sums (counts, Σx, Σx², Σ|x|, cluster sums). The windows are the 5a `Windows` unchanged. At render time the card computes per-row means, cluster-robust intervals widened by Bonferroni over the rows tested, and a label. The api serves the stored body unchanged, and the web renders it below Edge capture. The scanner's only change is recording Gamma event tags, behind a go/no-go check.

**Tech Stack:** Rust 1.98.1 (edition 2024), tokio, sqlx 0.8 (inside `store` only), rust_decimal 1.41+ (`serde-with-str`, `maths`), sha2, serde_json, proptest; React 19, Vite 8, TypeScript 5.9, Vitest 4 + Testing Library; Postgres 17.

**Spec:** `docs/superpowers/specs/2026-09-27-phase5d-sweet-spots-design.md` (builds on `docs/superpowers/specs/2026-09-26-phase5a-cards-design.md`; parent `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` §5.6, §11). The spec was revised after an independent review (its §13); this plan follows the revised text.

This is ONE plan with 10 sequential tasks:
- Task 1: the Gamma tags go/no-go.
- Tasks 2–4: store and the scanner.
- Tasks 5–7: the pure `cards` crate.
- Task 8: `analyze`.
- Task 9: the web.
- Task 10: the deploy runbook.

Tasks 1–8 never touch `web/`; Task 9 never touches Rust. **On a no-go in Task 1, skip Task 3 entirely** and set `CATEGORY_AVAILABLE = false` in Task 5. Every other task runs unchanged.

## Global Constraints

- **Paper only (parent §16):** no order placement, wallets, keys, signing or exchange clients anywhere. Nothing in this plan changes that.
- **Money and prices are decimal strings** in JSON (cells, bodies, api), never JSON numbers. `null` (not measured) and `"0"` (measured zero) stay distinct. Trailing zeros are dropped; `-0` is `"0"` (`cards::stats::dec`).
- **The api stays read-only and unchanged:** `GET /api/cards?win=` already serves every `card_results` row for the window as stored; SWEET is one more row. No api code changes.
- **`analyze` has no network client:** the existing arch test (`bins/scanner/tests/arch.rs`) keeps banning `reqwest`, `pm-api`, `tokio-tungstenite` and `sqlx` from `analyze`; it reaches Postgres only through `store`. `crates/cards` stays pure (no tokio, sqlx, reqwest, tokio-tungstenite, pm-api, store) and keeps its no-`f32`/`f64` source scan (`crates/cards/tests/no_floats.rs`).
- **Only `store` uses sqlx** (`deny.toml`: `{ name = "sqlx", wrappers = ["store"] }`).
- **`analyze_rw` after migration 0007:** SELECT on `episodes`, `runs`, `run_premises`, `health`, `paper_marks`, `paper_accounts` (0006) **plus** `events`, `episode_legs`, `paper_intents`, `paper_execs`, `paper_ledger`, `paper_markouts`; INSERT, UPDATE, DELETE on `card_cells`, `card_results`, `card_meta` only. The startup write-refusal check (`role_writes_outside_cards`) is unchanged and must still pass.
- **No new indexes** in 0007 (spec §2): every paper table's primary key starts with `(run_id, account_id, episode_id)`, `paper_ledger` has `paper_ledger_episode`, and a non-concurrent `CREATE INDEX` would block the scanner's inserts.
- **Premise areas are untouched.** Do not edit any file listed in `crates/arb-core/src/premises.rs` `PREMISES` (`crates/arb-core/src/{episode,fill,detect,fee}.rs`, `crates/sim/src/*.rs`); `crates/arb-core/tests/premises_lock.rs` must stay green without a version bump. SWEET's own parameters are versioned card-side (`SWEET_VERSION` + sha8 of `params_text()`, as the key suffix `;sweet=…`).
- **No floats in Rust:** clippy denies `float_arithmetic`, `unwrap_used`, `expect_used` (tests may unwrap: `clippy.toml`). Statistics use `i64` counts and `rust_decimal::Decimal`; square roots use `rust_decimal::MathematicalOps::sqrt` (the `maths` feature, declared explicitly in `crates/cards/Cargo.toml`).
- **Time** is integer ms since epoch; hours are UTC: `cards::hour_of(ms) = ms.div_euclid(3_600_000) * 3_600_000`.
- **Thresholds (spec §7):** a row needs **≥ 30 executed attempts and ≥ 30 event-hour clusters**; the Bonferroni z table is `m ≤ 1: 1.960 · 5: 2.576 · 10: 2.807 · 20: 3.023 · 50: 3.291 · 100: 3.481 · 200: 3.662 · 500: 3.891`; "few trades carry this row" below an effective n of 5; in-flight attempts are those executed < 60 s before the tick; leftovers are valued at the **30 s** markout mid; the late-attempt rebuild covers **[last tick − 10 min, now]**; days buckets `[0,1) [1,7) [7,30] (30,90) [90,365) ≥365 unknown`.
- **Exact copy strings (spec):**
  - `Nothing here pays at 300 ms. Don't trade this, or get closer.`
  - `survives here; worth a real small-order test.`
  - `loses here. Don't trade this.`
  - `even first in line, the money is locked too long to be worth it.`
  - `few trades carry this row`
  - the §7.6 caveat, `Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.`
  - the §7.2 footnote, `m rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.` (with the real m)
- **Web look:** the existing theme (IBM Plex Mono body, Press Start 2P headings and tags). No horizontal page scroll at 360 px: the sweet table scrolls inside its own box. The new (?) entries live in `web/src/help.ts`. No float formatting of money (the existing `money.test.ts` spy pattern); numbers parsed from decimal strings are for geometry only.
- **No fresh `npm install` in `web/`** (npm 10.9.2 crashes): reuse the existing `web/node_modules`. **No new web dependency** and **no new crates.io crate** (new Rust dependencies come from `[workspace.dependencies]` only) without owner approval.
- **Debug builds only** on the dev machine (no `--release`, no `cargo install`). Docker builds happen only in Task 10's runbook, run by the owner.
- **Disk budgets:** 30 GB on the PC (`sh scripts/disk-budget.sh`), 50 GB for the VPS project. Run `cargo clean` if `target/` passes 8 GB.
- **Commits:** stage only the files the task touched (`git add <paths>`, never `-A` or `.`); never stage `PROJECT.md` or `README.md` (untracked, the owner's). Commit messages have **no** `Co-Authored-By` or any other attribution trailer.
- **Shell prefix for every command:** `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`.
- **Postgres-backed tests** (store, analyze) use the Postgres already listening on **127.0.0.1:15432** (not 55432). Prefix their commands with `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" &&`. A test that prints `SKIPPED: TEST_DATABASE_URL not set` does **not** count as passing. Before the first such run in a task, check the port: `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up` must print `pg-up`; if it doesn't, stop and ask the owner.
- **Secrets and the machine:** never print `.env` (no `cat`, no `grep` that shows values, no `echo` of a password); never run `docker compose config` except as `docker compose --env-file .env.example … config -q`; never restart Docker or WSL.

## Review Focus

1. **An attempt that finishes after its episode's hour was built:** an episode opens at 10:59:50, its paper attempt executes at 11:00:30, and the tick at 11:00:45 sees it `in_flight` (< 60 s old). The episode closed before that tick, so the `updated_ms` cursor will never dirty 10:00 again. The next tick must still rebuild 10:00 (the 10-minute late rule) and count the attempt exactly once. Pinned by `an_attempt_in_flight_at_one_tick_is_counted_once_at_the_next` (Task 8).
2. **Pooled positions across episodes:** two NO episodes on the same leg in one account, where one episode's conversion or unwind takes the other's shares. Per-episode attribution must follow each ledger row's `episode_id`, clamp a negative net at 0 and count it as `pooled`, and never price a share twice. Pinned by `pooled_shares_follow_each_rows_episode_and_never_count_twice` (Task 5).
3. **A missed attempt that still traded:** `q_eff = 0` with a partial buy on one leg and a partial unwind. It must count in n and in the mean with its negative captured value, never be dropped as "nothing happened". Pinned by `a_missed_attempt_with_a_partial_buy_and_unwind_is_negative` (Task 5) and `missed_attempts_count_in_the_mean` (Task 7).
4. **Many rows, little data:** a window where most of ~200 rows have n < 30 or fewer than 30 clusters. They must render as `thin` with n and C shown, stay out of m, and never become `candidate`; a window with zero executed attempts must produce a valid body (every row `thin`, m = 0, z = 1.960) rather than a division panic. Pinned by `an_empty_window_renders_every_row_thin_with_m_zero` (Task 7).
5. **The key suffix must not move other cards:** adding SWEET must leave the premise keys of G1–G4 and EDGE byte-identical (otherwise every 5a card restarts `under_review` on deploy). Pinned by `the_sweet_suffix_leaves_other_family_keys_unchanged` (Task 8).

## File Structure

```
crates/pm-api/src/gamma.rs                        MODIFY (go only): GammaTag, GammaEvent.tags (lenient)
crates/pm-api/tests/gamma.rs                      MODIFY (go only): tags parse tests
crates/pm-api/tests/data/keyset_tags_sample.json  NEW (Task 1): a trimmed live keyset page
crates/store/migrations/0007_sweet.sql            NEW: events.tags column, analyze_rw grants
crates/store/src/rows.rs                          MODIFY (go only): EventRow.tags
crates/store/src/universe.rs                      MODIFY (go only): writes tags, frozen on conflict
crates/store/src/card_io.rs                       MODIFY: sweet reads (accounts, episodes, intents, ledger, mids)
crates/store/src/lib.rs                           MODIFY: re-exports
crates/store/tests/{migrate,universe,card_io}.rs  MODIFY
bins/scanner/src/universe.rs                      MODIFY (go only): GroupMeta.tags → EventRow.tags
bins/scanner/src/checks.rs, bins/scanner/tests/{common/mod.rs,replay.rs}  MODIFY (go only): GroupMeta literals
crates/cards/Cargo.toml                           MODIFY: rust_decimal `maths`
crates/cards/src/sweet.rs                         NEW: params, buckets, captured, SweetCell, stats, labels, body
crates/cards/src/cells.rs                         MODIFY: HourInput.sweet, CellData hooks (SWEET_INPUTS, LATE_MS, key_suffix)
crates/cards/src/body.rs                          MODIFY: BodyCtx.hurdle, Series.sweet
crates/cards/src/lib.rs                           MODIFY: pub mod sweet
crates/cards/tests/sweet.rs                       NEW
crates/cards/tests/{cells,edge,merge_property,bench_tick,bodies}.rs  MODIFY: new HourInput/BodyCtx fields; bench + sweet
bins/analyze/src/{tick,scenarios,main}.rs         MODIFY
bins/analyze/tests/tick.rs                        MODIFY: sweet tests
web/src/api/types.ts                              MODIFY: SweetSeries
web/src/lib/sweet.ts, web/src/lib/sweet.test.ts   NEW
web/src/components/SweetSpots.tsx, .test.tsx      NEW
web/src/pages/Paper.tsx, web/src/help.ts          MODIFY
docs/superpowers/runbooks/phase5d-sweet-vps.md    NEW
```

---

### Task 1: go/no-go: do Gamma keyset events carry tags?

The scanner reads events from `GAMMA_EVENTS_KEYSET` (`/events/keyset`, `crates/pm-api/src/routes.rs:7`) with `closed=false&active=true&limit=100` (`crates/pm-api/src/gamma.rs:286-304`). `GammaEvent` has no `tags` field today (`gamma.rs:48-59`). One read-only request decides whether the Category dimension exists (spec §3).

**Files:**
- Create: `crates/pm-api/tests/data/keyset_tags_sample.json`
- Modify: this plan's **Results** section (at the end)

**Interfaces:**
- Produces: the decision `GO` or `NO-GO`, and (on GO) the bucket slug lists that Task 5 writes into `CATEGORIES`.

- [ ] **Step 1: One read-only request, trimmed to the fields we need**

This is a public, unauthenticated GET, the same route the scanner already calls every universe refresh. `python` trims each event to its id, title and tags, so the fixture stays small:
```bash
curl -sS --max-time 20 'https://gamma-api.polymarket.com/events/keyset?closed=false&active=true&limit=100' -o /tmp/keyset_raw.json
python - <<'EOF'
import json
raw = json.load(open('/tmp/keyset_raw.json', encoding='utf-8'))
def tags(e):
    t = e.get('tags')
    if not isinstance(t, list):
        return None
    return [{'slug': x.get('slug'), 'label': x.get('label')} for x in t if isinstance(x, dict)]
out = {'events': [{'id': e.get('id'), 'title': e.get('title'), 'tags': tags(e)} for e in raw.get('events', [])],
       'next_cursor': raw.get('next_cursor')}
json.dump(out, open('crates/pm-api/tests/data/keyset_tags_sample.json', 'w', encoding='utf-8'), indent=1)
ev = out['events']
print('events', len(ev), 'with_tags', sum(1 for e in ev if e['tags'] and any(t.get('slug') for t in e['tags'])))
EOF
rm -f /tmp/keyset_raw.json
```
Use the scratchpad directory instead of `/tmp` if the session provides one. Do not keep the full response: it's large and contains fields we don't need.

- [ ] **Step 2: Decide**

- **GO** if at least 80 of the 100 events have a non-empty `tags` array whose entries carry a `slug`.
- **NO-GO** otherwise, including when the endpoint returns no `tags` key at all.

- [ ] **Step 3: On GO, list the slugs and fix the bucket lists**

```bash
python - <<'EOF'
import json, collections
ev = json.load(open('crates/pm-api/tests/data/keyset_tags_sample.json', encoding='utf-8'))['events']
c = collections.Counter(t['slug'] for e in ev for t in (e['tags'] or []) if t.get('slug'))
for s, n in c.most_common(60):
    print(n, s)
EOF
```
Start from the lists below (Task 5 writes them into `crates/cards/src/sweet.rs`). Add every slug from the sample that plainly belongs to a bucket, and leave out ambiguous or structural slugs such as `featured`, `all` and `hide-from-new`: tagged events that match nothing fall into `other`.
```text
politics : politics, us-politics, elections, us-election, trump, congress, senate
sports   : sports, nba, nfl, mlb, nhl, soccer, football, tennis, golf, mma, ufc, f1, boxing, esports, cricket, basketball
crypto   : crypto, bitcoin, ethereum, solana, crypto-prices, xrp, memecoins
economy  : economy, finance, business, fed, fed-rates, inflation, stocks, earnings, commodities
world    : world, geopolitics, international-elections, middle-east, ukraine, china, europe
culture  : pop-culture, culture, entertainment, awards, music, movies, tv, celebrities, oscars, grammys
tech     : tech, technology, ai, science, space
```
Each slug may appear in only one bucket. On a clash, the earlier bucket in the order above wins, which is also the runtime rule.

- [ ] **Step 4: Record the result**

In **Results** at the end of this plan, write:
- the date and time (UTC);
- the two counts from Step 1;
- `GO` or `NO-GO`;
- on GO, the final seven lists.

- [ ] **Step 5: Commit**

On GO, commit the fixture and the plan together. On NO-GO, commit only the plan (delete the fixture file; it is not needed).
```bash
git add crates/pm-api/tests/data/keyset_tags_sample.json docs/superpowers/plans/2026-09-27-phase5d-sweet-spots.md
git commit -m "5d: Gamma keyset tags go/no-go (sample and decision)"
```

---

### Task 2: store: migration 0007 (`events.tags`, `analyze_rw` grants)

The migration runs on both GO and NO-GO: the nullable column is free (an `ADD COLUMN … NULL` is a catalog change in Postgres), and the grants are needed either way.

**Files:**
- Create: `crates/store/migrations/0007_sweet.sql`
- Modify: `crates/store/tests/migrate.rs`

**Interfaces:**
- Produces: `events.tags TEXT[] NULL`; `analyze_rw` can SELECT `events`, `episode_legs`, `paper_intents`, `paper_execs`, `paper_ledger`, `paper_markouts`.

- [ ] **Step 1: Write the failing test**

Append to `crates/store/tests/migrate.rs`. Model it on `analyze_rw_reads_the_scanner_tables_and_writes_only_card_tables` (`crates/store/tests/migrate.rs:264`), which already shows how that file opens a role (`db.role("analyze_rw")`), runs a probe query and checks `role_writes_outside_cards`. Reuse exactly the helpers that test uses; add no new public store API for this test.
```rust
#[tokio::test]
async fn migration_0007_adds_tags_and_lets_analyze_rw_read_the_sweet_inputs() {
    let Some((owner, db)) = common::fresh().await else { return };
    let rw = db.role("analyze_rw").await;
    // the six new reads succeed (same probe style as the 0006 role test, one per table)
    for t in ["events", "episode_legs", "paper_intents", "paper_execs", "paper_ledger", "paper_markouts"] {
        probe_select(&rw, t).await.unwrap_or_else(|e| panic!("analyze_rw reads {t}: {e}"));
    }
    // the tags column exists: selecting it is not an error
    probe_select_column(&rw, "events", "tags").await.expect("events.tags exists");
    // still no write outside the card tables
    assert!(rw.role_writes_outside_cards().await.unwrap().is_empty());
    db.drop(owner).await;
}
```
`probe_select` / `probe_select_column` are the names to give to whatever the 0006 test does inline (`SELECT 1 FROM {t} LIMIT 1`, `SELECT {col} FROM {t} LIMIT 1` through the test-only admin query helper it uses). Write them as small `async fn`s at the bottom of the test file, built from that same helper. Idempotency is already covered by `migrations_apply_from_empty_and_are_idempotent` (`migrate.rs:5`), which now applies 0007 twice.

- [ ] **Step 2: Run it to see it fail**

Run (Postgres prefix): `cargo test -p store --test migrate migration_0007 -- --nocapture`
Expected: FAIL (permission denied on `events` for `analyze_rw`, and no `tags` column).

- [ ] **Step 3: Write the migration**

`crates/store/migrations/0007_sweet.sql`:
```sql
-- Phase 5d (spec 2026-09-27-phase5d-sweet-spots-design.md §2, §3): sweet spots. Runs as the owner.

-- 1. Gamma event tags, lower-cased slugs, at most 16, frozen at first sight by the universe upsert
--    (COALESCE on conflict). Nullable: NULL is "not recorded" (events gone before this deploy, or a
--    no-go on tags, spec §3). A catalog-only change: no rewrite, no long lock.
ALTER TABLE events ADD COLUMN IF NOT EXISTS tags TEXT[] NULL;

-- 2. analyze_rw reads each attempt's rows and the episodes' metadata. Still no write outside the
--    card tables (0006's startup check is unchanged). No new index: every paper table's primary key
--    starts with (run_id, account_id, episode_id), and a non-concurrent CREATE INDEX would block
--    the scanner's inserts (spec §2).
GRANT SELECT ON events, episode_legs, paper_intents, paper_execs, paper_ledger, paper_markouts TO analyze_rw;
```

- [ ] **Step 4: Run it to see it pass, and the rest of store**

Run (Postgres prefix): `cargo test -p store`
Expected: PASS, including `migrations_apply_from_empty_and_are_idempotent` and the 0006 role test.

- [ ] **Step 5: Commit**

```bash
git add crates/store/migrations/0007_sweet.sql crates/store/tests/migrate.rs
git commit -m "store: migration 0007 (events.tags, analyze_rw reads the sweet inputs)"
```

---

### Task 3 (GO only): record Gamma event tags, frozen at first sight

Skip this whole task on NO-GO.

**Files:**
- Modify: `crates/pm-api/src/gamma.rs:48-59` (GammaEvent), `crates/pm-api/tests/gamma.rs`
- Modify: `crates/store/src/rows.rs:22-29` (EventRow), `crates/store/src/universe.rs:95-122` (the events upsert), `crates/store/tests/universe.rs`
- Modify: `bins/scanner/src/universe.rs:83-90,179-186,228-235` (GroupMeta and its two conversions), `bins/scanner/src/checks.rs:419-428`, `bins/scanner/tests/common/mod.rs:28-36`, `bins/scanner/tests/replay.rs:60`

**Interfaces:**
- Produces:
  - `pm_api::gamma::GammaTag { pub slug: Option<String>, pub label: Option<String> }`
  - `pm_api::gamma::MAX_TAGS: usize = 16`
  - `GammaEvent::tag_slugs(&self) -> Vec<String>`: lower-cased, trimmed, non-empty, deduplicated, at most 16, in Gamma's order.
  - `store::EventRow.tags: Option<Vec<String>>`
  - `GroupMeta.tags: Option<Vec<String>>` (`#[serde(default)]`)

- [ ] **Step 1: Failing pm-api tests**

Append to `crates/pm-api/tests/gamma.rs`:
```rust
#[test]
fn tags_parse_leniently_and_slugs_are_lowercased_deduplicated_and_capped() {
    let text = r#"{"events": [
      {"id": "1", "tags": [{"slug": "Politics", "label": "Politics"}, {"slug": "politics"}, {"label": "no slug"}, {"slug": ""}]},
      {"id": "2"},
      {"id": "3", "tags": "not an array"},
      {"id": "4", "tags": [{"slug": 7}]},
      {"id": "5", "tags": [{"slug":"a"},{"slug":"b"},{"slug":"c"},{"slug":"d"},{"slug":"e"},{"slug":"f"},{"slug":"g"},{"slug":"h"},
                           {"slug":"i"},{"slug":"j"},{"slug":"k"},{"slug":"l"},{"slug":"m"},{"slug":"n"},{"slug":"o"},{"slug":"p"},{"slug":"q"}]}
    ], "next_cursor": null}"#;
    let p = parse_page(text).unwrap();
    assert_eq!(p.events.len(), 5, "a malformed tags field never drops the event");
    assert_eq!(p.events[0].tag_slugs(), vec!["politics".to_string()]);
    assert!(p.events[1].tag_slugs().is_empty());
    assert!(p.events[2].tag_slugs().is_empty());
    assert!(p.events[3].tag_slugs().is_empty());
    assert_eq!(p.events[4].tag_slugs().len(), 16);
    assert_eq!(p.events[4].tag_slugs()[15], "p");
}

#[test]
fn the_live_keyset_sample_parses_with_tags() {
    let text = include_str!("data/keyset_tags_sample.json");
    let p = parse_page(text).unwrap();
    assert!(p.events.iter().filter(|e| !e.tag_slugs().is_empty()).count() >= 80);
}
```
The sample has no `markets` key per event (Task 1 trimmed it). `GammaEvent.markets` is `Option`, so that parses.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p pm-api --test gamma tags`
Expected: FAIL to compile (`tag_slugs` not found).

- [ ] **Step 3: Implement the lenient field**

In `crates/pm-api/src/gamma.rs`, add to `GammaEvent`:
```rust
    /// Gamma's event tags (spec 5d §3). Anything malformed reads as None: tags are metadata, never
    /// a reason to drop an event.
    #[serde(default, deserialize_with = "lenient_tags")]
    pub tags: Option<Vec<GammaTag>>,
```
and next to the existing `string_or_number` helper:
```rust
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GammaTag {
    #[serde(default, deserialize_with = "lenient_string")]
    pub slug: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub label: Option<String>,
}

/// Max tags kept per event (spec 5d §3).
pub const MAX_TAGS: usize = 16;

fn lenient_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().map(str::to_string))
}

fn lenient_tags<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<GammaTag>>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let Some(items) = v.as_array() else { return Ok(None) };
    Ok(Some(
        items
            .iter()
            .filter_map(|i| serde_json::from_value::<GammaTag>(i.clone()).ok())
            .collect(),
    ))
}
```
and, inside the existing `impl GammaEvent`:
```rust
    /// Lower-cased, trimmed, non-empty, deduplicated slugs in Gamma's order, at most MAX_TAGS.
    pub fn tag_slugs(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in self.tags.as_deref().unwrap_or(&[]) {
            let Some(s) = t.slug.as_deref().map(|s| s.trim().to_lowercase()) else { continue };
            if s.is_empty() || out.contains(&s) {
                continue;
            }
            out.push(s);
            if out.len() == MAX_TAGS {
                break;
            }
        }
        out
    }
```

- [ ] **Step 4: Run the pm-api tests to see them pass**

Run: `cargo test -p pm-api`
Expected: PASS.

- [ ] **Step 5: Failing store test: tags are written and frozen**

Add `tags: None` to the `event()` helper in `crates/store/tests/universe.rs`, then append a test. Use the universe write method this file already calls (`grep -n "upsert\|write_universe" crates/store/tests/universe.rs`) in place of `upsert_universe` below, with the same argument shape.
```rust
#[tokio::test]
async fn event_tags_are_written_once_and_frozen() {
    let Some((store, db)) = common::fresh().await else { return };
    let mut e = event("ev1");
    e.tags = Some(vec!["politics".into(), "us-election".into()]);
    store.upsert_universe(&[e.clone()], &[], 1_000).await.unwrap();
    e.tags = Some(vec!["sports".into()]); // a later refresh with other tags changes nothing
    store.upsert_universe(&[e.clone()], &[], 2_000).await.unwrap();
    let mut f = event("ev2"); // first seen without tags, gains them later
    store.upsert_universe(&[f.clone()], &[], 1_000).await.unwrap();
    f.tags = Some(vec!["crypto".into()]);
    store.upsert_universe(&[f], &[], 2_000).await.unwrap();
    assert_eq!(store.event_tags(&["ev1".into(), "ev2".into()]).await.unwrap(), vec![
        ("ev1".to_string(), Some(vec!["politics".to_string(), "us-election".to_string()])),
        ("ev2".to_string(), Some(vec!["crypto".to_string()])),
    ]);
    db.drop(store).await;
}
```
`event_tags` is a small public read in `crates/store/src/universe.rs` (`SELECT event_id, tags FROM events WHERE event_id = ANY($1) ORDER BY event_id`, returning `Vec<(String, Option<Vec<String>>)>`). The runbook (Task 10) also uses it, through psql, to check tags arrive.

- [ ] **Step 6: Implement the store side**

- `crates/store/src/rows.rs`: `EventRow` gains `pub tags: Option<Vec<String>>,` with the doc line `/// Lower-cased Gamma tag slugs (≤ 16); None: not recorded.`
- `crates/store/src/universe.rs`:
  - add `tags` to the INSERT column list and to `push_values`: `.push_bind(e.tags.as_ref().map(|t| t.iter().map(|s| clean(s)).collect::<Vec<_>>()))`. sqlx binds `Option<Vec<String>>` as `TEXT[]`, and `clean` is the module's NUL-stripping helper;
  - extend the conflict clause to `…, gone_ms = NULL, tags = COALESCE(events.tags, EXCLUDED.tags)`;
  - add `event_tags`.

Run (Postgres prefix): `cargo test -p store`. Fix every `EventRow { … }` literal the compiler lists by adding `tags: None`.
Expected: PASS.

- [ ] **Step 7: Scanner: carry the slugs from Gamma to the row**

- `bins/scanner/src/universe.rs`:
  - `GroupMeta` gains `#[serde(default)] pub tags: Option<Vec<String>>,`. The default keeps old `universe_snapshots` readable.
  - At `:179`, set `tags: Some(e.tag_slugs()).filter(|t| !t.is_empty()),`.
  - At `:228`, set `tags: g.tags.clone(),`.
- Add `tags: None` to the `GroupMeta` literals in `bins/scanner/src/checks.rs:420`, `bins/scanner/tests/common/mod.rs:29` and `bins/scanner/tests/replay.rs:60`.

Add a unit test in `bins/scanner/src/universe.rs`'s `#[cfg(test)]` module (create one if absent):
```rust
#[test]
fn store_rows_carry_the_event_tags_and_old_snapshots_still_load() {
    let mut u = Universe::default();
    u.groups.push(GroupMeta {
        event_id: "e1".into(), slug: None, title: None, neg_risk_market_id: None,
        augmented: false, legs: vec![], tags: None,
    });
    // a snapshot written before this field existed: drop "tags" from the JSON and reload it
    let mut v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&u).unwrap()).unwrap();
    v["groups"][0].as_object_mut().unwrap().remove("tags");
    let old = Universe::from_json(&v.to_string()).unwrap();
    assert_eq!(old.groups[0].tags, None);
    u.groups[0].tags = Some(vec!["politics".into()]);
    let (events, _) = u.store_rows();
    assert_eq!(events[0].tags, Some(vec!["politics".to_string()]));
}
```
If `Universe` has no `Default`, build it the way the nearest existing test in this file or `bins/scanner/tests/replay.rs:60` does.

Run: `cargo test -p scanner` and `cargo clippy --workspace --all-targets -- -D warnings`.
Expected: PASS, no warnings. Also run `cargo test -p arb-core --test premises_lock`: still green, because no locked file changed.

- [ ] **Step 8: Commit**

```bash
git add crates/pm-api/src/gamma.rs crates/pm-api/tests/gamma.rs crates/store/src/rows.rs crates/store/src/universe.rs crates/store/tests/universe.rs bins/scanner/src/universe.rs bins/scanner/src/checks.rs bins/scanner/tests/common/mod.rs bins/scanner/tests/replay.rs
git commit -m "scanner, store, pm-api: record Gamma event tags, frozen at first sight"
```

---

### Task 4: store: what `analyze` reads for SWEET

**Files:**
- Modify: `crates/store/src/card_io.rs`, `crates/store/src/lib.rs` (re-exports), `crates/store/tests/card_io.rs`

**Interfaces:**
- Consumes: migration 0007 (Task 2).
- Produces the following on `impl Store` (analyze pool). Each read is capped with the module's `capped` helper at `MAX_SWEET_ROWS = 400_000` and fails with `StoreError::TooManyRows` rather than truncating.
```rust
pub const MAX_SWEET_ROWS: i64 = 400_000;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SweetAccount { pub account_id: String, pub policy: String, pub scenario: String, pub fee_bips: Decimal }
pub async fn sweet_accounts(&self) -> Result<Vec<SweetAccount>, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SweetEpisodeRow {
    pub episode_id: String, pub run_id: i64, pub event_id: String, pub kind: String,
    pub opened_ms: i64, pub pnl_net: Decimal, pub budget100: Option<Decimal>,
    pub suspect: bool, pub stale: bool, pub days_to_end: Option<Decimal>,
    pub annualized: Option<Decimal>, pub peak_cost: Decimal, pub s_legs: i64,
    pub tags: Option<Vec<String>>,
}
/// Live yes/no-basket episodes of `run_ids` OPENED in [from_ms, to_ms).
pub async fn sweet_episodes(&self, run_ids: &[i64], from_ms: i64, to_ms: i64) -> Result<Vec<SweetEpisodeRow>, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SweetIntentRow {
    pub run_id: i64, pub account_id: String, pub episode_id: String,
    pub skip_reason: Option<String>, pub planned_exec_ms: Option<i64>,
    pub exec_ms: Option<i64>, pub q_eff: Option<Decimal>,
}
/// Intents (LEFT JOIN execs) of `accounts` on the episodes `sweet_episodes` returns for the same range.
pub async fn sweet_intents(&self, run_ids: &[i64], accounts: &[String], from_ms: i64, to_ms: i64) -> Result<Vec<SweetIntentRow>, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SweetLedgerRow {
    pub run_id: i64, pub account_id: String, pub episode_id: String, pub kind: String,
    pub market_id: Option<String>, pub outcome: Option<String>, pub qty: Decimal, pub cash_delta: Decimal,
}
/// buy / sell / convert ledger rows of `accounts` carrying one of those episodes' ids, in seq order.
pub async fn sweet_ledger(&self, run_ids: &[i64], accounts: &[String], from_ms: i64, to_ms: i64) -> Result<Vec<SweetLedgerRow>, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SweetMidRow {
    pub run_id: i64, pub account_id: String, pub episode_id: String,
    pub market_id: String, pub outcome: String, pub mid: Option<Decimal>,
}
/// The markout mids at `offset_ms` (analyze passes 30_000) for those attempts.
pub async fn sweet_mids(&self, run_ids: &[i64], accounts: &[String], from_ms: i64, to_ms: i64, offset_ms: i64) -> Result<Vec<SweetMidRow>, StoreError>;
```

- [ ] **Step 1: Write the failing test**

Append to `crates/store/tests/card_io.rs`, which already has `ep`, `run_at`, `write` and `rw` helpers (lines 8-104).
- **Seed:**
  - one run; two episodes, one opened inside the hour and one outside, with three legs, two of them `in_s`;
  - four accounts: P0 fee 0, P0 fee 10, P1 fee 0, and a P3 that `sweet_accounts` must ignore;
  - intents and one exec;
  - ledger rows: a buy, a buy, a convert, then a settle and a mark, which must both be excluded;
  - two markouts, at offsets 1000 and 30000;
  - on GO, an `events` row with tags.
- **Row builders:** add small builders at the top of the file for `EpisodeLegRow`, `PaperAccountRow`, `PaperIntentRow`, `PaperExecRow`, `PaperLedgerRow` and `PaperMarkoutRow` (`crates/store/src/rows.rs:103-120,192-285`). Fill every required field with plain values. `ledger(run, account, seq, kind, episode, leg, qty, cash)` takes `leg: Option<(&str, &str)>` for (market, outcome) and zeroes or Nones the rest.
```rust
#[tokio::test]
async fn sweet_reads_return_one_hours_attempts_and_nothing_else() {
    let Some((owner, db)) = common::fresh().await else { return };
    let run = run_at(&owner, "live", 0).await;
    let t = 10 * H;
    let mut inside = ep("in", run, "no_basket", t + 60_000, Some(t + 70_000));
    inside.event_id = "ev-in".into();
    inside.days_to_end = Some(dec!(3.5));
    let outside = ep("out", run, "no_basket", t + H + 5, Some(t + H + 10));
    let p0 = "P0-converter/sp-idle";
    owner.write_batch(&Batch {
        episodes: vec![
            EpisodeWrite { row: inside, legs: vec![leg_row("in", 0, true), leg_row("in", 1, true), leg_row("in", 2, false)] },
            EpisodeWrite { row: outside, legs: vec![] },
        ],
        paper_accounts: vec![
            acct(run, p0, "P0-converter", "sp-idle", dec!(0)),
            acct(run, "P0-converter/sp-idle/fee10", "P0-converter", "sp-idle", dec!(10)),
            acct(run, "P1-settlement/sp-idle", "P1-settlement", "sp-idle", dec!(0)),
            acct(run, "P3-naive/sp-idle", "P3-naive", "sp-idle", dec!(0)),
        ],
        paper_intents: vec![
            intent(run, p0, "in", None, Some(t + 60_300)),
            intent(run, "P1-settlement/sp-idle", "in", Some("too_long"), None),
            intent(run, p0, "out", None, Some(t + H + 300)),
        ],
        paper_execs: vec![exec(run, p0, "in", t + 60_300, dec!(10))],
        paper_ledger: vec![
            ledger(run, p0, 1, "buy", Some("in"), Some(("m0", "no")), dec!(10), dec!(-4.0)),
            ledger(run, p0, 2, "buy", Some("in"), Some(("m1", "no")), dec!(10), dec!(-5.1)),
            ledger(run, p0, 3, "convert", Some("in"), None, dec!(10), dec!(10)),
            ledger(run, p0, 4, "settle", None, Some(("m2", "yes")), dec!(10), dec!(0)),
            ledger(run, p0, 5, "mark", None, None, dec!(0), dec!(0)),
        ],
        paper_markouts: vec![
            markout(run, p0, "in", "m0", "no", 1_000, dec!(0.41)),
            markout(run, p0, "in", "m0", "no", 30_000, dec!(0.39)),
        ],
        ..Batch::default()
    }).await.unwrap();
    let rw = rw(&db).await;
    let accts = rw.sweet_accounts().await.unwrap();
    let ids: Vec<&str> = accts.iter().map(|a| a.account_id.as_str()).collect();
    assert_eq!(ids, vec![p0, "P0-converter/sp-idle/fee10", "P1-settlement/sp-idle"]);
    let all: Vec<String> = accts.iter().map(|a| a.account_id.clone()).collect();
    let eps = rw.sweet_episodes(&[run], t, t + H).await.unwrap();
    assert_eq!(eps.len(), 1);
    assert_eq!((eps[0].s_legs, eps[0].days_to_end), (2, Some(dec!(3.5))));
    let ints = rw.sweet_intents(&[run], &all, t, t + H).await.unwrap();
    assert_eq!(ints.len(), 2);
    let a = ints.iter().find(|i| i.account_id == p0).unwrap();
    assert_eq!((a.exec_ms, a.q_eff), (Some(t + 60_300), Some(dec!(10))));
    let led = rw.sweet_ledger(&[run], &all, t, t + H).await.unwrap();
    assert_eq!(led.iter().map(|l| l.kind.as_str()).collect::<Vec<_>>(), vec!["buy", "buy", "convert"]);
    let mids = rw.sweet_mids(&[run], &all, t, t + H, 30_000).await.unwrap();
    assert_eq!(mids.iter().map(|m| m.mid).collect::<Vec<_>>(), vec![Some(dec!(0.39))]);
    db.drop(rw).await;
    owner.close().await;
}
```
On GO, also upsert an `EventRow` for `ev-in` with `tags: Some(vec!["politics".into()])` before the reads and assert `eps[0].tags == Some(vec!["politics".to_string()])`; on NO-GO assert `eps[0].tags == None`. Match this file's existing teardown order (`db.drop(...)`).

- [ ] **Step 2: Run it to see it fail**

Run (Postgres prefix): `cargo test -p store --test card_io sweet_reads`
Expected: FAIL to compile (`sweet_accounts` not found).

- [ ] **Step 3: Implement the reads**

In `crates/store/src/card_io.rs`, add the structs from **Interfaces** and:
```rust
/// Row cap for the SWEET reads. A 24-hour catch-up batch at ~20 arbitrage episodes an hour and 11
/// accounts is ~5k attempts and ~25k ledger rows: far below.
pub const MAX_SWEET_ROWS: i64 = 400_000;

/// The episodes one SWEET read covers (live yes/no baskets of the runs, opened in [$2, $3)), as a
/// CTE the attempt reads share.
const SWEET_EPS: &str = "WITH se AS (SELECT e.episode_id, e.run_id FROM episodes e \
     WHERE e.source = 'live' AND e.run_id = ANY($1) AND e.kind IN ('yes_basket', 'no_basket') \
       AND e.opened_server_ms >= $2 AND e.opened_server_ms < $3) ";

impl Store {
    /// The accounts SWEET reads (spec 5d §4): P0 at fee 0, 10 and 50 (NO side) and P1 at fee 0
    /// (YES side), one row per account id (its newest run's row).
    pub async fn sweet_accounts(&self) -> Result<Vec<SweetAccount>, StoreError> {
        let rows = sqlx::query_as(
            "SELECT DISTINCT ON (account_id) account_id, policy, scenario, fee_bips FROM paper_accounts \
             WHERE (policy = 'P0-converter' AND fee_bips IN (0, 10, 50)) \
                OR (policy = 'P1-settlement' AND fee_bips = 0) \
             ORDER BY account_id, run_id DESC LIMIT $1",
        )
        .bind(MAX_SWEET_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_SWEET_ROWS, "sweet_accounts")
    }

    pub async fn sweet_episodes(
        &self,
        run_ids: &[i64],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<SweetEpisodeRow>, StoreError> {
        if run_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as(
            "SELECT e.episode_id, e.run_id, e.event_id, e.kind, e.opened_server_ms AS opened_ms, \
                    e.peak_pnl_net_usd AS pnl_net, e.pnl_net_budget100_usd AS budget100, \
                    e.engine_suspect AS suspect, e.stale_transport AS stale, e.days_to_end, \
                    e.annualized_return AS annualized, e.peak_cost_usd AS peak_cost, \
                    (SELECT count(*) FROM episode_legs l WHERE l.episode_id = e.episode_id AND l.in_s) AS s_legs, \
                    ev.tags \
             FROM episodes e LEFT JOIN events ev ON ev.event_id = e.event_id \
             WHERE e.source = 'live' AND e.run_id = ANY($1) AND e.kind IN ('yes_basket', 'no_basket') \
               AND e.opened_server_ms >= $2 AND e.opened_server_ms < $3 \
             ORDER BY e.opened_server_ms, e.episode_id LIMIT $4",
        )
        .bind(run_ids.to_vec())
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_SWEET_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_SWEET_ROWS, "sweet_episodes")
    }

    pub async fn sweet_intents(
        &self,
        run_ids: &[i64],
        accounts: &[String],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<SweetIntentRow>, StoreError> {
        if run_ids.is_empty() || accounts.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "{SWEET_EPS}SELECT i.run_id, i.account_id, i.episode_id, i.skip_reason, \
                    i.exec_ms AS planned_exec_ms, x.exec_ms, x.q_eff \
             FROM se JOIN paper_intents i \
               ON i.run_id = se.run_id AND i.account_id = ANY($4) AND i.episode_id = se.episode_id \
             LEFT JOIN paper_execs x \
               ON x.run_id = i.run_id AND x.account_id = i.account_id AND x.episode_id = i.episode_id \
             ORDER BY i.run_id, i.account_id, i.episode_id LIMIT $5"
        );
        let rows = sqlx::query_as(&sql)
            .bind(run_ids.to_vec())
            .bind(from_ms)
            .bind(to_ms)
            .bind(accounts.to_vec())
            .bind(MAX_SWEET_ROWS + 1)
            .fetch_all(self.pool())
            .await?;
        capped(rows, MAX_SWEET_ROWS, "sweet_intents")
    }

    pub async fn sweet_ledger(
        &self,
        run_ids: &[i64],
        accounts: &[String],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<SweetLedgerRow>, StoreError> {
        if run_ids.is_empty() || accounts.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "{SWEET_EPS}SELECT l.run_id, l.account_id, l.episode_id, l.kind, l.market_id, l.outcome, \
                    l.qty, l.cash_delta \
             FROM se JOIN paper_ledger l ON l.episode_id = se.episode_id AND l.run_id = se.run_id \
             WHERE l.account_id = ANY($4) AND l.kind IN ('buy', 'sell', 'convert') \
             ORDER BY l.run_id, l.account_id, l.seq LIMIT $5"
        );
        let rows = sqlx::query_as(&sql)
            .bind(run_ids.to_vec())
            .bind(from_ms)
            .bind(to_ms)
            .bind(accounts.to_vec())
            .bind(MAX_SWEET_ROWS + 1)
            .fetch_all(self.pool())
            .await?;
        capped(rows, MAX_SWEET_ROWS, "sweet_ledger")
    }

    pub async fn sweet_mids(
        &self,
        run_ids: &[i64],
        accounts: &[String],
        from_ms: i64,
        to_ms: i64,
        offset_ms: i64,
    ) -> Result<Vec<SweetMidRow>, StoreError> {
        if run_ids.is_empty() || accounts.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "{SWEET_EPS}SELECT m.run_id, m.account_id, m.episode_id, m.market_id, m.outcome, m.mid \
             FROM se JOIN paper_markouts m \
               ON m.run_id = se.run_id AND m.account_id = ANY($4) AND m.episode_id = se.episode_id \
             WHERE m.offset_ms = $6 \
             ORDER BY m.run_id, m.account_id, m.episode_id, m.market_id, m.outcome LIMIT $5"
        );
        let rows = sqlx::query_as(&sql)
            .bind(run_ids.to_vec())
            .bind(from_ms)
            .bind(to_ms)
            .bind(accounts.to_vec())
            .bind(MAX_SWEET_ROWS + 1)
            .bind(offset_ms)
            .fetch_all(self.pool())
            .await?;
        capped(rows, MAX_SWEET_ROWS, "sweet_mids")
    }
}
```
Re-export from `crates/store/src/lib.rs` by adding `MAX_SWEET_ROWS, SweetAccount, SweetEpisodeRow, SweetIntentRow, SweetLedgerRow, SweetMidRow` to the `pub use card_io::{…}` list. The `tags` column always exists (Task 2), so these queries are the same on NO-GO, and `tags` is then always None.

- [ ] **Step 4: Check that the plans use the primary keys**

With the Postgres prefix, open psql on 127.0.0.1:15432 against the owner database. Run `EXPLAIN` on the `sweet_intents` and `sweet_mids` SQL with literal arguments, and **read the plan, not the timings**. Expect `paper_intents_pkey`, `paper_execs_pkey` and `paper_markouts_pkey` to appear as usable index paths. A `Seq Scan` on a near-empty table is fine; `SET enable_seqscan = off` for the check shows whether the index path exists. Note anything surprising in the commit body.

- [ ] **Step 5: Run the tests to see them pass**

Run (Postgres prefix): `cargo test -p store` and `cargo clippy -p store --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/store/src/card_io.rs crates/store/src/lib.rs crates/store/tests/card_io.rs
git commit -m "store: SWEET reads (accounts, episodes with S and tags, intents with execs, ledger, 30 s mids)"
```

---

### Task 5: `cards`: SWEET parameters, buckets and the captured value of one attempt

Pure code: no cells yet. `crates/cards/src/sweet.rs` starts here and grows in Tasks 6 and 7.

**Why a per-episode net can't go negative in normal operation.** Every sim operation that removes shares takes at most the attempt's own quantity:
- an unwind sells its own excess, capped at what is held (`crates/sim/src/trader.rs:688-693`);
- a conversion takes its own `q_eff`, capped at what is held (`crates/sim/src/account.rs:236-241`).

So another episode's rows never take *more* than their own. Shares can still leave without an `episode_id`: settlement (`account.rs:275-330`) and a restart that drops pending unwinds (`trader.rs:94-95`). Those leave a positive leftover on the attempt, which is valued at the 30 s mid (spec §4). A negative net is therefore a data anomaly: it is clamped at 0 and counted as `pooled`, never priced.

**Files:**
- Modify: `crates/cards/Cargo.toml` (rust_decimal `maths`), `crates/cards/src/lib.rs` (`pub mod sweet;`)
- Create: `crates/cards/src/sweet.rs`, `crates/cards/tests/sweet.rs`

**Interfaces:**
- Consumes: `cards::cells::{Num, Side}`, `cards::{HOUR_MS, DAY_MS}`, `arb_core::premises::sha8`.
- Produces (all `pub` in `cards::sweet`):
```rust
pub const SWEET_VERSION: &str = "1";
pub const MIN_EXEC: i64 = 30;
pub const MIN_CLUSTERS: i64 = 30;
pub const IN_FLIGHT_MS: i64 = 60_000;
pub const LATE_MS: i64 = 600_000;
pub const MARKOUT_MS: i64 = 30_000;
pub const FEW_TRADES_EFF_N: i64 = 5;
pub const P1_MAX_DAYS: i64 = 30;
pub const HEADLINE: &str = "sp-idle";
pub const FEE_BIPS: [i64; 2] = [10, 50];
pub const P1_FILTER_SKIPS: [&str; 3] = ["annualized_too_low", "no_lockup_figures", "too_long"];
pub const NOT_NET_POSITIVE: &str = "not_net_positive";
pub const DROPPED: &str = "dropped";
pub const CATEGORY_AVAILABLE: bool; // Task 1: true on GO, false on NO-GO
pub const CATEGORIES: &[(&str, &[&str])];
pub const OTHER: &str = "other";
pub const UNTAGGED: &str = "untagged";
pub const DIMS: [&str; 5] = ["all", "category", "days", "s", "hour"];
pub const DAYS_BUCKETS: [&str; 7] = ["lt1", "1-7", "7-30", "30-90", "90-365", "ge365", "unknown"];
pub const S_BUCKETS: [&str; 5] = ["2", "3", "4-5", "6-10", "gt10"];
pub const HOUR_BUCKETS: [&str; 6] = ["00-04", "04-08", "08-12", "12-16", "16-20", "20-24"];
pub const Z_TABLE: [(i64, i64); 8]; // (m ≤, z × 1000)
pub type Leg = (String, String); // (market_id, outcome)
pub fn params_text() -> String;
pub fn key_suffix() -> String; // ";sweet=1+<sha8(params_text())>"
pub fn days_bucket(days: Option<Decimal>) -> &'static str;
pub fn s_bucket(s_legs: i64) -> &'static str;
pub fn hour_bucket(opened_ms: i64) -> &'static str;
pub fn category_bucket(tags: Option<&[String]>) -> &'static str;
pub fn is_filter_skip(reason: &str) -> bool;
pub fn row_scenario(policy: &str, scenario: &str, fee_bips: Decimal) -> Option<(Side, String)>;
pub fn fee_of(scenario: &str) -> (String, i64); // "sp-idle@10" -> ("sp-idle", 10); "sp-idle" -> ("sp-idle", 0)
pub fn classify_intent(skip: Option<String>, planned_exec_ms: Option<i64>, executed: bool, now_ms: i64) -> (Option<String>, bool);
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum LineKind { Buy, Sell, Convert }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Line { pub kind: LineKind, pub leg: Option<Leg>, pub qty: Decimal, pub cash_delta: Decimal }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Attempt {
    pub episode_id: String, pub side: Side, pub scenario: String,
    pub skip: Option<String>, pub exec: Option<(i64, Decimal)>, pub pending: bool,
    pub lines: Vec<Line>, pub mid30: BTreeMap<Leg, Decimal>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)] pub struct Captured { pub x: Decimal, pub x0: Decimal, pub cost: Decimal, pub unpriced: i64, pub pooled: i64 }
pub fn captured(a: &Attempt) -> Captured;
```

- [ ] **Step 1: Declare the `maths` feature and the module**

`crates/cards/Cargo.toml`: `rust_decimal = { workspace = true, features = ["serde-with-str", "maths"] }` (the workspace already builds `maths` for `arb-core`; this makes the dependency explicit, spec nit). `crates/cards/src/lib.rs`: add `pub mod sweet;` after `pub mod stats;`.

- [ ] **Step 2: Write the failing tests**

`crates/cards/tests/sweet.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::cells::Side;
use cards::sweet::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;

const H: i64 = 3_600_000;

fn leg(m: &str, o: &str) -> Option<Leg> {
    Some((m.to_string(), o.to_string()))
}
fn buy(m: &str, o: &str, q: Decimal, cash: Decimal) -> Line {
    Line { kind: LineKind::Buy, leg: leg(m, o), qty: q, cash_delta: cash }
}
fn sell(m: &str, o: &str, q: Decimal, cash: Decimal) -> Line {
    Line { kind: LineKind::Sell, leg: leg(m, o), qty: q, cash_delta: cash }
}
fn convert(q: Decimal, cash: Decimal) -> Line {
    Line { kind: LineKind::Convert, leg: None, qty: q, cash_delta: cash }
}
fn attempt(side: Side, q_eff: Decimal, lines: Vec<Line>, mids: &[(&str, &str, Decimal)]) -> Attempt {
    Attempt {
        episode_id: "e1".into(),
        side,
        scenario: "sp-idle".into(),
        skip: None,
        exec: Some((0, q_eff)),
        pending: false,
        lines,
        mid30: mids.iter().map(|(m, o, v)| ((m.to_string(), o.to_string()), *v)).collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn days_buckets_close_on_the_right_at_p1s_thirty_days() {
    assert_eq!(days_bucket(None), "unknown");
    assert_eq!(days_bucket(Some(dec!(-0.5))), "unknown");
    assert_eq!(days_bucket(Some(dec!(0))), "lt1");
    assert_eq!(days_bucket(Some(dec!(0.999))), "lt1");
    assert_eq!(days_bucket(Some(dec!(1))), "1-7");
    assert_eq!(days_bucket(Some(dec!(7))), "7-30");
    assert_eq!(days_bucket(Some(dec!(30))), "7-30", "P1 takes days <= 30 (policy.rs:258)");
    assert_eq!(days_bucket(Some(dec!(30.0001))), "30-90");
    assert_eq!(days_bucket(Some(dec!(90))), "90-365");
    assert_eq!(days_bucket(Some(dec!(365))), "ge365");
}

#[test]
fn s_and_hour_buckets_are_half_open() {
    assert_eq!([1, 2, 3, 4, 5, 6, 10, 11].map(s_bucket), ["2", "2", "3", "4-5", "4-5", "6-10", "6-10", "gt10"]);
    let day = 20_000 * 24 * H;
    assert_eq!(hour_bucket(day), "00-04");
    assert_eq!(hour_bucket(day + 4 * H - 1), "00-04");
    assert_eq!(hour_bucket(day + 4 * H), "04-08");
    assert_eq!(hour_bucket(day + 23 * H + 59 * 60_000), "20-24");
    assert_eq!(hour_bucket(-1), "20-24", "rem_euclid: before the epoch still lands in a day");
}

#[test]
fn category_takes_the_first_bucket_in_order_then_other_then_untagged() {
    let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(category_bucket(None), UNTAGGED);
    assert_eq!(category_bucket(Some(&t(&[]))), UNTAGGED);
    assert_eq!(category_bucket(Some(&t(&["featured"]))), OTHER);
    assert_eq!(category_bucket(Some(&t(&["featured", "nba", "politics"]))), "politics", "bucket order wins, not tag order");
    assert_eq!(category_bucket(Some(&t(&["nba"]))), "sports");
    // every slug belongs to exactly one bucket
    let mut seen = std::collections::BTreeSet::new();
    for (_, slugs) in CATEGORIES {
        for s in *slugs {
            assert!(seen.insert(*s), "{s} is in two buckets");
            assert_eq!(*s, s.to_lowercase(), "{s} must be lower-case");
        }
    }
}

#[test]
fn accounts_map_to_row_scenarios() {
    assert_eq!(row_scenario("P0-converter", "sp-idle", dec!(0)), Some((Side::No, "sp-idle".into())));
    assert_eq!(row_scenario("P0-converter", "sp-idle", dec!(10)), Some((Side::No, "sp-idle@10".into())));
    assert_eq!(row_scenario("P0-converter", "sp-idle", dec!(50)), Some((Side::No, "sp-idle@50".into())));
    assert_eq!(row_scenario("P0-converter", "colo-idle", dec!(10)), None, "fee rows only at the headline scenario");
    assert_eq!(row_scenario("P1-settlement", "sp-burst", dec!(0)), Some((Side::Yes, "sp-burst".into())));
    assert_eq!(row_scenario("P1-settlement", "sp-idle", dec!(10)), None);
    assert_eq!(row_scenario("P3-naive", "sp-idle", dec!(0)), None);
    assert_eq!(row_scenario("P2-combo", "sp-idle", dec!(0)), None);
    assert_eq!(fee_of("sp-idle@10"), ("sp-idle".to_string(), 10));
    assert_eq!(fee_of("colo-idle"), ("colo-idle".to_string(), 0));
}

#[test]
fn intents_classify_as_skipped_pending_executed_or_dropped() {
    let now = 10 * H;
    assert_eq!(classify_intent(Some("no_cash".into()), None, false, now), (Some("no_cash".into()), false));
    assert_eq!(classify_intent(None, Some(now - 1_000), true, now), (None, false));
    assert_eq!(classify_intent(None, Some(now - 1_000), false, now), (None, true), "not executed yet");
    assert_eq!(classify_intent(None, Some(now - IN_FLIGHT_MS - 1), false, now), (Some(DROPPED.into()), false), "a restart dropped it");
    assert_eq!(classify_intent(None, None, false, now), (Some(DROPPED.into()), false));
}

#[test]
fn a_no_attempt_converts_and_realizes_its_edge() {
    let a = attempt(Side::No, dec!(10), vec![
        buy("m0", "no", dec!(10), dec!(-4.00)),
        buy("m1", "no", dec!(10), dec!(-5.10)),
        convert(dec!(10), dec!(10.00)),
    ], &[]);
    assert_eq!(captured(&a), Captured { x: dec!(0.90), x0: dec!(0.90), cost: dec!(9.10), unpriced: 0, pooled: 0 });
}

#[test]
fn a_yes_attempt_counts_its_complete_sets_at_one_each() {
    let a = attempt(Side::Yes, dec!(10), vec![
        buy("a", "yes", dec!(10), dec!(-3.00)),
        buy("b", "yes", dec!(10), dec!(-3.50)),
        buy("c", "yes", dec!(10), dec!(-3.40)),
    ], &[]);
    let c = captured(&a);
    assert_eq!((c.x, c.cost), (dec!(0.10), dec!(9.90)));
}

#[test]
fn a_missed_attempt_with_a_partial_buy_and_unwind_is_negative() {
    // q_eff = 0: one leg bought 10, the other none; 6 unwound; 4 left, valued at the 30 s mid.
    let a = attempt(Side::No, dec!(0), vec![
        buy("m0", "no", dec!(10), dec!(-4.00)),
        sell("m0", "no", dec!(6), dec!(2.30)),
    ], &[("m0", "no", dec!(0.39))]);
    let c = captured(&a);
    assert_eq!((c.x, c.x0, c.cost, c.unpriced), (dec!(-0.14), dec!(-1.70), dec!(4.00), 0));
}

#[test]
fn a_yes_leftover_beyond_the_sets_is_valued_at_its_mid_or_zero_when_unpriced() {
    let lines = vec![
        buy("a", "yes", dec!(12), dec!(-3.60)),
        buy("b", "yes", dec!(10), dec!(-3.50)),
        buy("c", "yes", dec!(10), dec!(-3.40)),
    ];
    let priced = captured(&attempt(Side::Yes, dec!(10), lines.clone(), &[("a", "yes", dec!(0.30))]));
    assert_eq!((priced.x, priced.x0, priced.unpriced), (dec!(0.10), dec!(-0.50), 0));
    let unpriced = captured(&attempt(Side::Yes, dec!(10), lines, &[]));
    assert_eq!((unpriced.x, unpriced.x0, unpriced.unpriced), (dec!(-0.50), dec!(-0.50), 1));
}

#[test]
fn pooled_shares_follow_each_rows_episode_and_never_count_twice() {
    // two NO episodes on the same legs in one account; the second's conversion was capped (held
    // short after a settlement took shares without an episode id): its leftover stays on it.
    let e1 = attempt(Side::No, dec!(10), vec![
        buy("m0", "no", dec!(10), dec!(-4.00)),
        buy("m1", "no", dec!(10), dec!(-5.10)),
        convert(dec!(10), dec!(10.00)),
    ], &[]);
    let e2 = attempt(Side::No, dec!(5), vec![
        buy("m0", "no", dec!(5), dec!(-2.00)),
        buy("m1", "no", dec!(5), dec!(-2.50)),
        convert(dec!(2), dec!(2.00)),
    ], &[("m0", "no", dec!(0.40)), ("m1", "no", dec!(0.55))]);
    let (c1, c2) = (captured(&e1), captured(&e2));
    assert_eq!(c1.x, dec!(0.90));
    assert_eq!(c2.x, dec!(0.35)); // -4.50 + 2.00 + 3 x 0.40 + 3 x 0.55
    // additive: the two attempts together equal one attempt holding every row (NO side is linear)
    let mut both = e2.clone();
    both.lines = e1.lines.iter().chain(&e2.lines).cloned().collect();
    assert_eq!(captured(&both).x, c1.x + c2.x, "attribution by episode is additive on the NO side");
    // a row that sells more than the attempt bought (a data anomaly) is clamped, counted, never priced
    let odd = attempt(Side::No, dec!(5), vec![
        buy("m0", "no", dec!(5), dec!(-2.00)),
        sell("m0", "no", dec!(8), dec!(3.20)),
    ], &[("m0", "no", dec!(0.40))]);
    let c = captured(&odd);
    assert_eq!((c.x, c.pooled, c.unpriced), (dec!(1.20), 1, 0));
}

#[test]
fn the_key_suffix_is_a_semicolon_segment_over_every_parameter() {
    let s = key_suffix();
    assert!(s.starts_with(";sweet=1+"), "{s}");
    assert_eq!(s.len(), ";sweet=1+".len() + 8);
    let p = params_text();
    for needle in ["min_exec=30", "min_clusters=30", "in_flight_ms=60000", "late_ms=600000", "markout_ms=30000",
                   "few_eff_n=5", "p1_max_days=30", "1:1960", "500:3891", "days=lt1,1-7,7-30,30-90,90-365,ge365,unknown"] {
        assert!(p.contains(needle), "params_text lacks {needle}: {p}");
    }
    assert_eq!(key_suffix(), key_suffix(), "deterministic");
}
```
Arithmetic check for `pooled_shares_follow…`: merged, the buys cost 13.60, the two conversions return 12.00 and the leftovers (3 of each leg) are worth 1.20 + 1.65, so x = 1.25 = 0.90 + 0.35. Each conversion only subtracts its own quantity from the legs its attempt bought, so attribution is exactly additive on the NO side.

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p cards --test sweet`
Expected: FAIL to compile (`cards::sweet` missing).

- [ ] **Step 4: Implement**

`crates/cards/src/sweet.rs`:
```rust
//! Sweet spots (Phase 5d spec `2026-09-27-phase5d-sweet-spots-design.md`): the edge each paper
//! attempt actually captured, after fees and lockup, sliced one dimension at a time. Pure: analyze
//! reads the rows (store) and calls in. No IO, no clock, no floats.
//!
//! Parameters are versioned card-side, not as a premise area (spec §6, review B1): every constant
//! below enters `params_text()`, whose sha8 is the premise-key suffix, so an edit starts a new key
//! and 5a's catch-up rebuilds the card from stored history.
use crate::cells::Side;
use crate::{DAY_MS, HOUR_MS};
use arb_core::premises::sha8;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const SWEET_VERSION: &str = "1";
/// Spec §7.1: a verdict needs this many executed attempts and this many event-hour clusters.
pub const MIN_EXEC: i64 = 30;
pub const MIN_CLUSTERS: i64 = 30;
/// Spec §4: executed less than this before the tick: conversion, unwinds and the 30 s markout may
/// not be written yet, so the attempt waits for a later tick.
pub const IN_FLIGHT_MS: i64 = 60_000;
/// Spec §6: every tick rebuilds every hour touched by [last tick − LATE_MS, now].
pub const LATE_MS: i64 = 600_000;
/// Spec §4: leftovers are valued at this markout's mid.
pub const MARKOUT_MS: i64 = 30_000;
/// Spec §7.2: below this effective n, "few trades carry this row".
pub const FEW_TRADES_EFF_N: i64 = 5;
/// P1's `max_days_to_end` in config/policies.toml (the days bucket closes here; analyze checks it).
pub const P1_MAX_DAYS: i64 = 30;
/// The headline scenario (the same as Edge capture's).
pub const HEADLINE: &str = "sp-idle";
/// The fee-sensitivity P0 accounts at the headline scenario.
pub const FEE_BIPS: [i64; 2] = [10, 50];
/// P1's three filter skips (policy.rs:255-263): a YES row whose every intent is one of these was
/// never tested (spec §7.4).
pub const P1_FILTER_SKIPS: [&str; 3] = ["annualized_too_low", "no_lockup_figures", "too_long"];
/// Not an attempt: the episode was not net-positive for the policy (spec §4, review S8).
pub const NOT_NET_POSITIVE: &str = "not_net_positive";
/// An intent with no skip whose planned execution passed without an exec row (a restart drops
/// pending intents, trader.rs:94-95): counted as a skipped attempt with this reason.
pub const DROPPED: &str = "dropped";
/// Task 1's go/no-go. false: the Category dimension is left out everywhere.
pub const CATEGORY_AVAILABLE: bool = true;
/// Spec §3: ordered buckets; the first bucket holding one of an event's tag slugs wins. From the
/// Task 1 sample (see the plan's Results).
pub const CATEGORIES: &[(&str, &[&str])] = &[
    ("politics", &["politics", "us-politics", "elections", "us-election", "trump", "congress", "senate"]),
    ("sports", &["sports", "nba", "nfl", "mlb", "nhl", "soccer", "football", "tennis", "golf", "mma", "ufc", "f1", "boxing", "esports", "cricket", "basketball"]),
    ("crypto", &["crypto", "bitcoin", "ethereum", "solana", "crypto-prices", "xrp", "memecoins"]),
    ("economy", &["economy", "finance", "business", "fed", "fed-rates", "inflation", "stocks", "earnings", "commodities"]),
    ("world", &["world", "geopolitics", "international-elections", "middle-east", "ukraine", "china", "europe"]),
    ("culture", &["pop-culture", "culture", "entertainment", "awards", "music", "movies", "tv", "celebrities", "oscars", "grammys"]),
    ("tech", &["tech", "technology", "ai", "science", "space"]),
];
pub const OTHER: &str = "other";
pub const UNTAGGED: &str = "untagged";
pub const DIMS: [&str; 5] = ["all", "category", "days", "s", "hour"];
pub const DAYS_BUCKETS: [&str; 7] = ["lt1", "1-7", "7-30", "30-90", "90-365", "ge365", "unknown"];
pub const S_BUCKETS: [&str; 5] = ["2", "3", "4-5", "6-10", "gt10"];
pub const HOUR_BUCKETS: [&str; 6] = ["00-04", "04-08", "08-12", "12-16", "16-20", "20-24"];
/// Spec §7.2: two-sided 5% family error over m rows, Bonferroni; the next larger m is used. z is
/// stored × 1000 (Decimal::new(z, 3)); the rows never exceed 500 (28 buckets × 9 row scenarios).
pub const Z_TABLE: [(i64, i64); 8] = [
    (1, 1960), (5, 2576), (10, 2807), (20, 3023), (50, 3291), (100, 3481), (200, 3662), (500, 3891),
];

/// (market_id, outcome).
pub type Leg = (String, String);

/// Every parameter, in one canonical text (the premise-key suffix hashes it; the card hash
/// includes it through `Parts.params`).
pub fn params_text() -> String {
    let mut s = format!(
        "v={SWEET_VERSION};min_exec={MIN_EXEC};min_clusters={MIN_CLUSTERS};in_flight_ms={IN_FLIGHT_MS};\
         late_ms={LATE_MS};markout_ms={MARKOUT_MS};few_eff_n={FEW_TRADES_EFF_N};p1_max_days={P1_MAX_DAYS};\
         headline={HEADLINE};category={CATEGORY_AVAILABLE};days={};s={};hour={};z=",
        DAYS_BUCKETS.join(","),
        S_BUCKETS.join(","),
        HOUR_BUCKETS.join(","),
    );
    for (m, z) in Z_TABLE {
        let _ = write!(s, "{m}:{z},");
    }
    s.push_str(";cats=");
    for (b, slugs) in CATEGORIES {
        let _ = write!(s, "{b}:{}|", slugs.join(","));
    }
    s
}

/// Appended to the run-derived premise key: a `;`-separated segment like an area's, so 5a's key
/// parsing (`edge::host_from_key`) is unaffected.
pub fn key_suffix() -> String {
    format!(";sweet={SWEET_VERSION}+{}", sha8(&params_text()))
}

pub fn days_bucket(days: Option<Decimal>) -> &'static str {
    let Some(d) = days else { return "unknown" };
    if d < Decimal::ZERO {
        "unknown"
    } else if d < Decimal::ONE {
        "lt1"
    } else if d < Decimal::from(7) {
        "1-7"
    } else if d <= Decimal::from(P1_MAX_DAYS) {
        "7-30"
    } else if d < Decimal::from(90) {
        "30-90"
    } else if d < Decimal::from(365) {
        "90-365"
    } else {
        "ge365"
    }
}

pub fn s_bucket(s_legs: i64) -> &'static str {
    match s_legs {
        i64::MIN..=2 => "2",
        3 => "3",
        4..=5 => "4-5",
        6..=10 => "6-10",
        _ => "gt10",
    }
}

pub fn hour_bucket(opened_ms: i64) -> &'static str {
    let block = opened_ms.rem_euclid(DAY_MS) / (4 * HOUR_MS);
    usize::try_from(block)
        .ok()
        .and_then(|i| HOUR_BUCKETS.get(i))
        .copied()
        .unwrap_or("20-24")
}

pub fn category_bucket(tags: Option<&[String]>) -> &'static str {
    let tags = match tags {
        Some(t) if !t.is_empty() => t,
        _ => return UNTAGGED,
    };
    for (bucket, slugs) in CATEGORIES {
        if tags.iter().any(|t| slugs.contains(&t.as_str())) {
            return bucket;
        }
    }
    OTHER
}

pub fn is_filter_skip(reason: &str) -> bool {
    P1_FILTER_SKIPS.contains(&reason)
}

/// The row scenario of a paper account (spec §4): P0 at fee 0 → NO rows under its scenario; P0 at
/// fee 10 or 50 → NO rows under "sp-idle@10" / "sp-idle@50", headline scenario only; P1 at fee 0 →
/// YES rows; anything else is not read.
pub fn row_scenario(policy: &str, scenario: &str, fee_bips: Decimal) -> Option<(Side, String)> {
    let fee = fee_bips.normalize();
    match policy {
        "P0-converter" if fee.is_zero() => Some((Side::No, scenario.to_string())),
        "P0-converter" if scenario == HEADLINE => FEE_BIPS
            .iter()
            .find(|f| Decimal::from(**f) == fee)
            .map(|f| (Side::No, format!("{HEADLINE}@{f}"))),
        "P1-settlement" if fee.is_zero() => Some((Side::Yes, scenario.to_string())),
        _ => None,
    }
}

/// "sp-idle@10" → ("sp-idle", 10); a plain scenario → (it, 0).
pub fn fee_of(scenario: &str) -> (String, i64) {
    match scenario.split_once('@') {
        Some((s, f)) => (s.to_string(), f.parse().unwrap_or(0)),
        None => (scenario.to_string(), 0),
    }
}

/// An intent's state for SWEET: (skip reason, pending). An intent with no skip and no exec row is
/// pending while its planned execution is under IN_FLIGHT_MS old, and dropped after that (a
/// restart drops pending intents).
pub fn classify_intent(
    skip: Option<String>,
    planned_exec_ms: Option<i64>,
    executed: bool,
    now_ms: i64,
) -> (Option<String>, bool) {
    if skip.is_some() || executed {
        return (skip, false);
    }
    match planned_exec_ms {
        Some(t) if t > now_ms - IN_FLIGHT_MS => (None, true),
        _ => (Some(DROPPED.to_string()), false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Buy,
    Sell,
    Convert,
}

/// One buy / sell / convert ledger row carrying the attempt's episode id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// None for a conversion (it takes `qty` from every leg the attempt bought).
    pub leg: Option<Leg>,
    pub qty: Decimal,
    pub cash_delta: Decimal,
}

/// One (paper account, episode) pair with an intent (spec §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub episode_id: String,
    pub side: Side,
    /// The row scenario (`row_scenario`).
    pub scenario: String,
    /// The skip reason (`classify_intent`), None when executed or pending.
    pub skip: Option<String>,
    /// (exec_ms, q_eff) once executed.
    pub exec: Option<(i64, Decimal)>,
    pub pending: bool,
    pub lines: Vec<Line>,
    /// The MARKOUT_MS markout mid per leg (absent: not priced).
    pub mid30: BTreeMap<Leg, Decimal>,
}

/// The captured value of one executed attempt (spec §4). `x` values leftovers at the 30 s mid,
/// `x0` at zero; `cost` is what the buys cost, fees included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Captured {
    pub x: Decimal,
    pub x0: Decimal,
    pub cost: Decimal,
    /// Leftover legs with no 30 s mid (valued at 0 in both).
    pub unpriced: i64,
    /// Legs whose net went negative (clamped at 0, never priced).
    pub pooled: i64,
}

pub fn captured(a: &Attempt) -> Captured {
    let mut out = Captured::default();
    let mut cash = Decimal::ZERO;
    let mut converted = Decimal::ZERO;
    let mut net: BTreeMap<&Leg, Decimal> = BTreeMap::new();
    for l in &a.lines {
        cash += l.cash_delta;
        match (l.kind, &l.leg) {
            (LineKind::Buy, Some(leg)) => {
                out.cost -= l.cash_delta;
                *net.entry(leg).or_default() += l.qty;
            }
            (LineKind::Buy, None) => out.cost -= l.cash_delta,
            (LineKind::Sell, Some(leg)) => *net.entry(leg).or_default() -= l.qty,
            (LineKind::Sell, None) => {}
            (LineKind::Convert, _) => converted += l.qty,
        }
    }
    // a conversion takes its quantity from every S leg: the legs this attempt bought
    if !converted.is_zero() {
        for q in net.values_mut() {
            *q -= converted;
        }
    }
    for q in net.values_mut() {
        if *q < Decimal::ZERO {
            *q = Decimal::ZERO;
            out.pooled += 1;
        }
    }
    let sets = match a.side {
        Side::Yes => net.values().min().copied().unwrap_or(Decimal::ZERO),
        Side::No => Decimal::ZERO,
    };
    let mut leftover = Decimal::ZERO;
    for (leg, q) in &net {
        let left = *q - sets;
        if left <= Decimal::ZERO {
            continue;
        }
        match a.mid30.get(*leg) {
            Some(m) => leftover += left * *m,
            None => out.unpriced += 1,
        }
    }
    out.x0 = cash + sets;
    out.x = out.x0 + leftover;
    out
}
```
On NO-GO, set `CATEGORY_AVAILABLE = false` and keep `CATEGORIES` as written (it is unused but harmless, and `params_text` records the flag).

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p cards --test sweet` then `cargo test -p cards` (including `no_floats`) and `cargo clippy -p cards --all-targets -- -D warnings`.
Expected: PASS. If clippy flags `Decimal` multiplication as `arithmetic_side_effects` elsewhere in the crate's style, follow the crate's existing pattern (it uses plain `+ - *` on Decimal in `edge.rs`).

- [ ] **Step 6: Commit**

```bash
git add crates/cards/Cargo.toml crates/cards/src/lib.rs crates/cards/src/sweet.rs crates/cards/tests/sweet.rs
git commit -m "cards: SWEET parameters, buckets, intent states and the captured value of one attempt"
```

---

### Task 6: `cards`: the SWEET cell (additive, clusters) and the CellData hooks

**Files:**
- Modify: `crates/cards/src/cells.rs:129-173` (HourInput, CellData), `crates/cards/src/sweet.rs`
- Modify (new field on every `HourInput` literal): `crates/cards/tests/cells.rs:36,235`, `crates/cards/tests/edge.rs:38`, `crates/cards/tests/merge_property.rs:71`, `crates/cards/tests/bench_tick.rs:106,141`, `bins/analyze/src/tick.rs:632`
- Test: `crates/cards/tests/sweet.rs`, `crates/cards/tests/sweet_property.rs` (new)

**Interfaces:**
- Consumes: Task 5.
- Produces:
```rust
// cards::sweet
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweetEp {
    pub id: String, pub event_id: String, pub side: Side, pub opened_ms: i64,
    pub pnl_net: Decimal, pub budget100: Option<Decimal>, pub flagged: bool,
    pub days: Option<Decimal>, pub annualized: Option<Decimal>, pub peak_cost: Decimal,
    pub s_legs: i64, pub tags: Option<Vec<String>>,
}
impl SweetEp { pub fn buckets(&self, fee_row: bool) -> Vec<(&'static str, &'static str)> }
#[derive(Debug, Clone, Copy, Default)]
pub struct SweetInput<'a> { pub now_ms: i64, pub eps: &'a [SweetEp], pub attempts: &'a [Attempt], pub scenarios: &'a [(Side, String)] }
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acc { /* every field additive, see Step 3 */ }
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweetCell { pub rows: BTreeMap<String, Acc> }
pub fn row_key(side: Side, scenario: &str, dim: &str, bucket: &str) -> String; // "no|sp-idle|days|1-7"
pub fn parse_key(k: &str) -> Option<(Side, String, String, String)>;
impl CellData for SweetCell { FAMILY = "sweet"; AREAS = EDGE's seven; SWEET_INPUTS = true; LATE_MS = LATE_MS; key_suffix() = sweet::key_suffix(); type Max = (); }
// cards::cells
pub struct HourInput<'a> { …existing…, pub sweet: SweetInput<'a> }
pub trait CellData { …existing…; const SWEET_INPUTS: bool = false; const LATE_MS: i64 = 0; fn key_suffix() -> String { String::new() } }
```

- [ ] **Step 1: Write the failing tests**

Append to `crates/cards/tests/sweet.rs`:
```rust
use cards::cells::{CellData, HourInput};

const T: i64 = 20_000 * 24 * H; // a UTC midnight

fn sep(id: &str, event: &str, side: Side, opened: i64) -> SweetEp {
    SweetEp {
        id: id.into(), event_id: event.into(), side, opened_ms: opened, pnl_net: dec!(0.41),
        budget100: Some(dec!(0.33)), flagged: false, days: Some(dec!(3)), annualized: Some(dec!(0.5)),
        peak_cost: dec!(99), s_legs: 2, tags: Some(vec!["nba".into()]),
    }
}
fn no_att(ep: &str, scen: &str, x_cash: Decimal) -> Attempt {
    // buys 10 + 10 NO for 9.10, converts 10 for 9.10 + x_cash
    let mut a = attempt(Side::No, dec!(10), vec![
        buy("m0", "no", dec!(10), dec!(-4.00)),
        buy("m1", "no", dec!(10), dec!(-5.10)),
        convert(dec!(10), dec!(9.10) + x_cash),
    ], &[]);
    a.episode_id = ep.into();
    a.scenario = scen.into();
    a
}
fn build(now: i64, eps: &[SweetEp], atts: &[Attempt], scen: &[(Side, String)]) -> SweetCell {
    SweetCell::build(&HourInput {
        hour_ms: T, episodes: &[], latency_ms: &[], equity_deltas: &[],
        sweet: SweetInput { now_ms: now, eps, attempts: atts, scenarios: scen },
    })
}
fn scen() -> Vec<(Side, String)> {
    vec![(Side::No, "sp-idle".into()), (Side::No, "sp-idle@10".into()), (Side::Yes, "sp-idle".into())]
}

#[test]
fn a_cell_counts_the_table_the_attempts_and_the_clusters_per_row() {
    let eps = vec![sep("e1", "ev1", Side::No, T + 60_000), sep("e2", "ev1", Side::No, T + 120_000), sep("e3", "ev2", Side::No, T + 180_000)];
    let atts = vec![no_att("e1", "sp-idle", dec!(1)), no_att("e2", "sp-idle", dec!(-0.5)), no_att("e3", "sp-idle", dec!(2))];
    let c = build(T + 2 * H, &eps, &atts, &scen());
    let all = &c.rows[&row_key(Side::No, "sp-idle", "all", "all")];
    assert_eq!((all.eps, all.attempts, all.filled, all.missed), (3, 3, 3, 0));
    assert_eq!(all.on_table.0, dec!(0.99));
    assert_eq!(all.sx.0, dec!(2.5));
    assert_eq!(all.sx2.0, dec!(5.25));  // 1 + 0.25 + 4
    assert_eq!(all.sabs.0, dec!(3.5));
    assert_eq!(all.cost.0, dec!(27.30));
    // clusters: ev1 (n=2, c=0.5) and ev2 (n=1, c=2)
    assert_eq!((all.clusters, all.sn2), (2, 5));
    assert_eq!(all.sc2.0, dec!(4.25));  // 0.25 + 4
    assert_eq!(all.scn.0, dec!(3));     // 0.5*2 + 2*1
    // the fee scenario has only the all row, on the table but with no attempts
    let fee = &c.rows[&row_key(Side::No, "sp-idle@10", "all", "all")];
    assert_eq!((fee.eps, fee.attempts), (3, 0));
    assert!(!c.rows.contains_key(&row_key(Side::No, "sp-idle@10", "days", "1-7")));
    // other dimensions carry the same attempt
    assert_eq!(c.rows[&row_key(Side::No, "sp-idle", "days", "1-7")].attempts, 3);
    assert_eq!(c.rows[&row_key(Side::No, "sp-idle", "s", "2")].attempts, 3);
    assert_eq!(c.rows[&row_key(Side::No, "sp-idle", "hour", "00-04")].attempts, 3);
    assert_eq!(c.rows.contains_key(&row_key(Side::No, "sp-idle", "category", "sports")), CATEGORY_AVAILABLE);
    // YES scenario: no YES episode, so no YES row at all
    assert!(c.rows.keys().all(|k| !k.starts_with("yes|")));
}

#[test]
fn in_flight_pending_non_positive_and_flagged_never_become_attempts() {
    let mut flagged = sep("e4", "ev4", Side::No, T + 1);
    flagged.flagged = true;
    let mut losing = sep("e5", "ev5", Side::No, T + 2);
    losing.pnl_net = dec!(-0.01);
    let eps = vec![sep("e1", "ev1", Side::No, T + 1), sep("e2", "ev2", Side::No, T + 1), flagged, losing];
    let now = T + H;
    let mut in_flight = no_att("e1", "sp-idle", dec!(1));
    in_flight.exec = Some((now - 10_000, dec!(10)));
    let mut pending = no_att("e2", "sp-idle", dec!(1));
    pending.exec = None;
    pending.pending = true;
    let atts = vec![in_flight, pending, no_att("e4", "sp-idle", dec!(1)), no_att("e5", "sp-idle", dec!(1))];
    let c = build(now, &eps, &atts, &scen());
    let all = &c.rows[&row_key(Side::No, "sp-idle", "all", "all")];
    assert_eq!((all.eps, all.attempts, all.flagged), (2, 0, 1));
}

#[test]
fn p1_filter_skips_count_and_leave_the_eligible_table() {
    let eps = vec![sep("y1", "ev1", Side::Yes, T + 1), sep("y2", "ev2", Side::Yes, T + 2)];
    let mut skip = attempt(Side::Yes, dec!(0), vec![], &[]);
    skip.episode_id = "y1".into();
    skip.exec = None;
    skip.skip = Some("too_long".into());
    let mut cash = skip.clone();
    cash.episode_id = "y2".into();
    cash.skip = Some("no_cash".into());
    let c = build(T + H, &eps, &[skip, cash], &scen());
    let all = &c.rows[&row_key(Side::Yes, "sp-idle", "all", "all")];
    assert_eq!((all.attempts, all.filter_skips), (2, 1));
    assert_eq!(all.skipped.get("too_long"), Some(&1));
    assert_eq!(all.skipped.get("no_cash"), Some(&1));
    assert_eq!((all.on_table.0, all.on_table_elig.0), (dec!(0.66), dec!(0.33)));
    assert_eq!((all.up_w.0, all.up_c.0), (dec!(99), dec!(198))); // 0.5 x 99 twice
}

#[test]
fn add_then_sub_is_the_identity_and_empty_rows_disappear() {
    let eps = vec![sep("e1", "ev1", Side::No, T + 1)];
    let c = build(T + H, &eps, &[no_att("e1", "sp-idle", dec!(1))], &scen());
    let mut sum = SweetCell::default();
    sum.add(&c);
    sum.add(&c);
    sum.sub(&c);
    assert_eq!(sum, c);
    sum.sub(&c);
    assert_eq!(sum, SweetCell::default());
    assert_eq!(SweetCell::from_json(&c.to_json().unwrap()).unwrap(), c);
}

#[test]
fn sweet_keys_its_family_with_the_suffix_and_rebuilds_late_hours() {
    assert_eq!(SweetCell::FAMILY, "sweet");
    assert!(SweetCell::SWEET_INPUTS);
    assert_eq!(SweetCell::LATE_MS, LATE_MS);
    assert_eq!(SweetCell::key_suffix(), key_suffix());
    assert_eq!(cards::edge::EdgeCell::key_suffix(), "");
    assert_eq!(cards::cells::G1Cell::key_suffix(), "");
    assert_eq!(cards::edge::EdgeCell::LATE_MS, 0);
}
```
Create `crates/cards/tests/sweet_property.rs`. It holds a proptest in the style of `crates/cards/tests/merge_property.rs`: random attempts over up to 6 hours and 4 events, with random captured cash (`-2.00..=2.00` in cents) and a random side-scenario pairing drawn from `scen()`.
- **Merge:** assert that merging the per-hour cells equals a direct computation over the whole range, done in the test by grouping attempts by row key and clusters by `(event_id, hour_of(opened_ms))`. Compare `attempts`, `filled`, `missed`, `sx`, `sx2`, `sabs`, `clusters`, `sc2`, `scn` and `sn2` for every row key.
- **Expiry:** subtracting one hour's cell from the merged sum equals the direct computation without that hour.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p cards --test sweet --test sweet_property`
Expected: FAIL to compile (`SweetEp`, `SweetCell`, `HourInput.sweet` missing).

- [ ] **Step 3: Implement**

In `crates/cards/src/cells.rs`, add to `HourInput`:
```rust
    /// SWEET: the hour's attempts and their episodes (Phase 5d). Default (empty) for other cards.
    pub sweet: crate::sweet::SweetInput<'a>,
```
and to `CellData` (after `EDGE_INPUTS`):
```rust
    /// Needs the SWEET inputs (Phase 5d): SWEET only.
    const SWEET_INPUTS: bool = false;
    /// analyze rebuilds every hour touched by [last tick − LATE_MS, now] (0: no such rule).
    const LATE_MS: i64 = 0;
    /// Appended to the run-derived premise key (card-side parameters, Phase 5d spec §6). Empty
    /// for every 5a family, so their keys never change.
    fn key_suffix() -> String {
        String::new()
    }
```
Add `sweet: SweetInput::default(),` (with `use cards::sweet::SweetInput;` where needed) to every `HourInput { … }` literal listed under **Files**; in `bins/analyze/src/tick.rs:632` write `sweet: SweetInput::default(),` for now (Task 8 fills it).

Append to `crates/cards/src/sweet.rs`:
```rust
use crate::cells::{CellData, HourInput, Num};
use crate::hour_of;
use serde::{Deserialize, Serialize};

/// One episode as SWEET sees it (analyze maps `store::SweetEpisodeRow` into this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweetEp {
    pub id: String,
    pub event_id: String,
    pub side: Side,
    pub opened_ms: i64,
    pub pnl_net: Decimal,
    pub budget100: Option<Decimal>,
    /// engine_suspect or stale_transport: excluded and counted (spec §4).
    pub flagged: bool,
    pub days: Option<Decimal>,
    pub annualized: Option<Decimal>,
    pub peak_cost: Decimal,
    pub s_legs: i64,
    pub tags: Option<Vec<String>>,
}

impl SweetEp {
    pub fn positive(&self) -> bool {
        self.pnl_net > Decimal::ZERO
    }

    /// (dimension, bucket) rows this episode lands in; a fee row has only "all".
    pub fn buckets(&self, fee_row: bool) -> Vec<(&'static str, &'static str)> {
        let mut out = vec![("all", "all")];
        if fee_row {
            return out;
        }
        if CATEGORY_AVAILABLE {
            out.push(("category", category_bucket(self.tags.as_deref())));
        }
        out.push(("days", days_bucket(self.days)));
        out.push(("s", s_bucket(self.s_legs)));
        out.push(("hour", hour_bucket(self.opened_ms)));
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SweetInput<'a> {
    pub now_ms: i64,
    /// Episodes of the build range; the cell keeps those opened in its hour.
    pub eps: &'a [SweetEp],
    pub attempts: &'a [Attempt],
    /// Every (side, row scenario) with an account: an episode is on the table in each, attempt or not.
    pub scenarios: &'a [(Side, String)],
}

/// One row's additive sums (spec §6). Every field merges by addition, so windows are sums of cells
/// and an expiring hour is subtracted exactly. Counts are i64; money is Num (a JSON string).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Acc {
    /// Net-positive, unflagged episodes with a $100-capped P&L, and their Σ (edge on the table).
    pub eps: i64,
    pub on_table: Num,
    /// On the table minus the episodes this row's P1 account filter-skipped (NO rows: the same).
    pub on_table_elig: Num,
    /// Net-positive but flagged episodes: excluded, counted.
    pub flagged: i64,
    /// YES upper bound (spec §7.5): Σ annualized × peak cost and Σ peak cost, over those episodes.
    pub up_w: Num,
    pub up_c: Num,
    pub attempts: i64,
    pub skipped: BTreeMap<String, i64>,
    pub filter_skips: i64,
    pub missed: i64,
    pub filled: i64,
    pub unpriced: i64,
    pub pooled: i64,
    /// Executed YES attempts whose episode has no days_to_end (annualization is then withheld).
    pub nodays: i64,
    pub sx: Num,
    pub sx2: Num,
    pub sabs: Num,
    pub sx0: Num,
    pub cost: Num,
    pub cost_days: Num,
    /// Clusters: (event, hour) groups with at least one executed attempt, and Σc², Σc·n, Σn².
    pub clusters: i64,
    pub sc2: Num,
    pub scn: Num,
    pub sn2: i64,
}

impl Acc {
    fn apply(&mut self, o: &Acc, sign: i64) {
        let s = Decimal::from(sign);
        self.eps += sign * o.eps;
        self.on_table.0 += s * o.on_table.0;
        self.on_table_elig.0 += s * o.on_table_elig.0;
        self.flagged += sign * o.flagged;
        self.up_w.0 += s * o.up_w.0;
        self.up_c.0 += s * o.up_c.0;
        self.attempts += sign * o.attempts;
        for (k, v) in &o.skipped {
            let e = self.skipped.entry(k.clone()).or_insert(0);
            *e += sign * v;
            if *e == 0 {
                self.skipped.remove(k);
            }
        }
        self.filter_skips += sign * o.filter_skips;
        self.missed += sign * o.missed;
        self.filled += sign * o.filled;
        self.unpriced += sign * o.unpriced;
        self.pooled += sign * o.pooled;
        self.nodays += sign * o.nodays;
        self.sx.0 += s * o.sx.0;
        self.sx2.0 += s * o.sx2.0;
        self.sabs.0 += s * o.sabs.0;
        self.sx0.0 += s * o.sx0.0;
        self.cost.0 += s * o.cost.0;
        self.cost_days.0 += s * o.cost_days.0;
        self.clusters += sign * o.clusters;
        self.sc2.0 += s * o.sc2.0;
        self.scn.0 += s * o.scn.0;
        self.sn2 += sign * o.sn2;
    }

    /// Executed attempts (the statistic's unit, spec §4): missed + filled.
    pub fn n_exec(&self) -> i64 {
        self.missed + self.filled
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweetCell {
    /// row_key → sums; empty rows are never stored.
    #[serde(default)]
    pub rows: BTreeMap<String, Acc>,
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Yes => "yes",
        Side::No => "no",
    }
}

pub fn row_key(side: Side, scenario: &str, dim: &str, bucket: &str) -> String {
    format!("{}|{scenario}|{dim}|{bucket}", side_str(side))
}

pub fn parse_key(k: &str) -> Option<(Side, String, String, String)> {
    let mut it = k.split('|');
    let side = match it.next()? {
        "yes" => Side::Yes,
        "no" => Side::No,
        _ => return None,
    };
    let (scen, dim, bucket) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    Some((side, scen.to_string(), dim.to_string(), bucket.to_string()))
}

impl SweetCell {
    fn apply(&mut self, o: &SweetCell, sign: i64) {
        for (k, a) in &o.rows {
            let e = self.rows.entry(k.clone()).or_default();
            e.apply(a, sign);
            if *e == Acc::default() {
                self.rows.remove(k);
            }
        }
    }
}

impl CellData for SweetCell {
    const FAMILY: &'static str = "sweet";
    /// Edge capture's areas (spec §6): no premise area of its own (review B1).
    const AREAS: &'static [&'static str] =
        &["depth", "detector_no", "detector_yes", "episode_def", "fees", "host", "sim"];
    const SWEET_INPUTS: bool = true;
    const LATE_MS: i64 = LATE_MS;
    type Max = ();

    fn key_suffix() -> String {
        key_suffix()
    }

    fn build(inp: &HourInput<'_>) -> Self {
        let s = &inp.sweet;
        let h = inp.hour_ms;
        let eps: BTreeMap<&str, &SweetEp> = s
            .eps
            .iter()
            .filter(|e| hour_of(e.opened_ms) == h)
            .map(|e| (e.id.as_str(), e))
            .collect();
        let mut by: BTreeMap<(&str, &str), &Attempt> = BTreeMap::new();
        for a in s.attempts {
            if eps.contains_key(a.episode_id.as_str()) {
                by.insert((a.scenario.as_str(), a.episode_id.as_str()), a);
            }
        }
        let mut c = SweetCell::default();
        // row → event → (executed n, Σx)
        let mut clus: BTreeMap<String, BTreeMap<&str, (i64, Decimal)>> = BTreeMap::new();
        for e in eps.values() {
            if !e.positive() {
                continue;
            }
            for (_, scen) in s.scenarios.iter().filter(|(sd, _)| *sd == e.side) {
                let fee_row = scen.contains('@');
                let a = by
                    .get(&(scen.as_str(), e.id.as_str()))
                    .copied()
                    .filter(|a| a.side == e.side);
                let cap = a.filter(|a| a.exec.is_some()).map(captured);
                for (dim, bucket) in e.buckets(fee_row) {
                    let key = row_key(e.side, scen, dim, bucket);
                    let acc = c.rows.entry(key.clone()).or_default();
                    if e.flagged {
                        acc.flagged += 1;
                        continue;
                    }
                    if let Some(b) = e.budget100 {
                        acc.eps += 1;
                        acc.on_table.0 += b;
                        let filtered = a
                            .and_then(|a| a.skip.as_deref())
                            .is_some_and(is_filter_skip);
                        if !filtered {
                            acc.on_table_elig.0 += b;
                        }
                    }
                    if e.side == Side::Yes
                        && let Some(ann) = e.annualized
                    {
                        acc.up_w.0 += ann * e.peak_cost;
                        acc.up_c.0 += e.peak_cost;
                    }
                    let Some(a) = a else { continue };
                    let in_flight = a.exec.is_some_and(|(t, _)| t > s.now_ms - IN_FLIGHT_MS);
                    if a.pending || in_flight || a.skip.as_deref() == Some(NOT_NET_POSITIVE) {
                        continue;
                    }
                    acc.attempts += 1;
                    if let Some(r) = &a.skip {
                        *acc.skipped.entry(r.clone()).or_insert(0) += 1;
                        if is_filter_skip(r) {
                            acc.filter_skips += 1;
                        }
                        continue;
                    }
                    let (Some((_, q)), Some(cp)) = (a.exec, cap) else { continue };
                    if q > Decimal::ZERO {
                        acc.filled += 1;
                    } else {
                        acc.missed += 1;
                    }
                    acc.sx.0 += cp.x;
                    acc.sx2.0 += cp.x * cp.x;
                    acc.sabs.0 += cp.x.abs();
                    acc.sx0.0 += cp.x0;
                    acc.cost.0 += cp.cost;
                    acc.unpriced += cp.unpriced;
                    acc.pooled += cp.pooled;
                    if e.side == Side::Yes {
                        match e.days {
                            Some(d) => acc.cost_days.0 += cp.cost * d,
                            None => acc.nodays += 1,
                        }
                    }
                    let cl = clus
                        .entry(key)
                        .or_default()
                        .entry(e.event_id.as_str())
                        .or_insert((0, Decimal::ZERO));
                    cl.0 += 1;
                    cl.1 += cp.x;
                }
            }
        }
        for (key, events) in clus {
            if let Some(acc) = c.rows.get_mut(&key) {
                for (n, cx) in events.values() {
                    acc.clusters += 1;
                    acc.sc2.0 += *cx * *cx;
                    acc.scn.0 += *cx * Decimal::from(*n);
                    acc.sn2 += n * n;
                }
            }
        }
        c.rows.retain(|_, a| *a != Acc::default());
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) {}

    fn merge_max(_: &mut (), _: &()) {}
}
```
If `Num`'s inner field is not `pub`, use its constructor and accessor the same way `edge.rs` does (`c.table.get_mut(e.side).0 += b` shows `.0` is public). Check that clippy accepts the let-chain style: the crate already uses let-chains in `edge.rs:188`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p cards` and `cargo clippy --workspace --all-targets -- -D warnings`. The analyze crate must compile with the `HourInput` literal updated.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cards/src/cells.rs crates/cards/src/sweet.rs crates/cards/tests/sweet.rs crates/cards/tests/sweet_property.rs crates/cards/tests/cells.rs crates/cards/tests/edge.rs crates/cards/tests/merge_property.rs crates/cards/tests/bench_tick.rs bins/analyze/src/tick.rs
git commit -m "cards: the SWEET cell (additive sums and event-hour clusters) and the CellData hooks"
```

---

### Task 7: `cards`: intervals, labels and the SWEET body

**Files:**
- Modify: `crates/cards/src/body.rs` (`BodyCtx.hurdle`, `Series.sweet`, the `Card::parts` doc), `crates/cards/src/sweet.rs`
- Modify (new `BodyCtx` field): every `BodyCtx { … }` literal (`grep -rn "BodyCtx {" crates bins`: 4 sites, including `bins/analyze/src/tick.rs` `ctx_for` and `crates/cards/tests/bench_tick.rs`)
- Test: `crates/cards/tests/sweet.rs`

**Interfaces:**
- Consumes: Tasks 5–6.
- Produces:
```rust
// cards::body
pub struct BodyCtx<'a> { …existing…, pub hurdle: Option<Decimal> }
pub struct Series { …existing…, #[serde(default, skip_serializing_if = "Option::is_none")] pub sweet: Option<SweetSeries> }
// cards::sweet
pub fn z_for(m: i64) -> Decimal;
pub struct RowStats { pub n: i64, pub mean: Option<Decimal>, pub se: Option<Decimal>, pub eff_n: Option<Decimal> }
pub fn row_stats(a: &Acc) -> RowStats;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum Label { Candidate, Negative, Inconclusive, Thin, NotTested, DeadOnPaper }
pub struct Verdict { pub label: Label, pub lo: Option<Decimal>, pub hi: Option<Decimal>, pub annualized: Option<Decimal>, pub annualized_lo: Option<Decimal>, pub annualized_upper: Option<Decimal>, pub few_trades: bool }
pub fn is_thin(a: &Acc) -> bool;
pub fn is_not_tested(side: Side, a: &Acc) -> bool;
pub fn verdict(side: Side, a: &Acc, z: Decimal, hurdle: Option<Decimal>) -> Verdict;
pub struct SweetRow { … see Step 3 … }
pub struct SweetFee { pub fee_bips: i64, pub captured: String, pub mean: Option<String>, pub label: Label }
pub struct SweetSeries { pub rows: Vec<SweetRow>, pub m: i64, pub z: String, pub category_available: bool, pub hurdle: Option<String>, pub fees: Vec<SweetFee> }
pub const CAVEAT: &str; pub fn footnote(m: i64) -> String;
pub fn sweet_parts(c: &SweetCell, ctx: &BodyCtx<'_>) -> Parts;
impl Card for SweetCell
```

- [ ] **Step 1: Write the failing tests**

Append to `crates/cards/tests/sweet.rs`:
```rust
use cards::body::{BodyCtx, Card, ScenarioLat, Status};

fn acc_with(xs: &[(&str, Decimal)]) -> Acc {
    // one executed attempt per (event, x), all filled; clusters by event within one hour
    let mut a = Acc::default();
    let mut by: BTreeMap<&str, (i64, Decimal)> = BTreeMap::new();
    for (ev, x) in xs {
        a.attempts += 1;
        a.filled += 1;
        a.sx.0 += *x;
        a.sx2.0 += *x * *x;
        a.sabs.0 += x.abs();
        a.cost.0 += dec!(10);
        a.cost_days.0 += dec!(10) * dec!(5);
        let e = by.entry(ev).or_insert((0, Decimal::ZERO));
        e.0 += 1;
        e.1 += *x;
    }
    for (n, c) in by.values() {
        a.clusters += 1;
        a.sc2.0 += *c * *c;
        a.scn.0 += *c * Decimal::from(*n);
        a.sn2 += n * n;
    }
    a
}
fn evs(n: usize, x: Decimal) -> Vec<(String, Decimal)> {
    (0..n).map(|i| (format!("ev{i}"), x)).collect()
}
fn acc_n(n: usize, x: Decimal) -> Acc {
    let v = evs(n, x);
    acc_with(&v.iter().map(|(e, x)| (e.as_str(), *x)).collect::<Vec<_>>())
}

#[test]
fn z_uses_the_next_larger_m() {
    assert_eq!(z_for(0), dec!(1.960));
    assert_eq!(z_for(1), dec!(1.960));
    assert_eq!(z_for(2), dec!(2.576));
    assert_eq!(z_for(51), dec!(3.481));
    assert_eq!(z_for(500), dec!(3.891));
    assert_eq!(z_for(10_000), dec!(3.891), "rows never exceed 196; the last entry is a floor");
}

#[test]
fn the_cluster_se_equals_the_plain_se_when_every_cluster_has_one_attempt() {
    // xs = 1, 2, 3, 4: mean 2.5, sample sd = sqrt(5/3), se = sd / 2
    let a = acc_with(&[("a", dec!(1)), ("b", dec!(2)), ("c", dec!(3)), ("d", dec!(4))]);
    let s = row_stats(&a);
    assert_eq!(s.n, 4);
    assert_eq!(s.mean, Some(dec!(2.5)));
    let se = s.se.unwrap();
    // sqrt(4/3 * 5) / 4 = 0.6454972...
    assert!((se - dec!(0.6454972)).abs() < dec!(0.0000001), "{se}");
}

#[test]
fn clusters_widen_the_se_when_one_event_repeats() {
    let spread = row_stats(&acc_with(&[("a", dec!(1)), ("b", dec!(1)), ("c", dec!(-1)), ("d", dec!(-1))]));
    let clumped = row_stats(&acc_with(&[("a", dec!(1)), ("a", dec!(1)), ("c", dec!(-1)), ("c", dec!(-1))]));
    assert!(clumped.se.unwrap() > spread.se.unwrap());
    assert_eq!(row_stats(&acc_with(&[("a", dec!(1))])).se, None, "one cluster: no se");
}

#[test]
fn missed_attempts_count_in_the_mean() {
    let mut a = acc_with(&[("a", dec!(1)), ("b", dec!(1))]);
    a.filled -= 1;
    a.missed += 1; // the second was a miss that still traded
    assert_eq!(row_stats(&a).n, 2);
    assert_eq!(row_stats(&a).mean, Some(dec!(1)));
}

#[test]
fn labels_follow_the_interval_the_minimums_and_the_hurdle() {
    let z = dec!(1.960);
    let h = Some(dec!(0.10));
    // 30 events, x = +0.10 each: se 0, lo = 0.10 > 0 → candidate (NO side has no hurdle)
    assert_eq!(verdict(Side::No, &acc_n(30, dec!(0.10)), z, h).label, Label::Candidate);
    assert_eq!(verdict(Side::No, &acc_n(30, dec!(-0.10)), z, h).label, Label::Negative);
    assert_eq!(verdict(Side::No, &acc_n(29, dec!(0.10)), z, h).label, Label::Thin);
    // 30 attempts in 29 clusters: thin
    let mut v = evs(29, dec!(0.10));
    v.push(("ev0".into(), dec!(0.10)));
    let a = acc_with(&v.iter().map(|(e, x)| (e.as_str(), *x)).collect::<Vec<_>>());
    assert_eq!((a.n_exec(), a.clusters), (30, 29));
    assert_eq!(verdict(Side::No, &a, z, h).label, Label::Thin);
    // mixed signs: inconclusive
    let mut mixed = evs(15, dec!(1));
    mixed.extend((15..30).map(|i| (format!("ev{i}"), dec!(-1))));
    let m = acc_with(&mixed.iter().map(|(e, x)| (e.as_str(), *x)).collect::<Vec<_>>());
    assert_eq!(verdict(Side::No, &m, z, h).label, Label::Inconclusive);
    // YES: lo 0.10 per $10 over 5 days: 0.10 x 30 / (300 x 5) x 365 = 0.73 >= 0.10 → candidate
    assert_eq!(verdict(Side::Yes, &acc_n(30, dec!(0.10)), z, h).label, Label::Candidate);
    // YES with the hurdle at 1.00: the annualized lower bound fails → inconclusive
    assert_eq!(verdict(Side::Yes, &acc_n(30, dec!(0.10)), z, Some(dec!(1))).label, Label::Inconclusive);
    // YES without a hurdle (analyze could not read it): never a candidate
    assert_eq!(verdict(Side::Yes, &acc_n(30, dec!(0.10)), z, None).label, Label::Inconclusive);
}

#[test]
fn all_three_p1_filter_skips_make_a_yes_row_not_tested_and_a_poor_upper_bound_dead() {
    for r in P1_FILTER_SKIPS {
        let mut a = Acc::default();
        a.attempts = 3;
        a.filter_skips = 3;
        a.skipped.insert(r.to_string(), 3);
        a.up_w.0 = dec!(5);
        a.up_c.0 = dec!(100); // upper bound 5% < 10%
        assert_eq!(verdict(Side::Yes, &a, dec!(1.96), Some(dec!(0.10))).label, Label::DeadOnPaper, "{r}");
        a.up_w.0 = dec!(50); // 50% ≥ 10%
        assert_eq!(verdict(Side::Yes, &a, dec!(1.96), Some(dec!(0.10))).label, Label::NotTested, "{r}");
        assert_eq!(verdict(Side::No, &a, dec!(1.96), Some(dec!(0.10))).label, Label::Thin, "NO rows are never not_tested");
    }
}

#[test]
fn few_trades_fire_below_an_effective_n_of_five() {
    let mut v = evs(29, dec!(0.01));
    v.push(("big".into(), dec!(50)));
    let a = acc_with(&v.iter().map(|(e, x)| (e.as_str(), *x)).collect::<Vec<_>>());
    assert!(verdict(Side::No, &a, dec!(1.96), None).few_trades);
    assert!(!verdict(Side::No, &acc_n(30, dec!(1)), dec!(1.96), None).few_trades);
}

fn ctx<'a>(sc: &'a [ScenarioLat], hurdle: Option<Decimal>) -> BodyCtx<'a> {
    BodyCtx {
        win: "30d", base: Status::Live, premise_key: "k", analyze_version: "t", from_ms: Some(0),
        to_ms: Some(H), since_ms: Some(0), computed_ms: H, scenarios: sc, hurdle,
    }
}
fn lats() -> Vec<ScenarioLat> {
    vec![ScenarioLat { name: "sp-idle".into(), net_ms: 145, latency_ms: 300 },
         ScenarioLat { name: "colo-idle".into(), net_ms: 1, latency_ms: 52 }]
}

#[test]
fn an_empty_window_renders_every_row_thin_with_m_zero() {
    let sc = lats();
    let body = SweetCell::bodies(&SweetCell::default(), &(), &ctx(&sc, Some(dec!(0.10))));
    assert_eq!(body.len(), 1);
    let b = &body[0];
    assert_eq!((b.card.as_str(), b.paper.as_str(), b.status), ("SWEET", "sweet", Status::Live));
    let s = b.series.sweet.as_ref().unwrap();
    assert_eq!((s.m, s.z.as_str(), s.rows.len()), (0, "1.96", 0));
    assert!(b.notes.iter().any(|n| n == CAVEAT));
}

#[test]
fn the_body_carries_rows_m_z_fees_latency_and_the_footnote() {
    let sc = lats();
    let mut c = SweetCell::default();
    c.rows.insert(row_key(Side::No, "sp-idle", "all", "all"), acc_n(30, dec!(0.10)));
    c.rows.insert(row_key(Side::No, "sp-idle", "days", "1-7"), acc_n(30, dec!(-0.10)));
    c.rows.insert(row_key(Side::No, "colo-idle", "all", "all"), acc_n(3, dec!(0.10)));
    c.rows.insert(row_key(Side::No, "sp-idle@10", "all", "all"), acc_n(30, dec!(0.05)));
    let b = &SweetCell::bodies(&c, &(), &ctx(&sc, Some(dec!(0.10))))[0];
    let s = b.series.sweet.as_ref().unwrap();
    assert_eq!(s.m, 3, "the thin colo-idle row is not tested");
    assert_eq!(s.z, "2.576");
    let r = s.rows.iter().find(|r| r.scenario == "sp-idle" && r.dimension == "all").unwrap();
    assert_eq!((r.label, r.latency_ms, r.fee_bips, r.n_exec), (Label::Candidate, Some(300), 0, 30));
    assert_eq!(r.mean.as_deref(), Some("0.1"));
    let fee = s.rows.iter().find(|r| r.fee_bips == 10).unwrap();
    assert_eq!((fee.scenario.as_str(), fee.latency_ms), ("sp-idle", Some(300)));
    assert_eq!(s.fees.iter().map(|f| f.fee_bips).collect::<Vec<_>>(), vec![0, 10]);
    assert!(b.notes.contains(&footnote(3)));
    assert_eq!(footnote(3), "3 rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.");
    assert!(b.hash.len() == 64);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p cards --test sweet`
Expected: FAIL to compile (`z_for`, `verdict`, `BodyCtx.hurdle`, `Series.sweet` missing).

- [ ] **Step 3: Implement**

`crates/cards/src/body.rs`:
- `BodyCtx` gains `/// P1's min_annualized from policies.toml (SWEET's YES hurdle); None: unknown.` `pub hurdle: Option<Decimal>,` (add `use rust_decimal::Decimal;`).
- `Series` gains:
```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweet: Option<crate::sweet::SweetSeries>,
```
- In the `Card::parts` doc, change "Must read from `ctx` only `premise_key` and `scenarios`" to "…only `premise_key`, `scenarios` and `hurdle` (the same for every window of a key)".
- Add `hurdle: None,` to the existing `BodyCtx` literals in tests. Task 8 sets it in analyze; add `hurdle: None` in `tick.rs` `ctx_for` for now.

Append to `crates/cards/src/sweet.rs`:
```rust
use crate::body::{BodyCtx, Card, Parts, Series};
use crate::stats::dec;
use rust_decimal::MathematicalOps;

pub const CAVEAT: &str = "Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.";

pub fn footnote(m: i64) -> String {
    format!("{m} rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.")
}

pub fn z_for(m: i64) -> Decimal {
    for (lim, z) in Z_TABLE {
        if m <= lim {
            return Decimal::new(z, 3);
        }
    }
    Decimal::new(Z_TABLE[Z_TABLE.len() - 1].1, 3)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowStats {
    pub n: i64,
    pub mean: Option<Decimal>,
    /// Cluster-robust (event-hour clusters), ratio estimator (spec §7.2); None below 2 clusters.
    pub se: Option<Decimal>,
    /// (Σ|x|)² / Σx²; None when Σx² = 0.
    pub eff_n: Option<Decimal>,
}

pub fn row_stats(a: &Acc) -> RowStats {
    let n = a.n_exec();
    if n <= 0 {
        return RowStats { n, mean: None, se: None, eff_n: None };
    }
    let nd = Decimal::from(n);
    let mean = a.sx.0 / nd;
    let ssr = (a.sc2.0 - Decimal::TWO * mean * a.scn.0 + mean * mean * Decimal::from(a.sn2))
        .max(Decimal::ZERO);
    let se = if a.clusters >= 2 {
        let c = Decimal::from(a.clusters);
        (c / (c - Decimal::ONE) * ssr).sqrt().map(|r| r / nd)
    } else {
        None
    };
    let eff_n = (a.sx2.0 > Decimal::ZERO).then(|| a.sabs.0 * a.sabs.0 / a.sx2.0);
    RowStats { n, mean: Some(mean), se, eff_n }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    Candidate,
    Negative,
    Inconclusive,
    Thin,
    NotTested,
    DeadOnPaper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub label: Label,
    pub lo: Option<Decimal>,
    pub hi: Option<Decimal>,
    pub annualized: Option<Decimal>,
    pub annualized_lo: Option<Decimal>,
    pub annualized_upper: Option<Decimal>,
    pub few_trades: bool,
}

pub fn is_thin(a: &Acc) -> bool {
    a.n_exec() < MIN_EXEC || a.clusters < MIN_CLUSTERS
}

/// A YES row where P1 took nothing: every intent was one of its three filter skips (spec §7.4).
pub fn is_not_tested(side: Side, a: &Acc) -> bool {
    side == Side::Yes && a.attempts > 0 && a.filter_skips == a.attempts
}

fn year() -> Decimal {
    Decimal::from(365)
}

pub fn verdict(side: Side, a: &Acc, z: Decimal, hurdle: Option<Decimal>) -> Verdict {
    let st = row_stats(a);
    let (lo, hi) = match (st.mean, st.se) {
        (Some(m), Some(se)) => (Some(m - z * se), Some(m + z * se)),
        _ => (None, None),
    };
    let yes_annualizable = side == Side::Yes && a.cost_days.0 > Decimal::ZERO && a.nodays == 0;
    let annualized = yes_annualizable.then(|| a.sx.0 / a.cost_days.0 * year());
    let annualized_lo = if yes_annualizable {
        lo.map(|l| l * Decimal::from(st.n) / a.cost_days.0 * year())
    } else {
        None
    };
    let annualized_upper = (side == Side::Yes && a.up_c.0 > Decimal::ZERO).then(|| a.up_w.0 / a.up_c.0);
    let few_trades = st.eff_n.is_some_and(|e| e < Decimal::from(FEW_TRADES_EFF_N));
    let label = if is_not_tested(side, a) {
        match (annualized_upper, hurdle) {
            (Some(u), Some(h)) if u < h => Label::DeadOnPaper,
            _ => Label::NotTested,
        }
    } else if is_thin(a) {
        Label::Thin
    } else if lo.is_some_and(|l| l > Decimal::ZERO)
        && (side == Side::No
            || matches!((annualized_lo, hurdle), (Some(al), Some(h)) if al >= h))
    {
        Label::Candidate
    } else if hi.is_some_and(|h| h < Decimal::ZERO) {
        Label::Negative
    } else {
        Label::Inconclusive
    };
    Verdict { label, lo, hi, annualized, annualized_lo, annualized_upper, few_trades }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweetRow {
    pub side: String,
    /// The account's scenario (without the fee suffix).
    pub scenario: String,
    pub fee_bips: i64,
    pub latency_ms: Option<i64>,
    pub dimension: String,
    pub bucket: String,
    pub label: Label,
    pub few_trades: bool,
    pub n_exec: i64,
    pub n_filled: i64,
    pub n_missed: i64,
    pub clusters: i64,
    pub n_attempts: i64,
    pub skipped: BTreeMap<String, i64>,
    pub episodes: i64,
    pub flagged: i64,
    pub on_table: String,
    pub on_table_eligible: String,
    pub captured: String,
    pub captured_leftover0: String,
    pub mean: Option<String>,
    pub lo: Option<String>,
    pub hi: Option<String>,
    pub eff_n: Option<String>,
    pub cost: String,
    pub annualized: Option<String>,
    pub annualized_lo: Option<String>,
    pub annualized_upper: Option<String>,
    pub leftover_unpriced: i64,
    pub pooled: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweetFee {
    pub fee_bips: i64,
    pub captured: String,
    pub mean: Option<String>,
    pub label: Label,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweetSeries {
    pub rows: Vec<SweetRow>,
    pub m: i64,
    pub z: String,
    pub category_available: bool,
    pub hurdle: Option<String>,
    /// The NO side's "all" rows at the headline scenario, by fee (spec §8).
    pub fees: Vec<SweetFee>,
}

/// Display precision for per-attempt figures (money): exact sums stay exact.
fn d6(v: Decimal) -> String {
    dec(v.round_dp(6))
}

fn order(list: &[&str], v: &str) -> usize {
    list.iter().position(|x| *x == v).unwrap_or(list.len())
}

fn bucket_order(dim: &str, bucket: &str) -> usize {
    match dim {
        "category" => CATEGORIES
            .iter()
            .position(|(b, _)| *b == bucket)
            .unwrap_or_else(|| if bucket == OTHER { CATEGORIES.len() } else { CATEGORIES.len() + 1 }),
        "days" => order(&DAYS_BUCKETS, bucket),
        "s" => order(&S_BUCKETS, bucket),
        "hour" => order(&HOUR_BUCKETS, bucket),
        _ => 0,
    }
}

pub fn sweet_parts(c: &SweetCell, ctx: &BodyCtx<'_>) -> Parts {
    let parsed: Vec<(Side, String, String, String, &Acc)> = c
        .rows
        .iter()
        .filter_map(|(k, a)| parse_key(k).map(|(s, sc, d, b)| (s, sc, d, b, a)))
        .collect();
    let m = i64::try_from(
        parsed
            .iter()
            .filter(|(s, _, _, _, a)| !is_thin(a) && !is_not_tested(*s, a))
            .count(),
    )
    .unwrap_or(i64::MAX);
    let z = z_for(m);
    let latency = |name: &str| ctx.scenarios.iter().find(|s| s.name == name).map(|s| s.latency_ms);
    let mut rows: Vec<SweetRow> = parsed
        .iter()
        .map(|(side, scen, dim, bucket, a)| {
            let v = verdict(*side, a, z, ctx.hurdle);
            let st = row_stats(a);
            let (base, fee) = fee_of(scen);
            SweetRow {
                side: side_str(*side).to_string(),
                latency_ms: latency(&base),
                scenario: base,
                fee_bips: fee,
                dimension: dim.clone(),
                bucket: bucket.clone(),
                label: v.label,
                few_trades: v.few_trades,
                n_exec: a.n_exec(),
                n_filled: a.filled,
                n_missed: a.missed,
                clusters: a.clusters,
                n_attempts: a.attempts,
                skipped: a.skipped.clone(),
                episodes: a.eps,
                flagged: a.flagged,
                on_table: dec(a.on_table.0),
                on_table_eligible: dec(a.on_table_elig.0),
                captured: dec(a.sx.0),
                captured_leftover0: dec(a.sx0.0),
                mean: st.mean.map(d6),
                lo: v.lo.map(d6),
                hi: v.hi.map(d6),
                eff_n: st.eff_n.map(|e| dec(e.round_dp(2))),
                cost: dec(a.cost.0),
                annualized: v.annualized.map(|x| dec(x.round_dp(4))),
                annualized_lo: v.annualized_lo.map(|x| dec(x.round_dp(4))),
                annualized_upper: v.annualized_upper.map(|x| dec(x.round_dp(4))),
                leftover_unpriced: a.unpriced,
                pooled: a.pooled,
            }
        })
        .collect();
    rows.sort_by(|x, y| {
        (x.side.as_str(), order(&DIMS, &x.dimension), bucket_order(&x.dimension, &x.bucket), x.latency_ms, x.fee_bips, x.scenario.as_str())
            .cmp(&(y.side.as_str(), order(&DIMS, &y.dimension), bucket_order(&y.dimension, &y.bucket), y.latency_ms, y.fee_bips, y.scenario.as_str()))
    });
    let fees: Vec<SweetFee> = rows
        .iter()
        .filter(|r| r.side == "no" && r.dimension == "all" && r.scenario == HEADLINE)
        .map(|r| SweetFee { fee_bips: r.fee_bips, captured: r.captured.clone(), mean: r.mean.clone(), label: r.label })
        .collect();
    let n = rows
        .iter()
        .filter(|r| r.dimension == "all" && r.fee_bips == 0)
        .map(|r| r.n_exec)
        .sum();
    let mut notes = vec![
        CAVEAT.to_string(),
        footnote(m),
        "Rows are one dimension at a time; the unit is one executed paper attempt (filled or missed), clustered by event and hour.".to_string(),
        "Leftover legs are valued at their 30 s markout mid; captured_leftover0 values them at 0.".to_string(),
        "YES rows measure only what P1 trades: complete baskets resolving within 30 days with at least the hurdle annualized; the rest is not tested.".to_string(),
        "YES complete sets are counted at 1 each (a non-binary resolution is an integrity class of its own).".to_string(),
        "Flagged episodes (engine_suspect, stale_transport) are excluded and counted, so SWEET does not reconcile with account equity.".to_string(),
    ];
    if ctx.hurdle.is_none() {
        notes.push("The YES hurdle could not be read from policies.toml: no YES row can be a candidate.".to_string());
    }
    if !CATEGORY_AVAILABLE {
        notes.push("No category: Gamma listed no tags for these events.".to_string());
    }
    Parts {
        card: "SWEET",
        paper: "sweet",
        title: "Sweet spots",
        params: params_text(),
        n,
        columns: Vec::new(),
        series: Series {
            sweet: Some(SweetSeries {
                rows,
                m,
                z: dec(z),
                category_available: CATEGORY_AVAILABLE,
                hurdle: ctx.hurdle.map(dec),
                fees,
            }),
            ..Series::default()
        },
        notes,
        thin: Vec::new(),
        episode_link: None,
    }
}

impl Card for SweetCell {
    fn parts(agg: &Self, _max: &(), ctx: &BodyCtx<'_>) -> Vec<Parts> {
        vec![sweet_parts(agg, ctx)]
    }
}
```
Notes on the implementation:
- `dec(Decimal::new(1960, 3))` is `"1.96"`, because `dec` normalizes; the test expects that.
- `Series` derives `Eq`, so `SweetSeries` must too (it does).
- If clippy's `float_arithmetic` lint objects to nothing, good: these are all `Decimal` ops.
- `sqrt` returns `Option` and never panics.
- `Decimal` multiplication can overflow on absurd inputs. The row sums are bounded by real money, and the crate's existing code uses plain operators, so follow it.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p cards` and `cargo clippy --workspace --all-targets -- -D warnings` (analyze must compile with `hurdle: None`).
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cards/src/body.rs crates/cards/src/sweet.rs crates/cards/tests/sweet.rs crates/cards/tests/bench_tick.rs crates/cards/tests/bodies.rs bins/analyze/src/tick.rs
git commit -m "cards: SWEET intervals (cluster-robust, Bonferroni), labels and the card body"
```
Stage only the `BodyCtx` literal files you actually touched (the grep in **Files** lists them).

---

### Task 8: `analyze`: the SWEET family (inputs, key suffix, late rebuild, hurdle) and the benchmark

**Files:**
- Modify: `bins/analyze/src/tick.rs` (Analyzer, Env, `sync`, `build_hours`, `ctx_for`), `bins/analyze/src/scenarios.rs`, `bins/analyze/src/main.rs:23,94`
- Test: `bins/analyze/tests/tick.rs`, `bins/analyze/src/scenarios.rs` (unit test), `crates/cards/tests/bench_sweet.rs` (new, `#[ignore]`)

**Interfaces:**
- Consumes: Task 4's store reads, Tasks 5–7.
- Produces:
  - `Analyzer::with_hurdle(self, hurdle: Option<Decimal>) -> Self`;
  - `analyze::scenarios::p1(path: &str) -> anyhow::Result<sim::config::P1Params>`;
  - the `SWEET` card for `24h`, `7d`, `30d` and `all` in `card_results`, keyed `<edge key>;sweet=1+<sha8>`.

- [ ] **Step 1: Write the failing analyze tests**

Append to `bins/analyze/tests/tick.rs`. The file already has `run`, `premises`, `ep`, `write`, `rw`, `body` and `paper` helpers (lines 1-148, 948-960). Insert premises exactly the way `a_tick_writes_every_card_and_window_with_statuses_and_meta` (line 149) does.
```rust
const P0: &str = "P0-converter/sp-idle";

fn p0_account(run: i64) -> PaperAccountRow {
    PaperAccountRow {
        run_id: run, account_id: P0.into(), policy: "P0-converter".into(), scenario: "sp-idle".into(),
        fee_bips: dec!(0), bankroll: dec!(1000), per_trade: dec!(100), policies_sha256: "x".into(),
    }
}

/// One executed NO attempt on `episode` at `exec_ms` capturing `x` (two buys, one conversion).
fn no_attempt(run: i64, episode: &str, exec_ms: i64, x: rust_decimal::Decimal, seq: i64) -> Batch {
    let led = |seq: i64, kind: &str, leg: Option<&str>, qty, cash| PaperLedgerRow {
        run_id: run, account_id: P0.into(), seq, t_ms: exec_ms, kind: kind.into(),
        episode_id: Some(episode.into()), market_id: leg.map(|m| m.to_string()), yes_token: leg.map(|m| format!("{m}-y")),
        outcome: leg.map(|_| "no".to_string()), qty, price: None, cash_delta: cash, fees: dec!(0),
        realized_delta: dec!(0), cash_after: dec!(1000), equity: None,
    };
    Batch {
        paper_intents: vec![PaperIntentRow {
            run_id: run, account_id: P0.into(), episode_id: episode.into(), decided_ms: exec_ms - 300,
            exec_ms: Some(exec_ms), skip_reason: None, q_target: Some(dec!(10)), detected_q: None,
            detected_pnl: None, reserve: None,
        }],
        paper_execs: vec![PaperExecRow {
            run_id: run, account_id: P0.into(), episode_id: episode.into(), exec_ms, q_eff: dec!(10),
            leg_risk_qty: dec!(0), pnl_at_tx: None, hit: true,
        }],
        paper_ledger: vec![
            led(seq, "buy", Some("m0"), dec!(10), dec!(-4.00)),
            led(seq + 1, "buy", Some("m1"), dec!(10), dec!(-5.10)),
            led(seq + 2, "convert", None, dec!(10), dec!(9.10) + x),
        ],
        ..Batch::default()
    }
}

fn sweet_row(b: &Value, side: &str, scenario: &str, dim: &str, bucket: &str) -> Value {
    b["series"]["sweet"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["side"] == side && r["scenario"] == scenario && r["dimension"] == dim && r["bucket"] == bucket && r["fee_bips"] == 0)
        .cloned()
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn an_attempt_in_flight_at_one_tick_is_counted_once_at_the_next() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r = run(&owner, T0 - 2 * H).await;
    owner.insert_run_premises(&premises(r, "1+ab")).await.unwrap();
    // opened 10 s before the hour, closed 5 s later: its update is seen before the attempt lands
    let opened = T0 + H - 10_000;
    let mut e = ep("e1", r, "no_basket", opened, Some(opened + 5_000));
    e.days_to_end = Some(dec!(3));
    write(&owner, vec![e]).await;
    owner.write_batch(&Batch { paper_accounts: vec![p0_account(r)], ..Batch::default() }).await.unwrap();
    owner.write_batch(&no_attempt(r, "e1", T0 + H + 30_000, dec!(0.5), 1)).await.unwrap();
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "t", scenarios()).await.unwrap();
    a.tick(T0 + H + 45_000).await.unwrap(); // 15 s after exec: in flight
    let row = sweet_row(&body(&rw, "24h", "SWEET").await, "no", "sp-idle", "all", "all");
    assert_eq!((row["episodes"].as_i64(), row["n_attempts"].as_i64()), (Some(1), Some(0)));
    a.tick(T0 + H + 15 * M).await.unwrap(); // the late rule rebuilds T0 (hour of the episode)
    let row = sweet_row(&body(&rw, "24h", "SWEET").await, "no", "sp-idle", "all", "all");
    assert_eq!((row["n_attempts"].as_i64(), row["n_exec"].as_i64(), row["captured"].as_str()), (Some(1), Some(1), Some("0.5")));
    a.tick(T0 + H + 30 * M).await.unwrap();
    let row = sweet_row(&body(&rw, "24h", "SWEET").await, "no", "sp-idle", "all", "all");
    assert_eq!(row["n_exec"].as_i64(), Some(1), "counted once");
    db.drop(rw).await;
}

#[tokio::test]
async fn the_sweet_suffix_leaves_other_family_keys_unchanged() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r = run(&owner, T0 - 2 * H).await;
    owner.insert_run_premises(&premises(r, "1+ab")).await.unwrap();
    write(&owner, vec![ep("e1", r, "no_basket", T0 + M, Some(T0 + 2 * M))]).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "t", scenarios()).await.unwrap();
    a.tick(T0 + H).await.unwrap();
    let edge = body(&rw, "7d", "EDGE").await["premise_key"].as_str().unwrap().to_string();
    for card in ["G1", "G2", "G3", "G4", "EDGE"] {
        let k = body(&rw, "7d", card).await["premise_key"].as_str().unwrap().to_string();
        assert!(!k.contains(";sweet="), "{card}: {k}");
    }
    let sweet = body(&rw, "7d", "SWEET").await["premise_key"].as_str().unwrap().to_string();
    assert_eq!(sweet, format!("{edge}{}", cards::sweet::key_suffix()));
    db.drop(rw).await;
}

#[tokio::test]
async fn thirty_events_with_the_same_edge_make_the_no_row_a_candidate() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r = run(&owner, T0 - 2 * H).await;
    owner.insert_run_premises(&premises(r, "1+ab")).await.unwrap();
    owner.write_batch(&Batch { paper_accounts: vec![p0_account(r)], ..Batch::default() }).await.unwrap();
    let mut eps = Vec::new();
    for i in 0..30 {
        let id = format!("e{i}");
        eps.push(ep(&id, r, "no_basket", T0 + i * M, Some(T0 + i * M + 1_000))); // event ev-e{i}: 30 clusters
        owner.write_batch(&no_attempt(r, &id, T0 + i * M + 500, dec!(0.10), 10 * i + 1)).await.unwrap();
    }
    write(&owner, eps).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "t", scenarios())
        .await
        .unwrap()
        .with_hurdle(Some(dec!(0.10)));
    a.tick(T0 + 2 * H).await.unwrap();
    let b = body(&rw, "24h", "SWEET").await;
    let row = sweet_row(&b, "no", "sp-idle", "all", "all");
    assert_eq!((row["label"].as_str(), row["n_exec"].as_i64(), row["clusters"].as_i64()), (Some("candidate"), Some(30), Some(30)));
    assert!(b["series"]["sweet"]["m"].as_i64().unwrap() >= 1);
    assert_eq!(b["series"]["sweet"]["hurdle"].as_str(), Some("0.1"));
    db.drop(rw).await;
}
```
Use the real name of the premises insert (`grep -n "run_premises" bins/analyze/tests/tick.rs | head`) and the real `db.drop` / teardown order of this file. `ep()` gives `event_id = ev-{id}` (line 50), so each episode is its own event. The G1–G4 cards need Gebele episodes to render, and a single no_basket episode renders them (maybe `thin`). If a card is missing, assert over the cards present in `card_results_for("7d")` instead of a fixed list.

Add a unit test at the bottom of `bins/analyze/src/scenarios.rs`:
```rust
    #[test]
    fn the_repo_policies_give_the_sweet_hurdle_and_days_edge() {
        let p = p1(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/policies.toml")).unwrap();
        assert_eq!(p.min_annualized, rust_decimal::Decimal::new(10, 2));
        assert_eq!(p.max_days_to_end, rust_decimal::Decimal::from(cards::sweet::P1_MAX_DAYS),
            "SWEET's days bucket closes at P1's max_days_to_end");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run (Postgres prefix): `cargo test -p analyze`
Expected: FAIL (`with_hurdle`, `p1` missing; no SWEET card).

- [ ] **Step 3: `scenarios::p1`**

```rust
/// P1's thresholds from the same frozen policies.toml: SWEET's YES hurdle (min_annualized) and
/// the days edge its bucket closes on (Phase 5d spec §5, §7.5).
pub fn p1(path: &str) -> anyhow::Result<sim::config::P1Params> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    let sim: sim::config::SimConfig = toml::from_str(&text).context("parse policies")?;
    sim.validate().context("validate policies")?;
    Ok(sim.p1)
}
```
If `P1Params` isn't `Clone` or `SimConfig.p1` isn't movable here, return `(min_annualized, max_days_to_end)` as a tuple instead and adjust the test.

- [ ] **Step 4: Wire the family in `tick.rs`**

1. **Imports:** `use cards::sweet::{self, Attempt, Line, LineKind, MARKOUT_MS, SweetCell, SweetEp, SweetInput, classify_intent, row_scenario};` plus `use rust_decimal::Decimal;` (already imported).
2. **The accounts SWEET reads**, rebuilt every tick (a policies change adds accounts):
```rust
/// SWEET's accounts, rebuilt each tick: account id → (side, row scenario), and every row scenario.
#[derive(Debug, Clone, Default)]
struct SweetAccounts {
    by_account: BTreeMap<String, (Side, String)>,
    scenarios: Vec<(Side, String)>,
}

async fn sweet_accounts(store: &Store) -> anyhow::Result<SweetAccounts> {
    let mut out = SweetAccounts::default();
    for a in store.sweet_accounts().await? {
        if let Some(v) = row_scenario(&a.policy, &a.scenario, a.fee_bips) {
            if !out.scenarios.contains(&v) {
                out.scenarios.push(v.clone());
            }
            out.by_account.insert(a.account_id, v);
        }
    }
    Ok(out)
}
```
3. **`Analyzer`** gains `sweet: Family<SweetCell>` (initialised to `Family::new(0)` in `new` and in `reset`) and `hurdle: Option<Decimal>` (None in `new`):
```rust
    /// P1's min_annualized (SWEET's YES hurdle); main sets it from policies.toml.
    pub fn with_hurdle(mut self, hurdle: Option<Decimal>) -> Self {
        self.hurdle = hurdle;
        self
    }
```
4. **`Env`** gains `hurdle: Option<Decimal>` and `sweet: &'a SweetAccounts`. In `tick`, build them before `env`: `let sweet_accts = sweet_accounts(&self.store).await?;` then `hurdle: self.hurdle, sweet: &sweet_accts,`. After the edge `sync`, add `sync(&env, &mut self.sweet, &mut results, &mut meta, &mut report).await?;`.
5. **`sync`**: right after `current_key`, shadow the key with the family's suffix; the 5a families' suffix is empty, so their keys are unchanged:
```rust
    let key = format!("{key}{}", C::key_suffix());
```
Then, after the block that inserts `env.now_hour` and the hours since the last tick, add the late rule:
```rust
    // SWEET: an attempt still in flight at the last tick may belong to an hour no rule above
    // re-dirties (its episode closed before the tick): rebuild every hour touched by
    // [last tick − LATE_MS, now] (Phase 5d spec §6).
    if C::LATE_MS > 0 {
        let from = env.last_tick_ms.unwrap_or(env.now_ms).min(env.now_ms) - C::LATE_MS;
        dirty.extend(hours_between(from, env.now_ms));
    }
```
The existing `dirty.retain(|h| *h >= since_hour && *h <= env.now_hour)` keeps it inside the key's data.
6. **`build_hours`**: load the SWEET inputs once per range and pass them to every hour:
```rust
    let (seps, satts) = if C::SWEET_INPUTS {
        sweet_inputs(env, &run_ids, from, to).await?
    } else {
        (Vec::new(), Vec::new())
    };
```
and in the `HourInput` literal replace `sweet: SweetInput::default(),` with:
```rust
                sweet: SweetInput {
                    now_ms: env.now_ms,
                    eps: &seps,
                    attempts: &satts,
                    scenarios: &env.sweet.scenarios,
                },
```
7. **`sweet_inputs`:**
```rust
type AttemptKey = (i64, String, String);

/// SWEET's inputs for [from, to): the episodes opened there and every attempt of SWEET's accounts
/// on them, with its own ledger rows and 30 s markout mids (Phase 5d spec §4).
async fn sweet_inputs(
    env: &Env<'_>,
    run_ids: &[i64],
    from: i64,
    to: i64,
) -> anyhow::Result<(Vec<SweetEp>, Vec<Attempt>)> {
    let eps: Vec<SweetEp> = env
        .store
        .sweet_episodes(run_ids, from, to)
        .await?
        .into_iter()
        .filter_map(|e| {
            Some(SweetEp {
                side: Side::from_kind(&e.kind)?,
                id: e.episode_id,
                event_id: e.event_id,
                opened_ms: e.opened_ms,
                pnl_net: e.pnl_net,
                budget100: e.budget100,
                flagged: e.suspect || e.stale,
                days: e.days_to_end,
                annualized: e.annualized,
                peak_cost: e.peak_cost,
                s_legs: e.s_legs,
                tags: e.tags,
            })
        })
        .collect();
    let ids: Vec<String> = env.sweet.by_account.keys().cloned().collect();
    let mut lines: BTreeMap<AttemptKey, Vec<Line>> = BTreeMap::new();
    for l in env.store.sweet_ledger(run_ids, &ids, from, to).await? {
        let kind = match l.kind.as_str() {
            "buy" => LineKind::Buy,
            "sell" => LineKind::Sell,
            "convert" => LineKind::Convert,
            _ => continue,
        };
        lines
            .entry((l.run_id, l.account_id, l.episode_id))
            .or_default()
            .push(Line { kind, leg: l.market_id.zip(l.outcome), qty: l.qty, cash_delta: l.cash_delta });
    }
    let mut mids: BTreeMap<AttemptKey, BTreeMap<sweet::Leg, Decimal>> = BTreeMap::new();
    for m in env.store.sweet_mids(run_ids, &ids, from, to, MARKOUT_MS).await? {
        if let Some(v) = m.mid {
            mids.entry((m.run_id, m.account_id, m.episode_id))
                .or_default()
                .insert((m.market_id, m.outcome), v);
        }
    }
    let mut out = Vec::new();
    for i in env.store.sweet_intents(run_ids, &ids, from, to).await? {
        let Some((side, scenario)) = env.sweet.by_account.get(&i.account_id).cloned() else {
            continue;
        };
        let exec = i.exec_ms.zip(i.q_eff);
        let (skip, pending) =
            classify_intent(i.skip_reason, i.planned_exec_ms, exec.is_some(), env.now_ms);
        let k = (i.run_id, i.account_id, i.episode_id);
        out.push(Attempt {
            side,
            scenario,
            skip,
            exec,
            pending,
            lines: lines.remove(&k).unwrap_or_default(),
            mid30: mids.remove(&k).unwrap_or_default(),
            episode_id: k.2,
        });
    }
    Ok((eps, out))
}
```
8. **`ctx_for`** in `family_results`: set `hurdle: env.hurdle,` (replacing Task 7's `hurdle: None`).
9. **`main.rs`**:
   - next to `let scenarios = analyze::scenarios::load(&cfg.policies_path)?;` (line 23), add:
```rust
    let p1 = analyze::scenarios::p1(&cfg.policies_path)?;
    if p1.max_days_to_end != rust_decimal::Decimal::from(cards::sweet::P1_MAX_DAYS) {
        eprintln!(
            "analyze: policies.toml p1.max_days_to_end is {}, SWEET's days bucket closes at {}: YES days rows will not line up with P1",
            p1.max_days_to_end,
            cards::sweet::P1_MAX_DAYS
        );
    }
    let hurdle = Some(p1.min_annualized);
```
   - at line 94, chain `.map(|a| a.with_hurdle(hurdle))` on the `Analyzer::new(…).await` result before `write_static_meta`. If `main.rs` builds the analyzer inside a function that receives `scenarios`, pass `hurdle` alongside it.
   - Check that `bins/analyze/Cargo.toml` already depends on `sim`, `toml` and `rust_decimal` (`scenarios.rs` uses the first two); add nothing from crates.io.

- [ ] **Step 5: Run the analyze tests to see them pass**

Run (Postgres prefix): `cargo test -p analyze` and `cargo clippy --workspace --all-targets -- -D warnings`.
Expected: PASS: the three new tests plus every 5a test unchanged (Review Focus 5: no 5a key moved).

- [ ] **Step 6: The benchmark (ignored by default, run by hand in a debug build)**

Create `crates/cards/tests/bench_sweet.rs`:
```rust
//! Phase 5d §9 benchmark (ignored; run by hand, debug build): a SWEET tick (advance one hour, rebuild
//! two hours, render 24h and 7d and one long window) stays under 50 ms over a 30-day history, and
//! what analyze holds for SWEET (48 recent cells + the four sums) serializes under 16 MB.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::body::{BodyCtx, Card, ScenarioLat, Status};
use cards::cells::{CellData, HourInput, Side};
use cards::sweet::*;
use cards::window::{SPANS, Windows};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::time::Instant;

const H: i64 = 3_600_000;
const PER_HOUR: i64 = 20;

fn scen() -> Vec<(Side, String)> {
    let mut v: Vec<(Side, String)> = ["colo-idle", "sp-idle", "sp-burst", "sp-idle-nodelay"]
        .iter().map(|s| (Side::No, s.to_string())).collect();
    v.push((Side::No, "sp-idle@10".into()));
    v.push((Side::No, "sp-idle@50".into()));
    v.extend(["colo-idle", "sp-idle", "sp-burst"].iter().map(|s| (Side::Yes, s.to_string())));
    v
}

fn cell_for(h: i64, sc: &[(Side, String)]) -> SweetCell {
    let mut eps = Vec::new();
    let mut atts = Vec::new();
    for i in 0..PER_HOUR {
        let side = if i % 4 == 0 { Side::Yes } else { Side::No };
        let id = format!("e{}-{i}", h / H);
        eps.push(SweetEp {
            id: id.clone(), event_id: format!("ev{}", i % 7), side, opened_ms: h + i * 60_000,
            pnl_net: Decimal::new(41, 2), budget100: Some(Decimal::new(33, 2)), flagged: false,
            days: Some(Decimal::from(i % 40)), annualized: Some(Decimal::new(5, 1)), peak_cost: Decimal::from(99),
            s_legs: 2 + i % 9, tags: Some(vec![["nba", "politics", "crypto", "featured"][(i % 4) as usize].to_string()]),
        });
        for (sd, s) in sc.iter().filter(|(sd, _)| *sd == side) {
            let x = Decimal::new((i * 37 % 200) - 100, 2);
            atts.push(Attempt {
                episode_id: id.clone(), side: *sd, scenario: s.clone(), skip: None,
                exec: Some((h + i * 60_000 + 300, Decimal::from(10))), pending: false,
                lines: vec![
                    Line { kind: LineKind::Buy, leg: Some(("m0".into(), "no".into())), qty: Decimal::from(10), cash_delta: Decimal::new(-400, 2) },
                    Line { kind: LineKind::Buy, leg: Some(("m1".into(), "no".into())), qty: Decimal::from(10), cash_delta: Decimal::new(-510, 2) },
                    Line { kind: LineKind::Convert, leg: None, qty: Decimal::from(10), cash_delta: Decimal::new(910, 2) + x },
                ],
                mid30: BTreeMap::new(),
            });
        }
    }
    SweetCell::build(&HourInput {
        hour_ms: h, episodes: &[], latency_ms: &[], equity_deltas: &[],
        sweet: SweetInput { now_ms: h + 2 * H, eps: &eps, attempts: &atts, scenarios: sc },
    })
}

#[test]
#[ignore]
fn sweet_tick_stays_under_50ms_and_holds_under_16mb() {
    let sc = scen();
    let lats: Vec<ScenarioLat> = vec![ScenarioLat { name: "sp-idle".into(), net_ms: 145, latency_ms: 300 }];
    let now = 20_000 * 24 * H;
    let mut stored: BTreeMap<i64, SweetCell> = BTreeMap::new();
    let mut w = Windows::<SweetCell>::new(now);
    let t = Instant::now();
    for i in 0..720 {
        let h = now - (719 - i) * H;
        let c = cell_for(h, &sc);
        w.apply(h, None, &c);
        stored.insert(h, c);
    }
    println!("catch-up 720 h: {} ms", t.elapsed().as_millis());
    // one tick: the next hour arrives
    let t = Instant::now();
    let next = now + H;
    stored.insert(next, cell_for(next, &sc));
    let needed: BTreeMap<i64, SweetCell> = w
        .needed_ranges(next)
        .iter()
        .flat_map(|(a, b)| stored.range(*a..*b).map(|(h, c)| (*h, SweetCell::from_json(&c.to_json().unwrap()).unwrap())))
        .collect();
    w.advance(next, &needed);
    for h in [next - H, next] {
        let old = stored.get(&h).cloned();
        let new = cell_for(h, &sc);
        w.apply(h, old.as_ref(), &new);
        let _ = new.to_json().unwrap();
        stored.insert(h, new);
    }
    let ctx = |win| BodyCtx {
        win, base: Status::Live, premise_key: "k", analyze_version: "t", from_ms: Some(0), to_ms: Some(next),
        since_ms: Some(0), computed_ms: next, scenarios: &lats, hurdle: Some(Decimal::new(10, 2)),
    };
    for (i, (name, _)) in SPANS.iter().enumerate() {
        let body = SweetCell::bodies(&w.spans[i], &(), &ctx(name));
        let _ = serde_json::to_string(&body).unwrap();
    }
    let ms = t.elapsed().as_millis();
    println!("sweet tick: {ms} ms");
    assert!(ms < 50, "sweet tick took {ms} ms");
    let held: usize = w.recent.values().map(|c| c.to_json().unwrap().len()).sum::<usize>()
        + w.spans.iter().map(|c| c.to_json().unwrap().len()).sum::<usize>()
        + w.all.to_json().unwrap().len();
    println!("sweet held: {} KB", held / 1024);
    assert!(held < 16 * 1024 * 1024, "held {held} bytes");
}
```
The tick above renders all three spans every time, which is stricter than analyze (analyze renders 30d at most hourly). Run it:

Run: `cargo test -p cards --test bench_sweet -- --ignored --nocapture`
Expected: PASS, printing the catch-up time, the tick time (< 50 ms) and the held size (< 16 MB). Also re-run the 5a benchmark to confirm it's unaffected: `cargo test -p cards --test bench_tick -- --ignored --nocapture` (< 200 ms).

- [ ] **Step 7: Commit**

```bash
git add bins/analyze/src/tick.rs bins/analyze/src/scenarios.rs bins/analyze/src/main.rs bins/analyze/tests/tick.rs crates/cards/tests/bench_sweet.rs
git commit -m "analyze: the SWEET family (attempt inputs, key suffix, late rebuild, P1 hurdle) and its benchmark"
```

---

### Task 9: web: the Sweet spots box on the Paper page

**Files:**
- Modify: `web/src/api/types.ts` (CardSeries), `web/src/components/WindowSwitch.tsx` (optional `label`), `web/src/help.ts`, `web/src/pages/Paper.tsx:289`, `web/src/test/fixtures.ts`
- Create: `web/src/lib/sweet.ts`, `web/src/lib/sweet.test.ts`, `web/src/components/SweetSpots.tsx`, `web/src/components/SweetSpots.test.tsx`

**Interfaces:**
- Consumes: the SWEET body from `GET /api/cards?win=` (Task 7's `SweetSeries`, serialized as is).
- Produces: `<SweetSpots />` below `<EdgeCapture />`.

- [ ] **Step 1: Types and fixture**

`web/src/api/types.ts`: add
```ts
export type SweetLabel = 'candidate' | 'negative' | 'inconclusive' | 'thin' | 'not_tested' | 'dead_on_paper';
export interface SweetRow {
  side: 'yes' | 'no';
  scenario: string;
  fee_bips: number;
  latency_ms: number | null;
  dimension: 'all' | 'category' | 'days' | 's' | 'hour';
  bucket: string;
  label: SweetLabel;
  few_trades: boolean;
  n_exec: number;
  n_filled: number;
  n_missed: number;
  clusters: number;
  n_attempts: number;
  skipped: Record<string, number>;
  episodes: number;
  flagged: number;
  on_table: Dec;
  on_table_eligible: Dec;
  captured: Dec;
  captured_leftover0: Dec;
  mean: Dec | null;
  lo: Dec | null;
  hi: Dec | null;
  eff_n: Dec | null;
  cost: Dec;
  annualized: Dec | null;
  annualized_lo: Dec | null;
  annualized_upper: Dec | null;
  leftover_unpriced: number;
  pooled: number;
}
export interface SweetFee {
  fee_bips: number;
  captured: Dec;
  mean: Dec | null;
  label: SweetLabel;
}
export interface SweetSeries {
  rows: SweetRow[];
  m: number;
  z: Dec;
  category_available: boolean;
  hurdle: Dec | null;
  fees: SweetFee[];
}
```
and `sweet?: SweetSeries;` in `CardSeries`.

`web/src/test/fixtures.ts`: add `sweetRow(o: Partial<SweetRow> = {}): SweetRow` (a thin NO/sp-idle/all/all row with every field filled: numbers 0, decimals `'0'`, `null` where nullable) and `sweetFixture(rows: SweetRow[], o: Partial<SweetSeries> = {}): CardBody` built on `cardFixture({ card: 'SWEET', paper: 'sweet', title: 'Sweet spots', win: '30d', series: { sweet: { rows, m: …, z: '1.96', category_available: true, hurdle: '0.1', fees: [], ...o } } })`. Compute `m` as the count of rows whose label is neither `thin` nor `not_tested`.

- [ ] **Step 2: Failing tests for the pure helpers**

`web/src/lib/sweet.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { sweetRow } from '../test/fixtures';
import type { SweetSeries } from '../api/types';
import { FEW_TRADES, LABEL_TEXT, NOTHING_PAYS, SWEET_CAVEAT, footnote, grid, headline, interval } from './sweet';

const series = (rows = [sweetRow()], o: Partial<SweetSeries> = {}): SweetSeries => ({
  rows, m: rows.filter((r) => r.label !== 'thin' && r.label !== 'not_tested').length, z: '2.576',
  category_available: true, hurdle: '0.1', fees: [], ...o,
});

describe('sweet helpers', () => {
  it('keeps the spec copy verbatim', () => {
    expect(SWEET_CAVEAT).toBe("Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.");
    expect(NOTHING_PAYS).toBe("Nothing here pays at 300 ms. Don't trade this, or get closer.");
    expect(LABEL_TEXT.candidate).toBe('survives here; worth a real small-order test.');
    expect(LABEL_TEXT.negative).toBe("loses here. Don't trade this.");
    expect(LABEL_TEXT.dead_on_paper).toBe('even first in line, the money is locked too long to be worth it.');
    expect(FEW_TRADES).toBe('few trades carry this row');
    expect(footnote(34)).toBe("34 rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.");
  });

  it('writes the headline per side at sp-idle with the 10 bips figure beside it', () => {
    const rows = [
      sweetRow({ label: 'negative' }),
      sweetRow({ dimension: 'days', bucket: '1-7', label: 'inconclusive' }),
      sweetRow({ fee_bips: 10, label: 'negative' }),
    ];
    expect(headline(series(rows, { fees: [{ fee_bips: 10, captured: '-1', mean: '-0.1', label: 'negative' }] }), 'no'))
      .toBe('NO: 0 candidates in 2 tested rows (at 10 bips: 0).');
    const cands = [sweetRow({ dimension: 'category', bucket: 'sports', label: 'candidate' }), sweetRow({ dimension: 's', bucket: '2', label: 'candidate' })];
    expect(headline(series(cands), 'no')).toBe('NO: 2 candidates: sports, 2 legs.');
    expect(headline(series([]), 'yes')).toBe('YES: 0 candidates in 0 tested rows.');
  });

  it('lays out buckets by scenarios and formats intervals as money', () => {
    const rows = [
      sweetRow({ dimension: 'days', bucket: '7-30', scenario: 'colo-idle', latency_ms: 52 }),
      sweetRow({ dimension: 'days', bucket: 'lt1', scenario: 'sp-idle', latency_ms: 300, mean: '0.1', lo: '-0.02', hi: '0.22' }),
      sweetRow({ dimension: 'days', bucket: 'lt1', scenario: 'sp-idle', fee_bips: 10 }),
    ];
    const g = grid(series(rows), 'no', 'days');
    expect(g.buckets).toEqual(['lt1', '7-30']);
    expect(g.scenarios.map((s) => s.name)).toEqual(['colo-idle', 'sp-idle']);
    expect(interval(g.cell('lt1', 'sp-idle')!)).toBe('+$0.10 [−$0.02, +$0.22]');
    expect(g.cell('7-30', 'sp-idle')).toBeUndefined();
  });
});
```
Check the exact output of `formatMoney` for `'-0.02'` in `web/src/lib/money.test.ts` (the minus sign it uses, and whether small values keep two decimals). Write the expected string in this test from what `formatMoney(v, { signed: true })` returns there, not from this plan.

- [ ] **Step 3: Implement `web/src/lib/sweet.ts`**

```ts
import type { SweetLabel, SweetRow, SweetSeries } from '../api/types';
import { formatMoney } from './money';

/** Spec 5d §7.6, verbatim. */
export const SWEET_CAVEAT =
  "Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.";
export const NOTHING_PAYS = "Nothing here pays at 300 ms. Don't trade this, or get closer.";
export const SWEET_EMPTY = 'No sweet spots yet. analyze has not finished a tick under these premises.';
export const FEW_TRADES = 'few trades carry this row';
export const HEADLINE_SCENARIO = 'sp-idle';
export const COLO_TITLE = 'a co-located trader';

export const LABEL_TEXT: Record<SweetLabel, string> = {
  candidate: 'survives here; worth a real small-order test.',
  negative: "loses here. Don't trade this.",
  inconclusive: 'could go either way at this sample size.',
  thin: 'too few trades or events to say anything.',
  not_tested: 'no paper account trades this.',
  dead_on_paper: 'even first in line, the money is locked too long to be worth it.',
};
export const LABEL_TAG: Record<SweetLabel, string> = {
  candidate: 'CANDIDATE',
  negative: 'NEGATIVE',
  inconclusive: 'UNSURE',
  thin: 'THIN',
  not_tested: 'NOT TESTED',
  dead_on_paper: 'DEAD ON PAPER',
};

export type SweetSide = 'no' | 'yes';
export type SweetDim = 'all' | 'category' | 'days' | 's' | 'hour';
export const DIMS: [SweetDim, string][] = [
  ['all', 'ALL'],
  ['category', 'CATEGORY'],
  ['days', 'DAYS'],
  ['s', 'S'],
  ['hour', 'HOUR'],
];

const BUCKET_TEXT: Record<string, string> = {
  all: 'all',
  lt1: '< 1 day',
  '1-7': '1–7 days',
  '7-30': '7–30 days',
  '30-90': '30–90 days',
  '90-365': '90–365 days',
  ge365: '≥ 365 days',
  unknown: 'unknown',
  gt10: '> 10 legs',
};

/** "sports", "2 legs", "1–7 days", "00–04 UTC". */
export function bucketText(dim: string, bucket: string): string {
  if (dim === 's') return BUCKET_TEXT[bucket] ?? `${bucket.replace('-', '–')} legs`;
  if (dim === 'hour') return `${bucket.replace('-', '–')} UTC`;
  return BUCKET_TEXT[bucket] ?? bucket;
}

export function footnote(m: number): string {
  return `${m} rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.`;
}

const tested = (r: SweetRow) => r.label !== 'thin' && r.label !== 'not_tested';

/** Spec 5d §8: one sentence per side at the headline scenario, the 10 bips figure beside it (NO). */
export function headline(s: SweetSeries, side: SweetSide): string {
  const rows = s.rows.filter((r) => r.side === side && r.scenario === HEADLINE_SCENARIO && r.fee_bips === 0);
  const cands = rows.filter((r) => r.label === 'candidate');
  const fee10 = s.fees.find((f) => f.fee_bips === 10);
  const fee = side === 'no' && fee10 ? ` (at 10 bips: ${fee10.label === 'candidate' ? 1 : 0})` : '';
  const SIDE = side.toUpperCase();
  if (cands.length === 0) return `${SIDE}: 0 candidates in ${rows.filter(tested).length} tested rows${fee}.`;
  const named = cands
    .filter((r) => r.dimension !== 'all')
    .slice(0, 3)
    .map((r) => bucketText(r.dimension, r.bucket));
  const list = named.length > 0 ? `: ${named.join(', ')}` : '';
  return `${SIDE}: ${cands.length} ${cands.length === 1 ? 'candidate' : 'candidates'}${list}${fee}.`;
}

/** True when both sides were tested at the headline scenario and neither has a candidate. */
export function nothingPays(s: SweetSeries): boolean {
  const rows = s.rows.filter((r) => r.scenario === HEADLINE_SCENARIO && r.fee_bips === 0);
  return rows.some(tested) && !rows.some((r) => r.label === 'candidate');
}

export interface Grid {
  scenarios: { name: string; latency_ms: number | null }[];
  buckets: string[];
  cell: (bucket: string, scenario: string) => SweetRow | undefined;
}

/** Rows = buckets (in the order the api sent them), columns = scenarios by latency; fee rows are left out. */
export function grid(s: SweetSeries, side: SweetSide, dim: SweetDim): Grid {
  const rows = s.rows.filter((r) => r.side === side && r.dimension === dim && r.fee_bips === 0);
  const buckets: string[] = [];
  const scen = new Map<string, number | null>();
  for (const r of rows) {
    if (!buckets.includes(r.bucket)) buckets.push(r.bucket);
    scen.set(r.scenario, r.latency_ms);
  }
  const scenarios = [...scen.entries()]
    .map(([name, latency_ms]) => ({ name, latency_ms }))
    .sort((a, b) => (a.latency_ms ?? Infinity) - (b.latency_ms ?? Infinity) || a.name.localeCompare(b.name));
  const byKey = new Map(rows.map((r) => [`${r.bucket}|${r.scenario}`, r]));
  return { scenarios, buckets, cell: (b, sc) => byKey.get(`${b}|${sc}`) };
}

/** "+$0.10 [−$0.02, +$0.22]"; "—" without a mean. Money through formatMoney only. */
export function interval(r: SweetRow): string {
  if (r.mean === null) return '—';
  const m = formatMoney(r.mean, { signed: true });
  if (r.lo === null || r.hi === null) return m;
  return `${m} [${formatMoney(r.lo, { signed: true })}, ${formatMoney(r.hi, { signed: true })}]`;
}
```
The buckets come sorted from the api (Task 7 sorts rows by dimension, then bucket order), so `grid` keeps the first-seen order. The `sort` on latency uses numbers from the api's integer `latency_ms`, which is geometry and not money, so it's allowed.

- [ ] **Step 4: Run the helper tests**

Run: `cd web && npx vitest run src/lib/sweet.test.ts`
Expected: PASS.

- [ ] **Step 5: Failing component tests**

`web/src/components/SweetSpots.test.tsx`:
```tsx
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { sweetFixture, sweetRow } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { SWEET_CAVEAT, SWEET_EMPTY } from '../lib/sweet';
import { SweetSpots } from './SweetSpots';

const renderBox = () => render(<MemoryRouter><SweetSpots /></MemoryRouter>);

describe('SweetSpots', () => {
  it('asks for 30D first, always shows the caveat, the headline, the table and the footnote', async () => {
    const rows = [
      sweetRow({ scenario: 'colo-idle', latency_ms: 52, label: 'candidate', mean: '0.2', lo: '0.05', hi: '0.35', n_exec: 40, clusters: 33 }),
      sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.1', lo: '-0.2', hi: '-0.01', n_exec: 41, clusters: 35, few_trades: true }),
    ];
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [sweetFixture(rows)] }) });
    renderBox();
    expect(await screen.findByText(SWEET_CAVEAT)).toBeInTheDocument();
    expect(String(fetch.mock.calls[0][0])).toBe('/api/cards?win=30d');
    expect(screen.getByText('NO: 0 candidates in 1 tested rows.')).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: /a co-located trader/ })).toBeInTheDocument();
    expect(screen.getByText('CANDIDATE')).toBeInTheDocument();
    expect(screen.getByText('few trades carry this row')).toBeInTheDocument();
    expect(screen.getByText(/rows tested; each interval is widened/)).toBeInTheDocument();
  });

  it('switches side and dimension, hides CATEGORY without tags, and says so when empty', async () => {
    mockApi({ '/api/cards': { win: '30d', cards: [sweetFixture([sweetRow({ side: 'yes', label: 'dead_on_paper' })], { category_available: false })] } });
    renderBox();
    const dims = await screen.findByRole('group', { name: 'dimension' });
    expect(within(dims).queryByRole('button', { name: 'CATEGORY' })).toBeNull();
    await userEvent.click(within(screen.getByRole('group', { name: 'side' })).getByRole('button', { name: 'YES' }));
    expect(screen.getByText('DEAD ON PAPER')).toBeInTheDocument();
  });

  it('shows the empty state when analyze has no SWEET card yet', async () => {
    mockApi({ '/api/cards': { win: '30d', cards: [] } });
    renderBox();
    expect(await screen.findByText(SWEET_EMPTY)).toBeInTheDocument();
  });
});
```

- [ ] **Step 6: Implement the component, the help entries and the page**

`web/src/components/WindowSwitch.tsx`: give `WindowSwitch` an optional `label = 'window'` prop used as the group's `aria-label`. Existing callers keep `'window'`; SweetSpots passes `'sweet spots window'`, so the Paper page has two distinguishable groups.

`web/src/help.ts`: add to `HELP`:
```ts
  sweet: {
    title: 'What is a sweet spot?',
    text: 'A slice of the paper attempts (a category, a time to resolution, a basket size, an hour of day) where the edge we actually captured, after fees and waiting for our money, stays above zero even at the edge of its interval. Almost nothing qualifies. That is the point of looking.',
  },
  sweetThin: {
    title: 'Why are most rows thin?',
    text: 'A row needs at least 30 executed attempts spread over at least 30 event-hours before it gets any verdict. Weeks of data spread over about 200 rows leave most of them short.',
  },
  sweetBonferroni: {
    title: 'What is Bonferroni, in one line?',
    text: 'Test enough rows and one looks good by luck; each interval is widened by the number of rows tested, so a false candidate shows up less than 1 time in 20 overall.',
  },
  sweetClusters: {
    title: 'Why count events, not trades?',
    text: 'One event can open many episodes in an hour, and those trades share the same books and the same competitors. Counting them as independent would make the intervals look tighter than they are.',
  },
  sweetYes: {
    title: 'Why is YES beyond 30 days not tested?',
    text: 'The YES paper account only buys complete baskets that resolve within 30 days and clear a 10% annualized edge. Anything else it skips, so there is nothing captured to measure; the row shows the edge on the table and whether even that clears the hurdle.',
  },
```
`web/src/components/SweetSpots.tsx`:
```tsx
import { useState } from 'react';
import { api } from '../api/client';
import type { SweetSeries } from '../api/types';
import { useApi } from '../api/useApi';
import { useTicker } from '../lib/hooks';
import { formatDecimal } from '../lib/money';
import {
  COLO_TITLE, DIMS, FEW_TRADES, LABEL_TAG, LABEL_TEXT, NOTHING_PAYS, SWEET_CAVEAT, SWEET_EMPTY,
  type SweetDim, type SweetSide, bucketText, footnote, grid, headline, interval, nothingPays,
} from '../lib/sweet';
import { Box } from './Box';
import { EmptyState } from './EmptyState';
import { Help } from './Help';
import { Loadable } from './Loadable';
import { Tag } from './Tag';
import { type Win, WindowSwitch } from './WindowSwitch';

/** Phase 5d spec §8: below Edge capture on the Paper page; its own window switch, 30D by default. */
export function SweetSpots() {
  const [win, setWin] = useState<Win>('30d');
  const [side, setSide] = useState<SweetSide>('no');
  const [dim, setDim] = useState<SweetDim>('all');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  return (
    <Box title="SWEET SPOTS" help="sweet">
      <p className="quip">{SWEET_CAVEAT}</p>
      <WindowSwitch win={win} onChange={setWin} label="sweet spots window" />
      <Loadable state={st} what="sweet spots">
        {(d) => {
          const s = d.cards.find((c) => c.card === 'SWEET')?.series.sweet;
          return s ? <SweetBody s={s} side={side} dim={dim} onSide={setSide} onDim={setDim} /> : <EmptyState text={SWEET_EMPTY} />;
        }}
      </Loadable>
    </Box>
  );
}

function SweetBody({ s, side, dim, onSide, onDim }: {
  s: SweetSeries; side: SweetSide; dim: SweetDim; onSide: (v: SweetSide) => void; onDim: (v: SweetDim) => void;
}) {
  const dims = DIMS.filter(([d]) => d !== 'category' || s.category_available);
  const g = grid(s, side, dim === 'category' && !s.category_available ? 'all' : dim);
  return (
    <>
      <p>{headline(s, 'no')}</p>
      <p>{headline(s, 'yes')}</p>
      {nothingPays(s) && <p className="quip">{NOTHING_PAYS}</p>}
      <div className="seg" role="group" aria-label="side">
        {(['no', 'yes'] as const).map((v) => (
          <button key={v} type="button" className={v === side ? 'on' : undefined} aria-pressed={v === side} onClick={() => onSide(v)}>
            {v.toUpperCase()}
          </button>
        ))}
        {side === 'yes' && <Help id="sweetYes" />}
      </div>
      <div className="seg" role="group" aria-label="dimension">
        {dims.map(([d, label]) => (
          <button key={d} type="button" className={d === dim ? 'on' : undefined} aria-pressed={d === dim} onClick={() => onDim(d)}>
            {label}
          </button>
        ))}
      </div>
      <div className="scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">{dim === 'all' ? '' : dim.toUpperCase()}</th>
              {g.scenarios.map((sc) => (
                <th key={sc.name} scope="col">
                  {sc.name === 'colo-idle' ? `${COLO_TITLE} · ` : ''}
                  {sc.name}
                  {sc.latency_ms === null ? '' : ` · ${sc.latency_ms} ms`}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {g.buckets.map((b) => (
              <tr key={b}>
                <th scope="row">{bucketText(dim, b)}</th>
                {g.scenarios.map((sc) => {
                  const r = g.cell(b, sc.name);
                  if (!r) return <td key={sc.name}>—</td>;
                  const dimmed = r.label === 'thin' || r.label === 'not_tested';
                  const alt = r.captured_leftover0 !== r.captured ? `leftovers at 0: ${formatDecimal(r.captured_leftover0)} total` : undefined;
                  return (
                    <td key={sc.name} className={dimmed ? 'dim' : undefined} title={alt}>
                      <Tag tone={r.label === 'candidate' ? 'no' : r.label === 'negative' || r.label === 'dead_on_paper' ? 'flag' : 'plain'} title={LABEL_TEXT[r.label]}>
                        {LABEL_TAG[r.label]}
                      </Tag>{' '}
                      {interval(r)}
                      <br />
                      <span className="dim">n {r.n_exec} · C {r.clusters}</span>
                      {r.side === 'yes' && r.annualized !== null && <span className="dim"> · {formatDecimal(r.annualized)}/yr</span>}
                      {r.few_trades && (
                        <>
                          <br />
                          <span className="dim">{FEW_TRADES}</span>
                        </>
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="dim">
        {footnote(s.m)} z = {s.z}. <Help id="sweetBonferroni" /> <Help id="sweetThin" /> <Help id="sweetClusters" />
      </p>
    </>
  );
}
```
Two notes on the component:
- **Annualized figure:** `annualized` is a fraction per year. `formatDecimal` shows it as a fraction, such as `0.73/yr`. If the page's convention for rates is `%`, format it with `fmtValue` from `lib/cards.ts` after multiplying in decimal-string space. Don't use floats on money. A rate is not money, but keep it string-exact anyway.
- **Paper page:** in `web/src/pages/Paper.tsx`, render `<SweetSpots />` right after `<EdgeCapture />` and import it.

- [ ] **Step 7: Run the web tests, the build and the lint**

Run: `cd web && npx vitest run` then `npm run build` and `npm run lint`.
Expected: PASS. Several existing tests may now fail:
- **Paper page tests:** the mocked `/api/cards` also answers SweetSpots's `win=30d` request, and a second window group exists. Fix them by scoping queries with `within(screen.getByRole('group', { name: 'window' }))`, and by letting the mock route return the EDGE fixture for any `win`, as the Paper test does today (line 100).
- **Existing `WindowSwitch` tests:** they keep their default `'window'` label.

Check 360 px by hand in `npm run dev` with the browser's responsive mode: the page must not scroll sideways, and the table scrolls inside `.scroll`.

- [ ] **Step 8: Commit**

```bash
git add web/src/api/types.ts web/src/components/WindowSwitch.tsx web/src/help.ts web/src/pages/Paper.tsx web/src/test/fixtures.ts web/src/lib/sweet.ts web/src/lib/sweet.test.ts web/src/components/SweetSpots.tsx web/src/components/SweetSpots.test.tsx
git commit -m "web: Sweet spots box on the Paper page (headline, table by scenario, labels, caveat, Bonferroni footnote)"
```
Add any existing test file you had to scope (e.g. `web/src/pages/Paper.test.tsx`) to the same commit.

---

### Task 10: deploy: the Phase 5d VPS runbook

No code. The owner runs it. It follows the 5a runbook's mechanics (`docs/superpowers/runbooks/phase5a-cards-vps.md`) for building, bundling, shipping and loading images; only what differs is spelled out.

**Files:**
- Create: `docs/superpowers/runbooks/phase5d-sweet-vps.md`

- [ ] **Step 1: Write the runbook**

It has these sections, with commands:

1. **Benchmark (PC):**
   - `cargo test -p cards --test bench_sweet -- --ignored --nocapture` (< 50 ms, < 16 MB held);
   - `cargo test -p cards --test bench_tick -- --ignored --nocapture` (< 200 ms).
   - Record both in Results.
2. **Build (PC):** 5a runbook §3 unchanged (same keys; no new `.env` key in 5d): the `rust:dev` image (scanner + api + analyze) and the `web:dev` image. Record `GIT_SHA`.
3. **Bundle and ship:** 5a runbook §4 and §7 unchanged.
4. **Pre-flight (VPS):** record the current card keys, so you can prove after the deploy that no 5a card restarted:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT card, win, premise_key FROM card_results ORDER BY 1, 2" > ~/pre5d_keys.txt; wc -l ~/pre5d_keys.txt
```
   Back up the database as in 5a §5.
5. **Stop the scanner, migrate 0007, start again:**
```bash
docker compose stop --timeout 60 scanner analyze
docker compose --profile core run --rm migrate; echo "migrate exit=$?"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT max(version) FROM _sqlx_migrations WHERE success"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT has_table_privilege('analyze_rw', 'paper_ledger', 'SELECT'), has_table_privilege('analyze_rw', 'paper_ledger', 'INSERT')"
docker compose up -d --no-build --no-deps scanner analyze api web
```
   Expect:
   - `migrate exit=0`;
   - `7`;
   - `t|f`: `analyze_rw` can read `paper_ledger` but not write it.
6. **Post-deploy checks:**
   - **(GO) Tags arrive** after the first universe refresh, within about 10 minutes:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FILTER (WHERE tags IS NOT NULL), count(*) FROM events WHERE gone_ms IS NULL"
```
     Expect most listed events tagged.
   - **SWEET results** after the first tick and catch-up:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT win, status FROM card_results WHERE card = 'SWEET' ORDER BY 1"
```
     Expect `24h`, `7d`, `30d` and `all`: `under_review` while catch-up runs, then `live` or `warming`.
   - **No 5a card restarted:**
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT card, win, premise_key FROM card_results WHERE card <> 'SWEET' ORDER BY 1, 2" | diff - ~/pre5d_keys.txt && echo keys-unchanged
```
     Expect `keys-unchanged`. A diff in `premise_key` only would mean a 5a family's key moved: stop and roll back.
   - **Memory and CPU:** `docker stats --no-stream` for `analyze` after catch-up must show memory below 96 MB and CPU within its 0.25 cap. Record both.
   - **In the browser (signed in):**
     - the Paper page shows **SWEET SPOTS** below Edge capture, 30D selected, with the caveat, two headline sentences and the footnote;
     - switching to YES shows `NOT TESTED` or `DEAD ON PAPER` on the long-dated rows;
     - at 360 px the page does not scroll sideways.
7. **Exit (spec §1):** on the VPS, the box shows 7d / 30d / all, every row carries n, C, a label and the card hash, and `analyze` stays inside its caps. Record the date.
8. **Rollback:** start the previous `rust:dev` and `web:dev` image tags recorded in pre-flight (5a runbook Rollback). Migration 0007 stays: its column is nullable and unused by the old code, and its grants are read-only. SWEET rows left in `card_results` are ignored by the old web, which filters cards by paper id.
9. **Results:** blanks for `GIT_SHA`, the benchmark numbers, the tag counts, the SWEET statuses, `keys-unchanged`, the memory and CPU, and the date.

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/runbooks/phase5d-sweet-vps.md
git commit -m "runbook: Phase 5d on the VPS (migrate 0007, SWEET checks, 5a keys unchanged)"
```

---

## Results

Task 1 (go/no-go), filled in by the implementer:
- Date (UTC): 2026-09-27 07:03 UTC
- Events in sample / with tags: 100 / 100
- Decision (GO / NO-GO): GO
- Final bucket lists (GO only):
```text
politics : politics, us-politics, elections, us-election, trump, congress, senate, us-presidential-election, midterms, governor-midterms, trump-presidency, governor-tossups, main-election, primaries, senate-elections, us-government, house, supreme-court, courts
sports   : sports, nba, nfl, mlb, nhl, soccer, football, tennis, golf, mma, ufc, f1, boxing, esports, cricket, basketball
crypto   : crypto, bitcoin, ethereum, solana, crypto-prices, xrp, memecoins, token-launch, airdrops, stablecoins, crypto-listings, crypto-legal, pumpptfun, megaeth
economy  : economy, finance, business, fed, fed-rates, inflation, stocks, earnings, commodities, ipos, economic-policy, taxes, fiscal
world    : world, geopolitics, international-elections, middle-east, ukraine, china, europe, global-elections, foreign-policy, israel, world-elections, russia, nato, ukraine-map, world-affairs, gaza, international-election-props, brazil, france, russia-capture, macron, uk, starmer, uk-labour-leadership, india, eu, zelensky, us-iran, poland, syria, macro-geopolitics, israel-election
culture  : pop-culture, culture, entertainment, awards, music, movies, tv, celebrities, oscars, grammys, taylor-swift
tech     : tech, technology, ai, science, space, big-tech, openai, sam-altman, gpt-5
```
Sample distribution across these buckets (100 events, no clashes): politics 72, crypto 16, world 5, economy 3, tech 2, culture 1, sports 1, other 0. Excluded as ambiguous/structural (fall to `other`): `earn-4`, `pre-market`, `united-states`, `trump-zelenskyy`, `trump-putin`, `epstein`, `price-milestone`, `recurring`, `featured`, `parent-for-derivative`, `2025-predictions`, `exchange`, `resign`, `pedophile`, `england`, `security-guarantee`, `pregnant`, `military-action`, `other`, `trump-x-al-sharaa`, `hfc`, `president`, `rewards-20-4pt5-50`, `prediction-markets`, `legal`, `maxwell`, `ghislaine-maxwell`.
