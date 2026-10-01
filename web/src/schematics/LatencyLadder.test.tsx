import { render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Ladder, LadderRung, SweetRow } from '../api/types';
import { LADDER, type LadderIn, NOT_MONOTONE, type Rung, SLOTS, ladderModel, slotX } from '../lib/ladder';
import type { GridRow } from '../lib/sweet';
import { ladderFixture, ladderLiveFixture } from '../test/fixtures';
import { LADDER_CAPTION, LatencyLadder, MARKETS_CAPTION, MARKETS_MISSING, MARKETS_RECOMPUTING, MARKETS_WARMING } from './LatencyLadder';

const W = 700;
const draw = (o: Partial<Ladder> = {}) => render(<LatencyLadder model={ladderModel(ladderFixture(o))} />);
const num = (el: Element | null, a: string) => Number(el?.getAttribute(a));
const rungItems = () => within(screen.getByRole('list', { name: 'rungs' })).getAllByRole('listitem');

describe('LatencyLadder', () => {
  it('says the break-even big, marks it between its two rungs, and labels every rung in HTML', () => {
    const { container } = draw();
    expect(screen.getByText('≈ 70 ms')).toHaveClass('ll-big');
    expect(screen.getByText('Pays on paper below it, loses on paper beyond it.')).toBeInTheDocument();
    const be = container.querySelector('line.ll-be');
    expect(num(be, 'x1')).toBeGreaterThan(slotX(2) * W);
    expect(num(be, 'x1')).toBeLessThan(slotX(3) * W);
    expect(rungItems().map((li) => li.firstChild?.textContent)).toEqual(['0', '25', '52', '100', '200', '300', 'extras']);
    expect(rungItems().slice(0, 6).map((li) => li.lastChild?.textContent)).toEqual(['40', '40', '40', '40', '40', '40']);
    expect(screen.getByText('as of 21:14 UTC')).toBeInTheDocument();
    expect(screen.getByText('$ on paper since epoch 1 began, 2026-09-29 19:58 UTC')).toBeInTheDocument();
    expect(container.querySelector('svg.ll-svg')).toHaveAttribute('preserveAspectRatio', 'none');
    expect(container.querySelector('svg.ll-svg text')).toBeNull();
  });

  it('signs the bars: basket gains above zero, leg-risk cost below, unrealized hatched', () => {
    const { container } = draw();
    const zero = num(container.querySelector('line.ll-zero'), 'y1');
    const g = container.querySelector('g[data-scenario="colo-idle"]');
    const gains = g?.querySelector('rect.ll-gains') ?? null;
    expect(num(gains, 'y') + num(gains, 'height')).toBeCloseTo(zero);
    expect(num(g?.querySelector('rect.ll-leg_risk') ?? null, 'y')).toBeGreaterThanOrEqual(zero);
    const fill = g?.querySelector('rect.ll-unrealized')?.getAttribute('fill') ?? '';
    const id = /^url\(#(.+)\)$/.exec(fill)?.[1];
    expect(id).toMatch(/^ll-hatch-/);
    expect(container.querySelector(`pattern[id="${id}"]`)).not.toBeNull();
    // square at any stretch: a zero-length capped line, never a rect with a width that preserveAspectRatio="none" squashes
    const extras = container.querySelectorAll('line.ll-extra');
    expect(extras).toHaveLength(2);
    expect(container.querySelector('rect.ll-extra')).toBeNull();
    for (const x of extras) {
      expect(x.hasAttribute('width')).toBe(false);
      expect(num(x, 'x1')).toBe(num(x, 'x2'));
      expect(num(x, 'y1')).toBe(num(x, 'y2'));
      expect(num(x, 'x1')).toBeGreaterThan((6 / 7) * W);
    }
    const dot = g?.querySelector('circle.ll-net') ?? null; // D-13: the net marker is a dot
    expect(num(dot, 'cx')).toBeCloseTo(slotX(2) * W);
    expect(num(dot, 'cy')).toBeLessThan(zero); // colo-idle nets +3.10
    expect(container.querySelectorAll('circle.ll-net')).toHaveLength(6);
    expect(screen.getByText(/^sp-burst \(450 ms idle, more in bursts\):/)).toHaveTextContent('sp-burst (450 ms idle, more in bursts): −$80.10 on paper · n 40');
  });

  it('puts the us-band between the right rungs and names it; without a measurement there is no band', () => {
    const { container, unmount } = draw();
    const us = container.querySelector('rect.ll-us');
    expect(num(us, 'x')).toBeGreaterThan(slotX(4) * W);
    expect(num(us, 'x')).toBeLessThan(slotX(5) * W);
    expect(num(us, 'x') + num(us, 'width')).toBeGreaterThan(slotX(5) * W);
    expect(screen.getByText('us (measured ≈155 + matching)')).toBeInTheDocument();
    unmount();
    const r = draw({ measured: null });
    expect(r.container.querySelector('rect.ll-us')).toBeNull();
    expect(screen.getByText('us: not measured yet')).toBeInTheDocument();
  });

  it('keeps the us-band out of the extras slot and never lets it collapse (carry-forward from Task 2)', () => {
    const edge = (LADDER.length / SLOTS) * W; // the right edge of the 300 ms slot
    for (const latency_ms of [155, 250, 5000]) {
      const { container, unmount } = draw({ measured: { latency_ms, host: 'vps-kvm2' } });
      const us = container.querySelector('rect.ll-us');
      expect(num(us, 'x') + num(us, 'width'), `measured ${latency_ms}`).toBeLessThanOrEqual(edge + 1e-9);
      expect(num(us, 'width'), `measured ${latency_ms}`).toBeGreaterThanOrEqual(6);
      unmount();
    }
  });

  it('renders every break_even kind', () => {
    for (const [be, big] of [
      [{ kind: 'all' }, 'pays at every rung'],
      [{ kind: 'none' }, 'none'],
      [{ kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true }, '≈ 40 ms'],
    ] as const) {
      const r = draw({ break_even: be });
      expect(screen.getByText(big)).toHaveClass('ll-big');
      r.unmount();
    }
    const { container } = draw({ break_even: { kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true } });
    expect(screen.getByText(NOT_MONOTONE)).toBeInTheDocument();
    expect(screen.getByText('Loses on paper below it, pays on paper beyond it.')).toBeInTheDocument();
    expect(container.querySelector('line.ll-be')).not.toBeNull();
  });

  it('thin: "too few trades yet", each rung n/30, extras still drawn', () => {
    const { container } = render(<LatencyLadder model={ladderModel(ladderLiveFixture())} />);
    expect(screen.getByText('too few trades yet')).toHaveClass('ll-big');
    expect(rungItems().slice(0, 6).map((li) => li.lastChild?.textContent)).toEqual(['2 / 30', '2 / 30', '1 / 30', '1 / 30', '0 / 30', '0 / 30']);
    expect(screen.getByText('as of 20:09 UTC')).toBeInTheDocument();
    expect(container.querySelector('line.ll-be')).toBeNull();
    expect(container.querySelectorAll('line.ll-extra')).toHaveLength(2);
  });

  it('a rung with no account is an empty column labelled so, and the answer says why (review focus 1)', () => {
    const { container } = draw({ rungs: ladderFixture().rungs.filter((r) => r.scenario !== 'rung-100'), break_even: { kind: 'thin' } });
    expect(container.querySelector('rect.ll-missing[data-scenario="rung-100"]')).not.toBeNull();
    expect(container.querySelector('g[data-scenario="rung-100"]')).toBeNull();
    expect(screen.getByText('no account')).toBeInTheDocument();
    expect(screen.getByText('a rung has no account yet')).toHaveClass('ll-big');
    expect(screen.queryByText('too few trades yet')).toBeNull();
    expect(rungItems()).toHaveLength(7);
  });

  it('a fresh epoch and epoch 0 draw no NaN (review focus 3)', () => {
    const epoch0 = ladderFixture({ epoch: 0, start_ms: null, as_of_ms: null, rungs: [], measured: null, break_even: { kind: 'thin' } });
    for (const l of [ladderLiveFixture(), epoch0]) {
      const { container, unmount } = render(<LatencyLadder model={ladderModel(l)} />);
      for (const el of container.querySelectorAll('svg rect, svg line, svg circle')) {
        for (const a of ['x', 'y', 'width', 'height', 'x1', 'x2', 'y1', 'y2', 'cx', 'cy', 'r']) {
          const v = el.getAttribute(a);
          if (v !== null) expect(Number.isFinite(Number(v)), `${el.getAttribute('class')} ${a}=${v}`).toBe(true);
        }
      }
      unmount();
    }
    render(<LatencyLadder model={ladderModel(epoch0)} />);
    expect(screen.getByText('no epoch yet')).toHaveClass('ll-big');
    expect(screen.getAllByText('no account')).toHaveLength(6);
  });

  it('which markets: the compact grid for live and warming, words for under_review and missing; the time frame is named', () => {
    const a = draw();
    const fold = screen.getByText('which markets').closest('details') as HTMLDetailsElement;
    expect(fold.open).toBe(false);
    expect(within(fold).getByText(MARKETS_CAPTION)).toBeInTheDocument();
    expect(within(fold).getByRole('table', { name: 'which markets' })).toBeInTheDocument();
    expect(within(fold).getByRole('button', { name: 'crypto at rung-0 (0 ms): thin, mean > 0' })).toHaveTextContent('◐');
    a.unmount();
    const b = draw({ markets_status: 'warming' });
    expect(screen.getByText(MARKETS_WARMING)).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'which markets' })).toBeInTheDocument();
    b.unmount();
    const c = draw({ markets_status: 'under_review', markets: [] });
    expect(screen.getByText(MARKETS_RECOMPUTING)).toBeInTheDocument();
    expect(screen.queryByRole('table', { name: 'which markets' })).toBeNull();
    c.unmount();
    draw({ markets_status: 'missing', markets: [] });
    expect(screen.getByText(MARKETS_MISSING)).toBeInTheDocument();
  });

  it('names the unwind rule and keeps the numbers in a closed fold, labelled paper', () => {
    draw();
    expect(screen.getByText(LADDER_CAPTION)).toBeInTheDocument();
    const nums = screen.getByText('the numbers').closest('details') as HTMLDetailsElement;
    expect(nums.open).toBe(false);
    expect(within(nums).getAllByRole('row')).toHaveLength(9); // header + 6 rungs + 2 extras
    expect(within(nums).getByText(/^paper, since the epoch start/)).toBeInTheDocument();
    expect(screen.getAllByText('−$80.10')).toHaveLength(2); // extras line + the numbers fold
  });
});

