// Engine figures come from 7b-A's fixtures (the sections start from fixture inputs) or from the engine for
// the page's own input; none is computed by hand here (R13).
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { formatDecimal } from '../lib/money';
import { CROSSED } from '../schematics/MirrorBooks';
import { endText } from '../schematics/ClassifyTree';
import { HAVE_PKG, WASM_REQUIRED, fixtureCase, loadTestEngine, skipReason } from '../wasm/testEngine';
import type { ClassifyModel, Engine, MirrorModel, PayoffMatrix } from '../wasm/types';
import { SECTIONS } from './copy';
import { CLASSIFY_START, ClassifySection, classifyInput } from './sections/ClassifySection';
import { MIRROR_START, MirrorSection } from './sections/MirrorSection';
import { PAYOFF_START, PayoffSection, payoffInput } from './sections/PayoffSection';

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('sections a–c'));

const lit = (c: HTMLElement) => [...c.querySelectorAll('li.cl-node.lit')].map((li) => li.getAttribute('data-node'));

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('sections a–c on the real engine (09-27 §8)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });

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
  });

  it('a: the frame has the quip and the caption naming the real functions (R7)', () => {
    render(<MirrorSection engine={engine} />);
    expect(screen.getByText(SECTIONS.a.quip)).toBeInTheDocument();
    expect(screen.getByText(SECTIONS.a.caption)).toBeInTheDocument();
    expect(SECTIONS.a.caption).toContain('arb_core::book::d1_sums');
    expect(SECTIONS.a.caption).toContain('arb_core::detect::canary');
  });

  it('a: crossing the book raises the alert, and reset clears it', async () => {
    render(<MirrorSection engine={engine} />);
    fireEvent.click(screen.getByRole('button', { name: 'cross the book' }));
    expect(await screen.findByText(CROSSED)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'reset' }));
    await waitFor(() => expect(screen.queryByText(CROSSED)).toBeNull());
  });

  it("a: a size of 0 is the engine's sentence, and the page keeps working (review focus 2)", async () => {
    const { container } = render(<MirrorSection engine={engine} />);
    fireEvent.change(screen.getByLabelText('YES bid 1 size'), { target: { value: '0' } });
    expect(await screen.findByRole('alert')).toHaveTextContent('size "0" must be above 0 and at most 1,000,000, with at most 6 decimals');
    expect(container.querySelector('svg.mb-svg')).toBeNull();
    fireEvent.change(screen.getByLabelText('YES bid 1 size'), { target: { value: '100' } });
    await waitFor(() => expect(container.querySelector('svg.mb-svg')).not.toBeNull());
  });

  it('a: dragging a price past its neighbour is clamped, so the page never sends two levels at one price', async () => {
    const { container } = render(<MirrorSection engine={engine} />);
    const drag = (label: string, v: string) => fireEvent.change(screen.getByLabelText(label), { target: { value: v } });
    drag('YES bid 1 price', '51');
    drag('YES bid 4 price', '90');
    drag('YES ask 1 price', '99');
    drag('YES ask 4 price', '1');
    await new Promise((r) => setTimeout(r, 40));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(container.querySelector('svg.mb-svg')).not.toBeNull();
  });

  it('b: the YES basket with Other unlisted breaks on Other; the NO basket never does; the implication has an impossible column', async () => {
    render(<PayoffSection engine={engine} />);
    expect(PAYOFF_START).toMatchObject({ kind: 'yes', s: 3, listed: false, priced: false });
    expect(screen.getByRole('columnheader', { name: 'Other ✗' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'NO basket' }));
    await waitFor(() => expect(screen.queryByRole('columnheader', { name: /✗/ })).toBeNull());
    fireEvent.click(screen.getByRole('button', { name: 'implication A ⇒ B' }));
    expect(await screen.findByRole('columnheader', { name: 'A∧¬B (impossible)' })).toBeInTheDocument();
  });

  it('b: S goes 2 to 6 and the engine accepts each', async () => {
    render(<PayoffSection engine={engine} />);
    for (const k of [2, 4, 5, 6, 3]) {
      fireEvent.click(screen.getByRole('button', { name: `S ${k}` }));
      await waitFor(() => expect(screen.getByRole('button', { name: `S ${k}` })).toHaveAttribute('aria-pressed', 'true'));
      await new Promise((r) => setTimeout(r, 40));
      expect(screen.queryByRole('alert')).toBeNull();
      expect(screen.getAllByRole('columnheader').length).toBeGreaterThan(k);
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
    expect(screen.getByText(/on the table/)).toBeInTheDocument();
    expect(SECTIONS.b.caption).toContain('arb_core::payoff');
    expect(SECTIONS.b.caption).toContain('arb_core::fill::walk');
  });

  it('b: the implication offers only the two prices the engine reads (R10), and the edge is the engine’s', async () => {
    render(<PayoffSection engine={engine} />);
    fireEvent.click(screen.getByRole('button', { name: 'implication A ⇒ B' }));
    fireEvent.click(screen.getByLabelText('with prices'));
    const sliders = await screen.findAllByRole('slider');
    expect(sliders.map((s) => s.getAttribute('aria-label'))).toEqual(['A bid (NO_A costs 1 − bid)', 'B ask']);
    expect(screen.queryByLabelText('“Other” is listed')).toBeNull();
    const edge = () => screen.getByText('A bid − B ask =').parentElement?.querySelector('.m-big')?.textContent;
    const before = edge();
    expect(before).toBeTruthy();
    fireEvent.change(sliders[1], { target: { value: '40' } });
    await waitFor(() => expect(edge()).not.toBe(before));
    const input = payoffInput({ ...PAYOFF_START, kind: 'implication', priced: true, impl: [PAYOFF_START.impl[0], 40] });
    expect(input.s).toBe(2);
    expect(input.prices).toHaveLength(2);
    const m = JSON.parse(engine.payoff_json(JSON.stringify(input))) as PayoffMatrix;
    expect(edge()).toBe(formatDecimal(m.top_edge_gross));
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

  it('c: the start is fixture no-edge-3 and lights its path; a leg closed YES ends where the engine says (R17)', async () => {
    const fx = fixtureCase('classify_json', 'no-edge-3');
    expect(classifyInput(CLASSIFY_START)).toEqual(JSON.parse(fx.input));
    const m = JSON.parse(fx.output) as ClassifyModel;
    const { container } = render(<ClassifySection engine={engine} />);
    expect(lit(container)).toEqual(m.path);
    expect(container.querySelector('.cl-end')).toHaveTextContent('NO basket · via the converter');
    expect(container.querySelector('.cl-net')).toHaveTextContent(`Q* ${m.q_star}`);
    fireEvent.change(screen.getByLabelText('leg A status'), { target: { value: 'closed_yes' } });
    const legs = [{ ...CLASSIFY_START.legs[0], status: 'closed_yes' as const }, ...CLASSIFY_START.legs.slice(1)];
    const closed = JSON.parse(engine.classify_json(JSON.stringify(classifyInput({ ...CLASSIFY_START, legs })))) as ClassifyModel;
    await waitFor(() => expect(lit(container)).toEqual(closed.path));
    expect(container.querySelector('.cl-end')).toHaveTextContent(endText(closed));
  });

  it('c: the post_end note is on the page, static, and the caption names arb_core::classify::classify', () => {
    render(<ClassifySection engine={engine} />);
    expect(screen.getByText(/the scanner adds it from event dates and it never changes the kind/)).toBeInTheDocument();
    expect(SECTIONS.c.caption).toContain('arb_core::classify::classify');
    expect(SECTIONS.c.caption).not.toContain('detect::classify');
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
    fireEvent.change(screen.getByLabelText('leg A best bid'), { target: { value: '10' } });
    fireEvent.click(screen.getByRole('button', { name: 'reset' }));
    const m = JSON.parse(fixtureCase('classify_json', 'no-edge-3').output) as ClassifyModel;
    await waitFor(() => expect(lit(container)).toEqual(m.path));
    expect(screen.getByLabelText(/augmented/)).not.toBeChecked();
  });
});
