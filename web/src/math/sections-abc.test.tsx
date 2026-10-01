// Engine figures come from 7b-A's fixtures (the sections start from fixture inputs) or from the engine for
// the page's own input; none is computed by hand here (R13). No fixed sleeps: every wait is a waitFor/findBy.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync, readFileSync } from 'node:fs';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { formatDecimal } from '../lib/money';
import { CROSSED } from '../schematics/MirrorBooks';
import { endText } from '../schematics/ClassifyTree';
import { HAVE_PKG, WASM_REQUIRED, fixtureCase, loadTestEngine, skipReason } from '../wasm/testEngine';
import type { ClassifyModel, Engine, MirrorModel, PayoffMatrix } from '../wasm/types';
import { IMPLICATION_PRICE_USE } from './adapt';
import { B_QUIPS, SECTIONS } from './copy';
import { toTick } from './defaults';
import { CLASSIFY_START, ClassifySection, classifyInput } from './sections/ClassifySection';
import { MIRROR_START, MirrorSection } from './sections/MirrorSection';
import { PAYOFF_START, PayoffSection, payoffInput } from './sections/PayoffSection';

declare const process: { cwd(): string };

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('sections a–c'));

const lit = (c: HTMLElement) => [...c.querySelectorAll('li.cl-node.lit')].map((li) => li.getAttribute('data-node'));
const change = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });

describe('captions name functions that exist in the Rust code (R7)', () => {
  const root = `${process.cwd()}/../crates`;
  const DIR = { arb_core: 'arb-core', sim: 'sim' } as const;
  const refs = Object.values(SECTIONS).flatMap((s) =>
    [...s.caption.matchAll(/\b(arb_core|sim)::([a-z_]+)(?:::(\{[^}]*\}|[A-Za-z_0-9]+))?/g)].flatMap((m) => {
      const names = m[3] === undefined ? [null] : m[3].replace(/[{}]/g, '').split(',').map((n) => n.trim());
      return names.map((name) => ({ crate: m[1] as keyof typeof DIR, mod: m[2], name }));
    }),
  );

  it('finds references in every caption', () => {
    for (const [k, s] of Object.entries(SECTIONS)) expect(/\b(arb_core|sim)::/.test(s.caption), k).toBe(true);
    expect(refs.length).toBeGreaterThan(10);
  });

  it.each(refs.map((r) => [`${r.crate}::${r.mod}${r.name === null ? '' : `::${r.name}`}`, r] as const))('%s exists', (_label, r) => {
    const file = `${root}/${DIR[r.crate]}/src/${r.mod}.rs`;
    expect(existsSync(file), file).toBe(true);
    if (r.name !== null) expect(readFileSync(file, 'utf8')).toMatch(new RegExp(`pub\\s+(?:fn|struct|trait|enum)\\s+${r.name}\\b`));
  });
});

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('sections a–c on the real engine (09-27 §8)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });
  /** The engine with every mirror_json request recorded, to inspect what the page really sent. */
  const spied = () => {
    const sent: string[] = [];
    return { sent, engine: { ...engine, mirror_json: (i: string) => (sent.push(i), engine.mirror_json(i)) } as Engine };
  };
  const lastBook = (sent: string[]) => JSON.parse(sent.at(-1) ?? '{}') as { bids: { p: string }[]; asks: { p: string }[] };
  const ticks = (rows: { p: string }[]) => rows.map((l) => toTick(l.p));

  it('a: the start is fixture fed-book, and the page draws its output (review focus 4)', () => {
    const fx = fixtureCase('mirror_json', 'fed-book');
    expect(MIRROR_START).toEqual(JSON.parse(fx.input));
    expect(engine.mirror_json(JSON.stringify(MIRROR_START))).toBe(fx.output);
    const m = JSON.parse(fx.output) as MirrorModel;
    render(<MirrorSection engine={engine} />);
    expect(screen.getByRole('heading', { name: 'a · one book, twice' })).toBeInTheDocument();
    expect(screen.getByText(`D1 long ${m.d1_long}`)).toBeInTheDocument();
    expect(screen.getByText(`short ${m.d1_short}`)).toBeInTheDocument();
    expect(screen.getByText(`spread ${m.spread}`)).toBeInTheDocument();
    expect(screen.getByText(SECTIONS.a.quip)).toBeInTheDocument();
    expect(screen.getByText(SECTIONS.a.caption)).toBeInTheDocument();
  });

  it('a: crossing the book raises the alert, and reset clears it', async () => {
    render(<MirrorSection engine={engine} />);
    fireEvent.click(screen.getByRole('button', { name: 'cross the book' }));
    expect(await screen.findByText(CROSSED)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'reset' }));
    await waitFor(() => expect(screen.queryByText(CROSSED)).toBeNull());
  });

  it("a: a size of 0 is the engine's sentence (fixture size-zero), and the page keeps working (review focus 2)", async () => {
    const sentence = (JSON.parse(fixtureCase('mirror_json', 'size-zero').output) as { error: string }).error;
    const { container } = render(<MirrorSection engine={engine} />);
    change('YES bid 1 size', '0');
    expect(await screen.findByRole('alert')).toHaveTextContent(sentence);
    expect(container.querySelector('svg.mb-svg')).toBeNull();
    change('YES bid 1 size', '100');
    await waitFor(() => expect(container.querySelector('svg.mb-svg')).not.toBeNull());
  });

  it('a: every price slider has room to move (none is pinned by its neighbours)', () => {
    render(<MirrorSection engine={engine} />);
    for (const side of ['bid', 'ask']) {
      for (let i = 1; i <= 4; i++) {
        const el = screen.getByLabelText(`YES ${side} ${i} price`);
        expect(Number(el.getAttribute('max')) - Number(el.getAttribute('min')), `${side} ${i}`).toBeGreaterThan(50);
      }
    }
  });

  it('a: dragging an inner level pushes its neighbours, and the engine always gets a strict order', async () => {
    const { sent, engine: spy } = spied();
    render(<MirrorSection engine={spy} />);
    change('YES bid 3 price', '60'); // bids were 54 53 52 51
    await waitFor(() => expect(ticks(lastBook(sent).bids)).toEqual([62, 61, 60, 51]));
    expect(screen.queryByRole('alert')).toBeNull();
    change('YES bid 2 price', '40'); // pushes the levels behind it down
    await waitFor(() => expect(ticks(lastBook(sent).bids)).toEqual([62, 40, 39, 38]));
    change('YES ask 2 price', '70'); // asks were 55 56 57 58
    await waitFor(() => expect(ticks(lastBook(sent).asks)).toEqual([55, 70, 71, 72]));
    expect(screen.queryByRole('alert')).toBeNull();
    for (const raw of sent) {
      const b = JSON.parse(raw) as { bids: { p: string }[]; asks: { p: string }[] };
      const bt = ticks(b.bids);
      const at = ticks(b.asks);
      expect(bt.every((t, k) => k === 0 || t < bt[k - 1]), raw).toBe(true);
      expect(at.every((t, k) => k === 0 || t > at[k - 1]), raw).toBe(true);
    }
  });

  it('a: dragging to the extremes keeps every level inside 0.01–0.99 and in order', async () => {
    const { sent, engine: spy } = spied();
    render(<MirrorSection engine={spy} />);
    change('YES bid 1 price', '99');
    await waitFor(() => expect(ticks(lastBook(sent).bids)).toEqual([99, 53, 52, 51]));
    change('YES bid 1 price', '1');
    await waitFor(() => expect(ticks(lastBook(sent).bids)).toEqual([4, 3, 2, 1]));
    change('YES ask 4 price', '1');
    await waitFor(() => expect(ticks(lastBook(sent).asks)).toEqual([1, 2, 3, 4]));
    change('YES ask 1 price', '99');
    await waitFor(() => expect(ticks(lastBook(sent).asks)).toEqual([96, 97, 98, 99]));
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('b: the YES basket with Other unlisted breaks on Other; the NO basket never does; the implication has an impossible column', async () => {
    render(<PayoffSection engine={engine} />);
    expect(PAYOFF_START).toMatchObject({ kind: 'yes', s: 3, listed: false, priced: false });
    expect(payoffInput(PAYOFF_START)).toEqual(JSON.parse(fixtureCase('payoff_json', 'yes-3-other').input));
    expect(screen.getByRole('columnheader', { name: 'Other ✗' })).toBeInTheDocument();
    expect(screen.getByText(B_QUIPS.yes)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'NO basket' }));
    await waitFor(() => expect(screen.queryByRole('columnheader', { name: /✗/ })).toBeNull());
    expect(screen.getByText(B_QUIPS.no)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'implication A ⇒ B' }));
    expect(await screen.findByRole('columnheader', { name: 'A∧¬B (impossible)' })).toBeInTheDocument();
    expect(screen.getByText(B_QUIPS.implication)).toBeInTheDocument();
  });

  it('b: S goes 2 to 6 and the engine accepts each, with exactly S+1 columns while Other is unlisted', async () => {
    render(<PayoffSection engine={engine} />);
    for (const k of [2, 4, 5, 6, 3]) {
      fireEvent.click(screen.getByRole('button', { name: `S ${k}` }));
      await waitFor(() => expect(screen.getAllByRole('columnheader').filter((h) => /^[A-F]$|^Other/.test(h.textContent ?? ''))).toHaveLength(k + 1));
      expect(screen.queryByRole('alert')).toBeNull();
    }
    expect(screen.queryByRole('button', { name: 'S 7' })).toBeNull();
  });

  it("b: with prices, the formula line carries the engine's top_edge_gross for the page's input", async () => {
    render(<PayoffSection engine={engine} />);
    fireEvent.click(screen.getByLabelText('“Other” is listed'));
    fireEvent.click(screen.getByLabelText('with prices'));
    const m = JSON.parse(engine.payoff_json(JSON.stringify(payoffInput({ ...PAYOFF_START, listed: true, priced: true })))) as PayoffMatrix;
    expect(m.top_edge_gross).not.toBeNull();
    expect(await screen.findByText('1 − Σ asks =')).toBeInTheDocument();
    expect(screen.getByText(formatDecimal(m.top_edge_gross))).toHaveClass('m-big');
    expect(screen.getByText(/before fees, on the table/)).toBeInTheDocument();
    expect(screen.queryByText(/not risk-free/)).toBeNull();
  });

  it('b: the formula line says so when a column breaks, and when the edge is not positive', async () => {
    render(<PayoffSection engine={engine} />);
    fireEvent.click(screen.getByLabelText('with prices'));
    expect(await screen.findByText(/not risk-free: see ✗/)).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('“Other” is listed'));
    await waitFor(() => expect(screen.queryByText(/not risk-free/)).toBeNull());
    change('leg A ask', '90');
    const input = payoffInput({ ...PAYOFF_START, listed: true, priced: true, ticks: [90, ...PAYOFF_START.ticks.slice(1)] });
    const m = JSON.parse(engine.payoff_json(JSON.stringify(input))) as PayoffMatrix;
    expect(m.top_edge_gross?.startsWith('-')).toBe(true);
    expect(await screen.findByText(/nothing on the table/)).toBeInTheDocument();
    expect(screen.getByText(formatDecimal(m.top_edge_gross))).toHaveClass('m-big');
  });

  it('b: the implication offers only the two prices the engine reads (R10), both move the edge, and it is the engine’s', async () => {
    render(<PayoffSection engine={engine} />);
    fireEvent.click(screen.getByRole('button', { name: 'implication A ⇒ B' }));
    fireEvent.click(screen.getByLabelText('with prices'));
    const formula = await screen.findByText('A bid − B ask =');
    const sliders = screen.getAllByRole('slider');
    expect(sliders.map((s) => s.getAttribute('aria-label'))).toEqual(IMPLICATION_PRICE_USE.map((u) => u.label));
    expect(IMPLICATION_PRICE_USE[0].label).toContain('1 − bid');
    expect(screen.queryByLabelText('“Other” is listed')).toBeNull();
    const edge = () => formula.parentElement?.querySelector('.m-big')?.textContent;
    const seen = [edge()];
    fireEvent.change(sliders[1], { target: { value: '40' } });
    await waitFor(() => expect(edge()).not.toBe(seen[0]));
    seen.push(edge());
    fireEvent.change(sliders[0], { target: { value: '60' } });
    await waitFor(() => expect(edge()).not.toBe(seen[1]));
    const input = payoffInput({ ...PAYOFF_START, kind: 'implication', priced: true, impl: [60, 40] });
    expect(input.s).toBe(2);
    expect(input.prices).toHaveLength(2);
    const m = JSON.parse(engine.payoff_json(JSON.stringify(input))) as PayoffMatrix;
    await waitFor(() => expect(edge()).toBe(formatDecimal(m.top_edge_gross)));
  });

  it('b: reset restores YES, S 3, Other unlisted, no prices', async () => {
    render(<PayoffSection engine={engine} />);
    fireEvent.click(screen.getByRole('button', { name: 'NO basket' }));
    fireEvent.click(screen.getByRole('button', { name: 'S 5' }));
    fireEvent.click(screen.getByRole('button', { name: 'reset' }));
    await waitFor(() => expect(screen.getByRole('columnheader', { name: 'Other ✗' })).toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'YES basket' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'S 3' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('b: range inputs carry the shown decimal as aria-valuetext', async () => {
    render(<PayoffSection engine={engine} />);
    fireEvent.click(screen.getByLabelText('with prices'));
    expect((await screen.findByLabelText('leg A ask')).getAttribute('aria-valuetext')).toBe('0.30');
  });

  it('c: the start is fixture no-edge-3 and lights its path; a leg closed YES ends where the engine says (R17)', async () => {
    const fx = fixtureCase('classify_json', 'no-edge-3');
    expect(classifyInput(CLASSIFY_START)).toEqual(JSON.parse(fx.input));
    const m = JSON.parse(fx.output) as ClassifyModel;
    const { container } = render(<ClassifySection engine={engine} />);
    expect(lit(container)).toEqual(m.path);
    expect(container.querySelector('.cl-end')).toHaveTextContent(endText(m));
    expect(container.querySelector('.cl-net')).toHaveTextContent(`Q* ${m.q_star}`);
    change('leg A status', 'closed_yes');
    const legs = [{ ...CLASSIFY_START.legs[0], status: 'closed_yes' as const }, ...CLASSIFY_START.legs.slice(1)];
    const closed = JSON.parse(engine.classify_json(JSON.stringify(classifyInput({ ...CLASSIFY_START, legs })))) as ClassifyModel;
    await waitFor(() => expect(lit(container)).toEqual(closed.path));
    expect(container.querySelector('.cl-end')).toHaveTextContent(endText(closed));
  });

  it('c: the post_end note is on the page, static', () => {
    render(<ClassifySection engine={engine} />);
    expect(screen.getByText(/the scanner adds it from event dates and it never changes the kind/)).toBeInTheDocument();
  });

  it('c: a leg that is not live has its bid and ask disabled, with the reason shown', () => {
    render(<ClassifySection engine={engine} />);
    expect(screen.getByLabelText('leg A best bid')).toBeEnabled();
    expect(screen.queryByText('not live: its book is ignored')).toBeNull();
    change('leg A status', 'halted');
    expect(screen.getByLabelText('leg A best bid')).toBeDisabled();
    expect(screen.getByLabelText('leg A best ask')).toBeDisabled();
    expect(screen.getByLabelText('leg B best bid')).toBeEnabled();
    expect(screen.getByText('not live: its book is ignored')).toBeInTheDocument();
  });

  it('c: every status offered is one the engine accepts, on every leg', () => {
    for (const status of ['live', 'placeholder', 'other', 'halted', 'closed_no', 'closed_yes'] as const) {
      for (let i = 0; i < CLASSIFY_START.legs.length; i++) {
        const legs = CLASSIFY_START.legs.map((l, k) => (k === i ? { ...l, status } : l));
        for (const augmented of [false, true]) {
          expect(engine.classify_json(JSON.stringify(classifyInput({ legs, augmented })))).not.toContain('"error"');
        }
      }
    }
  });

  it('c: augmented reaches the engine, and reset restores the start', async () => {
    const { container } = render(<ClassifySection engine={engine} />);
    fireEvent.click(screen.getByLabelText(/augmented/));
    const aug = JSON.parse(engine.classify_json(JSON.stringify(classifyInput({ ...CLASSIFY_START, augmented: true })))) as ClassifyModel;
    await waitFor(() => expect(lit(container)).toEqual(aug.path));
    change('leg A best bid', '10');
    fireEvent.click(screen.getByRole('button', { name: 'reset' }));
    const m = JSON.parse(fixtureCase('classify_json', 'no-edge-3').output) as ClassifyModel;
    await waitFor(() => expect(lit(container)).toEqual(m.path));
    expect(screen.getByLabelText(/augmented/)).not.toBeChecked();
  });
});
