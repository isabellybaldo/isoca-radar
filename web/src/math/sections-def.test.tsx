// Engine figures come from 7b-A's fixtures, from the engine for the page's own input, or from the contract's
// stated rules; none is retyped (R13). No fixed sleeps: every wait is a waitFor/findBy on an exact expectation.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { formatMoneyCents, signOf } from '../lib/money';
import { basketVerdict } from '../schematics/BasketLegs';
import { HAVE_PKG, WASM_REQUIRED, fixtureCase, loadTestEngine, skipReason } from '../wasm/testEngine';
import type { BasketIn, BasketModel, DepthWalkIn, Engine, EpisodeIn, EpisodeModel, WalkModel } from '../wasm/types';
import { basketInput } from './basket';
import { F_QUIPS, SECTIONS } from './copy';
import { F_BOOKS, F_BURST, F_START, PRESETS, toTick } from './defaults';
import { DEPTH_START, DepthSection, depthInput } from './sections/DepthSection';
import { EPISODE_START, EpisodeSection, episodeInput } from './sections/EpisodeSection';
import { LegMissSection } from './sections/LegMissSection';

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('sections d–f'));

const legItems = () => within(screen.getByRole('list', { name: 'legs' })).getAllByRole('listitem');
const change = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });
const ascending = (levels: { p: string }[]) => levels.every((x, k) => k === 0 || toTick(x.p) > toTick(levels[k - 1].p));

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('sections d–f on the real engine (7b §8)', () => {
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
      expect(await screen.findByText('Q* 0')).toBeInTheDocument();
    });

    it('"exactly 1.00" sends best asks summing to 1.00, every leg ascending, and the engine accepts it', async () => {
      const { sent, engine: spy } = spied('depth_walk_json');
      render(<DepthSection engine={spy} />);
      fireEvent.click(screen.getByRole('button', { name: 'exactly 1.00' }));
      const best = () => allSent<DepthWalkIn>(sent).at(-1)?.legs.reduce((a, l) => a + toTick(l.levels[0].p), 0);
      await waitFor(() => expect(best()).toBe(100));
      for (const l of (allSent<DepthWalkIn>(sent).at(-1) as DepthWalkIn).legs) expect(ascending(l.levels)).toBe(true);
      expect(screen.queryByRole('alert')).toBeNull();
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
      expect(screen.queryByRole('alert')).toBeNull();
      for (const i of allSent<DepthWalkIn>(sent)) for (const l of i.legs) expect(ascending(l.levels), JSON.stringify(l)).toBe(true);
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
      expect(screen.queryByRole('alert')).toBeNull();
      expect(last().legs.every((l) => ascending(l.levels))).toBe(true);
    });
  });

  describe('e', () => {
    const model = (i: EpisodeIn) => JSON.parse(engine.episode_json(JSON.stringify(i))) as EpisodeModel;
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
      expect(screen.getByText(/^captured at 300 ms:/)).toBeInTheDocument();
      const line = screen.getByText(/^captured at 300 ms:/);
      expect(within(line).getByText(formatMoneyCents(m.captured, { signed: true }))).toBeInTheDocument();
      expect(line).toHaveClass("m-verdict-line");
      expect(line).toHaveTextContent("on the table");
      expect(screen.getByText(times(m))).toBeInTheDocument();
      expect(screen.getByText(/caught is not filled/)).toBeInTheDocument();
      expect(container.querySelector('rect.db-bar.order')).not.toBeNull();
      expect(container.querySelector('figure.ubars')).not.toBeNull();
    });

    it('1,000 ms of net and match misses it, and the exact ms say why', async () => {
      render(<EpisodeSection engine={engine} />);
      change('net ms', '1000');
      change('match ms', '1000');
      const m = model(episodeInput({ ...EPISODE_START, sc: { proc_ms: 5, net_ms: 1000, match_ms: 1000 } }));
      expect(m.caught).toBe(false);
      expect(await screen.findByText('missed')).toHaveClass('m-verdict');
      expect(screen.getByText(times(m))).toBeInTheDocument();
      expect(screen.queryByText(/^captured at/)).toBeNull();
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
      expect(screen.queryByRole('alert')).toBeNull();
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
      expect(screen.getByText('filled, +$2.50 on paper')).toBeInTheDocument();
      expect(screen.getByText('live part')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'the usual burst' }));
      expect(await screen.findByText('caught, and lost $6.00 on paper')).toBeInTheDocument();
      expect(legItems()[1]).toHaveTextContent(/^leg B0 of 50/);
      expect(legItems()[0]).toHaveTextContent('sold 50 at 0.54');
      expect(legItems()[2]).toHaveTextContent('sold 50 at 0.64');
    });

    it('leg C at 20 % at t_x (fixture no-one-leg-partial) fills 20 of 50', async () => {
      render(<LegMissSection engine={engine} />);
      change('leg C depth left at t_x', '20');
      await waitFor(() => expect(legItems()[2]).toHaveTextContent(/^leg C20 of 50/));
    });

    it('its line says only what the engine says: all filled, a leg caught short, nothing filled', async () => {
      render(<LegMissSection engine={engine} />);
      expect(screen.getByText(F_QUIPS.filled)).toHaveClass('quip');
      fireEvent.click(screen.getByRole('button', { name: 'the usual burst' }));
      expect(await screen.findByText(F_QUIPS.caught)).toBeInTheDocument();
      expect(screen.queryByText(F_QUIPS.filled)).toBeNull();
      for (const l of ['A', 'B', 'C']) change(`leg ${l} depth left at t_x`, '0');
      expect(await screen.findByText(F_QUIPS.nothing)).toBeInTheDocument();
      expect(screen.getByText('nothing filled, $0.00 on paper')).toBeInTheDocument();
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
      expect(screen.queryByRole('alert')).toBeNull();
      expect(Object.keys(last()).sort()).toEqual(['kind', 'legs', 'q_request']);
    });

    it('the largest unwind move and a drained book are accepted by the engine', async () => {
      render(<LegMissSection engine={engine} />);
      for (const l of ['A', 'B', 'C']) change(`leg ${l} unwind book against us`, '20');
      change('leg A depth left at t_x', '0');
      await waitFor(() => expect(legItems()[0]).toHaveTextContent(/^leg A0 of 50/));
      expect(screen.queryByRole('alert')).toBeNull();
    });
  });
});
