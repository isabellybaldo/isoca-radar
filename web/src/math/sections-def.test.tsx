// Engine figures come from 7b-A's fixtures, from the engine for the page's own input, or from the contract's
// stated rules; none is retyped (R13). No fixed sleeps: every wait is a waitFor/findBy on an exact expectation.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { formatAvg, formatDecimal, formatMoneyCents, signOf } from '../lib/money';
import { basketVerdict } from '../schematics/BasketLegs';
import { DepthStairs } from '../schematics/DepthStairs';
import { costDomain, tickText } from '../schematics/geo';
import { describeWasm, fixtureCase, loadTestEngine } from '../wasm/testEngine';
import type { BasketIn, BasketModel, DepthWalkIn, Engine, EpisodeIn, EpisodeModel, WalkModel } from '../wasm/types';
import { basketInput } from './basket';
import { E_QUIPS, F_QUIPS, SECTIONS } from './copy';
import { F_BOOKS, F_BURST, F_START, PRESETS, exactlyOne, toTick } from './defaults';
import { DEPTH_START, DepthSection, type DepthState, depthInput } from './sections/DepthSection';
import { EPISODE_START, EpisodeSection, episodeInput } from './sections/EpisodeSection';
import { LegMissSection } from './sections/LegMissSection';

/** The engine's one sentence in place of a drawing (Section's Out): a polite status with the error class, or null. */
const engineError = () => document.querySelector('p.error');
const legItems = () => within(screen.getByRole('list', { name: 'legs' })).getAllByRole('listitem');
const change = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });
/** What BasketLegs prints for leg i from the engine's own model (same formatters), never a retyped figure. */
const bought = (m: BasketModel, i: number, name: string) => `leg ${name}${formatDecimal(m.legs[i].buy.filled)} of ${formatDecimal(m.legs[i].buy.requested)}`;
const sold = (m: BasketModel, i: number) => `sold ${formatDecimal(m.legs[i].unwind.filled)} at ${formatAvg(m.legs[i].unwind.vwap)}`;
/** A slider by its full accessible name: the text shown beside it, and the legend of the group it sits in. */
const slider = (name: string) => {
  const el = screen.getByLabelText(name);
  return { text: el.closest('label')?.querySelector('span')?.textContent, group: el.closest('fieldset')?.querySelector('legend')?.textContent };
};
const ascending = (levels: { p: string }[]) => levels.every((x, k) => k === 0 || toTick(x.p) > toTick(levels[k - 1].p));