/** A rung as the public snapshot carries it (spec 7b §5.1): no account id. */
const noAccount = (r: LadderRung): Rung => {
  const { account_id, ...rest } = r;
  void account_id;
  return rest;
};
/** A Sweet row with only the fields the grid reads. */
const gridOnly = (r: SweetRow): GridRow => ({
  side: r.side, scenario: r.scenario, fee_bips: r.fee_bips, latency_ms: r.latency_ms, dimension: r.dimension, bucket: r.bucket,
  label: r.label, n_exec: r.n_exec, clusters: r.clusters, mean: r.mean, lo: r.lo, hi: r.hi,
});

describe('LatencyLadder on the public snapshot (spec 7b §7.2)', () => {
  it('draws a ladder whose rungs carry no account_id, keyed by scenario (review focus 4)', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const l = ladderFixture();
    const input: LadderIn = { ...l, measured: { latency_ms: 155 }, rungs: l.rungs.map(noAccount), markets: l.markets.map(gridOnly) };
    const { container } = render(<LatencyLadder model={ladderModel(input)} />);
    expect(screen.getByText('≈ 70 ms')).toHaveClass('ll-big');
    expect(container.querySelectorAll('circle.ll-net')).toHaveLength(6);
    expect(container.querySelectorAll('line.ll-extra')).toHaveLength(2);
    expect(within(screen.getByRole('list', { name: 'extras' })).getAllByRole('listitem')).toHaveLength(2);
    expect(within(screen.getByRole('table', { name: 'which markets' })).getAllByRole('button').length).toBeGreaterThan(0);
    expect(error).not.toHaveBeenCalled();
    error.mockRestore();
  });
});