describeWasm('sections d–f on the real engine (7b §8)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });
  /** The engine with every request to `fn` recorded, to inspect what the page really sent. */
  const spied = (fn: 'depth_walk_json' | 'episode_json' | 'basket_json') => {
    const sent: string[] = [];
    return { sent, engine: { ...engine, [fn]: (i: string) => (sent.push(i), engine[fn](i)) } as Engine };
  };
  const allSent = <T,>(sent: string[]) => sent.map((s) => JSON.parse(s) as T);
  const walk = (i: DepthWalkIn) => JSON.parse(engine.depth_walk_json(JSON.stringify(i))) as WalkModel;

  describe('d', () => {
    it('starts from three legs of three levels with a positive edge, as the engine sees it', () => {
      expect(DEPTH_START.legs).toHaveLength(3);
      for (const l of DEPTH_START.legs) expect(l).toHaveLength(3);
      const m = walk(depthInput(DEPTH_START));
      expect(signOf(m.pi)).toBeGreaterThan(0);
      expect(signOf(m.q_star)).toBeGreaterThan(0);
      expect(m.steps.length).toBeGreaterThan(0);
    });

    it("draws the engine's walk for its start; \"exactly 1.00\" leaves nothing to take", async () => {
      const m = walk(depthInput(DEPTH_START));
      const { container } = render(<DepthSection engine={engine} />);
      expect(screen.getByText(`Q* ${m.q_star}`)).toBeInTheDocument();
      expect(container.querySelectorAll('rect.ds-pi').length).toBe(m.steps.filter((s) => s.profitable && s.cost !== s.payoff).length);
      expect(screen.getByText(SECTIONS.d.caption)).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'exactly 1.00' }));
      const none = walk(depthInput({ ...DEPTH_START, legs: exactlyOne(DEPTH_START.legs) }));
      expect(await screen.findByText(`Q* ${none.q_star}`)).toBeInTheDocument();
      expect(signOf(none.q_star)).toBe(0);
    });

    it('"exactly 1.00" sends best asks summing to 1.00, every leg ascending, and the engine accepts it', async () => {
      const { sent, engine: spy } = spied('depth_walk_json');
      render(<DepthSection engine={spy} />);
      fireEvent.click(screen.getByRole('button', { name: 'exactly 1.00' }));
      const best = () => allSent<DepthWalkIn>(sent).at(-1)?.legs.reduce((a, l) => a + toTick(l.levels[0].p), 0);
      await waitFor(() => expect(best()).toBe(100));
      for (const l of (allSent<DepthWalkIn>(sent).at(-1) as DepthWalkIn).legs) expect(ascending(l.levels)).toBe(true);
      expect(engineError()).toBeNull();
    });

    it('dragging a level past its neighbour pushes the neighbours: the request stays strictly ascending (R8)', async () => {
      const { sent, engine: spy } = spied('depth_walk_json');
      render(<DepthSection engine={spy} />);
      const ticks = (leg: number) => (allSent<DepthWalkIn>(sent).at(-1) as DepthWalkIn).legs[leg].levels.map((l) => toTick(l.p));
      change('leg A level 1 price', '40'); // A was 30 32 35
      await waitFor(() => expect(ticks(0)).toEqual([40, 41, 42]));
      change('leg B level 3 price', '5'); // B was 31 33 36: pulled below its neighbours
      await waitFor(() => expect(ticks(1)).toEqual([3, 4, 5]));
      change('leg C level 2 price', '99'); // the top clamps, the first keeps its place
      await waitFor(() => expect(ticks(2)).toEqual([33, 98, 99]));
      expect(engineError()).toBeNull();
      for (const i of allSent<DepthWalkIn>(sent)) for (const l of i.legs) expect(ascending(l.levels), JSON.stringify(l)).toBe(true);
    });

    it('"exactly 1.00" is off, with the reason, when the other legs already leave no room for it', async () => {
      render(<DepthSection engine={engine} />);
      expect(screen.getByRole('button', { name: 'exactly 1.00' })).toBeEnabled();
      change('leg B level 1 price', '97');
      change('leg C level 1 price', '97');
      await waitFor(() => expect(screen.getByRole('button', { name: 'exactly 1.00' })).toBeDisabled());
      expect(screen.getByText(/out of reach.*nearest the grid allows is Σ 1\.95/)).toBeInTheDocument();
    });

    it('zooms the cost axis for the start, "exactly 1.00", one step only and fees on: every line inside, most of the height used (Task 16 V2)', () => {
      const cases: [string, DepthState][] = [
        ['start', DEPTH_START],
        ['exactly 1.00', { ...DEPTH_START, legs: exactlyOne(DEPTH_START.legs) }],
        ['one step', { ...DEPTH_START, legs: DEPTH_START.legs.map((l) => l.slice(0, 1)) }],
        ['fees on', { ...DEPTH_START, fees: true }],
      ];
      for (const [name, st] of cases) {
        const m = walk(depthInput(st));
        expect(m.steps.length, name).toBeGreaterThan(0);
        if (name === 'one step') expect(m.steps, name).toHaveLength(1);
        const d = costDomain(m.steps.flatMap((s) => [s.cost, s.payoff]));
        const { container, unmount } = render(<DepthStairs model={m} />);
        expect([...container.querySelectorAll('.ds-yaxis li')].map((li) => li.textContent), name).toEqual([tickText(d.hi), tickText(d.lo)]);
        const ys = [...container.querySelectorAll('line.ds-cost, line.ds-pay')].map((l) => Number(l.getAttribute('y1')));
        expect(ys, name).toHaveLength(2 * m.steps.length);
        for (const y of ys) {
          expect(y, name).toBeGreaterThan(0);
          expect(y, name).toBeLessThan(120);
        }
        expect(Math.max(...ys) - Math.min(...ys), name).toBeGreaterThanOrEqual(0.6 * 120); // the old axis from 0 left it about a tenth
        unmount();
      }
    });

    it('groups the level sliders by leg: the legend says the leg once, each slider shows the rest, the full name stays accessible (Task 16 V5)', () => {
      render(<DepthSection engine={engine} />);
      expect(slider('leg A level 1 price')).toEqual({ text: 'level 1 price', group: 'leg A' });
      expect(slider('leg C level 3 price')).toEqual({ text: 'level 3 price', group: 'leg C' });
    });

    it('every level slider has room to move', () => {
      render(<DepthSection engine={engine} />);
      for (const leg of ['A', 'B', 'C']) {
        for (const n of [1, 2, 3]) {
          const el = screen.getByLabelText(`leg ${leg} level ${n} price`);
          expect(Number(el.getAttribute('max')) - Number(el.getAttribute('min')), `${leg}${n}`).toBeGreaterThan(50);
        }
      }
    });

    it('fees on send the crypto fee on every leg; legs go 2 to 4 only', async () => {
      const { sent, engine: spy } = spied('depth_walk_json');
      render(<DepthSection engine={spy} />);
      fireEvent.click(screen.getByLabelText(/taker fees on/));
      const last = () => allSent<DepthWalkIn>(sent).at(-1) as DepthWalkIn;
      await waitFor(() => expect(last().legs.every((l) => l.fee?.rate === '0.07' && l.fee.exponent === 1)).toBe(true));
      fireEvent.click(screen.getByRole('button', { name: 'add a leg' }));
      await waitFor(() => expect(last().legs).toHaveLength(4));
      expect(screen.getByRole('button', { name: 'add a leg' })).toBeDisabled();
      expect(screen.getByLabelText('leg D level 1 price')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'remove a leg' }));
      fireEvent.click(screen.getByRole('button', { name: 'remove a leg' }));
      await waitFor(() => expect(last().legs).toHaveLength(2));
      expect(screen.getByRole('button', { name: 'remove a leg' })).toBeDisabled();
      expect(engineError()).toBeNull();
      expect(last().legs.every((l) => ascending(l.levels))).toBe(true);
    });
  });

  describe('e', () => {
    const model = (i: EpisodeIn) => JSON.parse(engine.episode_json(JSON.stringify(i))) as EpisodeModel;
    /** The caught line starts with these words; a miss must not have it. */
    const CAPTURED = /^captured [0-9]+ ms after/;
    const times = (m: EpisodeModel) => `opened at ${m.open_ms} ms · our order lands at ${m.t_x} ms · closed at ${m.close_ms} ms`;

    it('starts from the five updates with no basket, at sp-idle', () => {
      const i = episodeInput(EPISODE_START);
      expect(i.updates).toHaveLength(5);
      expect(i).not.toHaveProperty('basket');
      expect(i).not.toHaveProperty('gap_ms');
      expect(i.scenario).toMatchObject({ proc_ms: 5, net_ms: 145, match_ms: 150 });
    });

    it('sp-idle catches the example episode 300 ms after it opens, and says the exact ms', () => {
      const m = model(episodeInput(EPISODE_START));
      expect(m.caught).toBe(true);
      const { container } = render(<EpisodeSection engine={engine} />);
      expect(screen.getByText('caught')).toHaveClass('m-verdict');
      const after = `captured ${(m.t_x as number) - (m.open_ms as number)} ms after it opened:`;
      const line = screen.getByText((_, el) => el?.tagName === 'P' && (el.textContent ?? '').startsWith(after));
      expect(within(line).getByText(formatMoneyCents(m.captured, { signed: true }))).toBeInTheDocument();
      expect(line).toHaveClass("m-verdict-line");
      expect(screen.getByText(CAPTURED)).toBe(line); // the query the miss test uses finds the line when it is there
      expect(line).toHaveTextContent("on the table");
      expect(screen.getByText(times(m))).toBeInTheDocument();
      expect(screen.getByText(/caught is not filled/)).toBeInTheDocument();
      expect(container.querySelector('rect.db-bar.order')).not.toBeNull();
      expect(container.querySelector('figure.ubars')).not.toBeNull();
    });

    it('its closing line follows the engine: caught says caught on paper and points at f', () => {
      expect(model(episodeInput(EPISODE_START)).caught).toBe(true);
      render(<EpisodeSection engine={engine} />);
      expect(screen.getByText('caught')).toHaveClass('m-verdict');
      expect(screen.getByText(E_QUIPS.caught)).toHaveClass('quip');
      expect(E_QUIPS.caught).toBe('Caught on paper. Whether it fills is section f.');
      expect(screen.queryByText(E_QUIPS.missed)).toBeNull();
    });

    it('its closing line follows the engine: a miss says the edge did not wait', async () => {
      render(<EpisodeSection engine={engine} />);
      change('net ms', '1000');
      change('match ms', '1000');
      expect(model(episodeInput({ ...EPISODE_START, sc: { proc_ms: 5, net_ms: 1000, match_ms: 1000 } })).caught).toBe(false);
      expect(await screen.findByText('missed')).toHaveClass('m-verdict');
      expect(screen.getByText(E_QUIPS.missed)).toHaveClass('quip');
      expect(E_QUIPS.missed).toBe('The edge was real. It just did not wait.');
      expect(screen.queryByText(E_QUIPS.caught)).toBeNull();
    });

    it('an engine error shows its one sentence and no closing line', async () => {
      const broken = { ...engine, episode_json: () => JSON.stringify({ error: 'no updates today' }) } as Engine;
      const { container } = render(<EpisodeSection engine={broken} />);
      expect(await screen.findByRole('status')).toHaveTextContent('no updates today');
      expect(engineError()).toHaveTextContent('no updates today');
      expect(container.querySelector('p.quip')).toBeNull();
    });

    it('1,000 ms of net and match misses it, and the exact ms say why', async () => {
      render(<EpisodeSection engine={engine} />);
      change('net ms', '1000');
      change('match ms', '1000');
      const m = model(episodeInput({ ...EPISODE_START, sc: { proc_ms: 5, net_ms: 1000, match_ms: 1000 } }));
      expect(m.caught).toBe(false);
      expect(await screen.findByText('missed')).toHaveClass('m-verdict');
      expect(screen.getByText(times(m))).toBeInTheDocument();
      expect(screen.queryByText(CAPTURED)).toBeNull();
    });

    it('the presets are the ladder rungs: picking one moves the three sliders to its values', async () => {
      render(<EpisodeSection engine={engine} />);
      for (const p of PRESETS) {
        fireEvent.click(screen.getByRole('button', { name: p.name }));
        await waitFor(() => expect(screen.getByLabelText('match ms')).toHaveValue(String(p.match_ms)));
        expect(screen.getByLabelText('proc ms')).toHaveValue(String(p.proc_ms));
        expect(screen.getByLabelText('net ms')).toHaveValue(String(p.net_ms));
        expect(screen.getByRole('button', { name: p.name })).toHaveAttribute('aria-pressed', 'true');
      }
    });

    it('never sends gap_ms; the secondsDelay toggle sends seconds_delay 1; scrubbing sends fewer updates', async () => {
      const { sent, engine: spy } = spied('episode_json');
      render(<EpisodeSection engine={spy} />);
      const last = () => allSent<EpisodeIn>(sent).at(-1) as EpisodeIn;
      fireEvent.click(screen.getByLabelText(/secondsDelay/));
      await waitFor(() => expect(last().seconds_delay).toBe(1));
      change('updates so far', '3');
      await waitFor(() => expect(last().updates).toHaveLength(3));
      for (const i of allSent<EpisodeIn>(sent)) expect(Object.keys(i)).not.toContain('gap_ms');
      expect(engineError()).toBeNull();
    });
  });

  describe('f', () => {
    it('the start is fixture no-all-fill and says its net cash; the usual burst is no-one-leg-empty-burst', async () => {
      const all = fixtureCase('basket_json', 'no-all-fill');
      const burst = fixtureCase('basket_json', 'no-one-leg-empty-burst');
      expect(engine.basket_json(JSON.stringify(basketInput(F_BOOKS, F_START)))).toBe(all.output);
      expect(engine.basket_json(JSON.stringify(basketInput(F_BOOKS, F_BURST)))).toBe(burst.output);
      render(
        <LegMissSection engine={engine}>
          <p>live part</p>
        </LegMissSection>,
      );
      expect(screen.getByText(basketVerdict(JSON.parse(all.output) as BasketModel).big)).toHaveClass('bl-big');
      expect(screen.getByText('live part')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'the usual burst' }));
      const b = JSON.parse(burst.output) as BasketModel;
      expect(await screen.findByText(basketVerdict(b).big)).toHaveClass('bl-big');
      expect(legItems()[1]).toHaveTextContent(bought(b, 1, 'B'));
      expect(legItems()[0]).toHaveTextContent(sold(b, 0));
      expect(legItems()[2]).toHaveTextContent(sold(b, 2));
    });

    it('a slider under the legend "leg A" does not repeat it; the full name stays the accessible one (Task 16 V5)', () => {
      render(<LegMissSection engine={engine} />);
      expect(slider('leg A depth left at t_x')).toEqual({ text: 'depth left at t_x', group: 'leg A' });
      expect(slider('leg B unwind book against us')).toEqual({ text: 'unwind book against us', group: 'leg B' });
      expect(slider('shares to buy')).toEqual({ text: 'shares to buy', group: undefined }); // no group, nothing to drop
    });

    it('leg C at 20 % at t_x (fixture no-one-leg-partial) fills 20 of 50', async () => {
      render(<LegMissSection engine={engine} />);
      const part = JSON.parse(fixtureCase('basket_json', 'no-one-leg-partial').output) as BasketModel;
      change('leg C depth left at t_x', '20');
      await waitFor(() => expect(legItems()[2]).toHaveTextContent(bought(part, 2, 'C')));
    });

    it('its line says only what the engine says: all filled, a leg caught short, nothing filled', async () => {
      render(<LegMissSection engine={engine} />);
      expect(screen.getByText(F_QUIPS.filled)).toHaveClass('quip');
      fireEvent.click(screen.getByRole('button', { name: 'the usual burst' }));
      expect(await screen.findByText(F_QUIPS.caught)).toBeInTheDocument();
      expect(screen.queryByText(F_QUIPS.filled)).toBeNull();
      for (const l of ['A', 'B', 'C']) change(`leg ${l} depth left at t_x`, '0');
      expect(await screen.findByText(F_QUIPS['nothing filled'])).toBeInTheDocument();
      const zero = JSON.parse(engine.basket_json(JSON.stringify(basketInput(F_BOOKS, { ...F_BURST, depth: [0, 0, 0] })))) as BasketModel;
      expect(screen.getByText(basketVerdict(zero).big)).toBeInTheDocument();
    });

    it('sends the drained depth, the unwind book moved against us, the share count and the fee', async () => {
      const { sent, engine: spy } = spied('basket_json');
      render(<LegMissSection engine={spy} />);
      const last = () => allSent<BasketIn>(sent).at(-1) as BasketIn;
      change('leg B depth left at t_x', '0');
      await waitFor(() => expect(last().legs[1].t_x.bids).toEqual([]));
      expect(last().legs[1].open).toEqual(F_BOOKS[1]);
      change('leg A unwind book against us', '7');
      await waitFor(() => expect(last().legs[0].unwind.asks).toEqual([{ p: '0.48', s: '100' }]));
      expect(last().legs[0].unwind.bids).toEqual(F_BOOKS[0].bids);
      change('shares to buy', '80');
      await waitFor(() => expect(last().q_request).toBe('80'));
      fireEvent.click(screen.getByLabelText(/taker fees on/));
      await waitFor(() => expect(last().legs.every((l) => l.fee?.rate === '0.07')).toBe(true));
      expect(engineError()).toBeNull();
      expect(Object.keys(last()).sort()).toEqual(['kind', 'legs', 'q_request']);
    });

    it('an engine error shows its one sentence and no deadpan line', async () => {
      const broken = { ...engine, basket_json: () => JSON.stringify({ error: 'no books today' }) } as Engine;
      const { container } = render(<LegMissSection engine={broken} />);
      expect(await screen.findByRole('status')).toHaveTextContent('no books today');
      expect(engineError()).toHaveTextContent('no books today'); // what the "no engine error" checks in this file look for
      expect(screen.queryByRole('alert')).toBeNull(); // a status, so a slider does not make it read out again on every step
      expect(container.querySelector('p.quip')).toBeNull();
    });

    it('the largest unwind move and a drained book are accepted by the engine', async () => {
      render(<LegMissSection engine={engine} />);
      for (const l of ['A', 'B', 'C']) change(`leg ${l} unwind book against us`, '20');
      change('leg A depth left at t_x', '0');
      const m = JSON.parse(engine.basket_json(JSON.stringify(basketInput(F_BOOKS, { ...F_START, depth: [0, 100, 100], against: [20, 20, 20] })))) as BasketModel;
      await waitFor(() => expect(legItems()[0]).toHaveTextContent(bought(m, 0, 'A')));
      expect(engineError()).toBeNull();
    });
  });
});
