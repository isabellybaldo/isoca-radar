import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { App } from '../App';
import type { BreakEven, Epochs, Ladder } from '../api/types';
import { useEpochs } from '../api/useEpochs';
import { RESET_COPY, RESET_OFF, RESET_QUEUED, ResetFold } from '../components/ResetFold';
import { SweetLadderGrid } from '../components/SweetLadderGrid';
import { EPISODE_DEF } from '../help';
import { nextAllowedText } from '../lib/epochs';
import { LADDER, NOT_MONOTONE, breakEvenText, ladderModel } from '../lib/ladder';
import { GLYPH_NAME, GLYPH_ORDER, GRID_HINT, LABEL_TEXT } from '../lib/sweet';
import {
  AXIS_TITLE, LADDER_CAPTION, MARKETS_CAPTION, MARKETS_MISSING, MARKETS_RECOMPUTING, MARKETS_WARMING, NO_RUNGS, TABLE_CAPTION,
} from '../schematics/LatencyLadder';
import { appRoutes } from '../test/appRoutes';
import { epochsFixture, ladderFixture, ladderLiveFixture, sweetRow } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { COPY, caughtText, onTable100, onTableDepth } from './copy';
import { checkHonesty, unlabelledMoney } from './honesty';

const PAGES = ['/', '/sweet', '/episodes', '/episodes/live-1-00000001', '/paper', '/papers', '/health'];

function scan(el: HTMLElement, where: string) {
  expect(checkHonesty(el.textContent ?? ''), where).toEqual([]);
  expect(unlabelledMoney(el), where).toEqual([]);
}

describe('checkHonesty and unlabelledMoney', () => {
  it('fail on the real-money words and on profit outside the existing phrases', () => {
    expect(checkHonesty('We earned $3 of income; it made money')).toEqual(['earned', 'income', 'made']);
    expect(checkHonesty('Earn more')).toEqual(['earn']);
    expect(checkHonesty('this latency is profitable')).toEqual(['profitable']);
    expect(checkHonesty('PROFIT @ $100, net profit at the peak, profit after taker fees, profit before taker fees')).toEqual([]);
    expect(checkHonesty('a nice profit')).toEqual(['profit']);
    expect(checkHonesty('learn the homemade way')).toEqual([]);
    expect(checkHonesty('basket gains, leg-risk cost, pays on paper, loses on paper')).toEqual([]);
  });

  it('find a money figure with no paper / on the table / captured at N ms label', () => {
    const ok = document.createElement('div');
    ok.innerHTML = '<p>On the table: <span>$1.00</span></p><table><caption>paper</caption><tbody><tr><td>$2.00</td></tr></tbody></table>';
    expect(unlabelledMoney(ok)).toEqual([]);
    const bad = document.createElement('div');
    bad.innerHTML = '<p>You have <b>$5.00</b></p>';
    expect(unlabelledMoney(bad)).toEqual(['You have $5.00']);
    // a label elsewhere in the panel does not cover a bare figure
    const panel = document.createElement('div');
    panel.innerHTML = '<p>paper money</p><div>$5.00</div><section>$6.00 <span>$7.00</span></section><table><tbody><tr><td>$8.00</td></tr></tbody></table>';
    expect(unlabelledMoney(panel)).toEqual(['$5.00', '$6.00 $7.00', '$8.00']);
    const bare = document.createElement('div');
    bare.innerHTML = 'paper $9.00';
    expect(unlabelledMoney(bare)).toEqual(['paper $9.00']);
  });
});

describe('every fixed sentence the panel, the grid and the reset fold can show (spec 7a §9.4)', () => {
  it('is honest', () => {
    const kinds: BreakEven[] = [
      { kind: 'thin' }, { kind: 'all' }, { kind: 'none' },
      { kind: 'at', latency_ms: 70, direction: 'loses_beyond' }, { kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true },
    ];
    const ladders: Ladder[] = [
      ...kinds.map((be) => ladderFixture({ break_even: be })),
      ladderFixture({ rungs: [], break_even: { kind: 'thin' } }),
      ladderFixture({ epoch: 0, start_ms: null, rungs: [], break_even: { kind: 'thin' } }),
      ladderFixture({ rungs: ladderFixture().rungs.map((r) => ({ ...r, latency_ms: null })), break_even: { kind: 'thin' } }),
    ];
    const words = [
      ...Object.values(COPY), caughtText(null), caughtText(4190), onTable100('0.33'), onTable100(null), onTableDepth('0.41'),
      ...ladders.flatMap((l) => Object.values(breakEvenText(ladderModel(l)))),
      LADDER_CAPTION, AXIS_TITLE, MARKETS_CAPTION, MARKETS_WARMING, MARKETS_RECOMPUTING, MARKETS_MISSING, NO_RUNGS, TABLE_CAPTION, NOT_MONOTONE,
      RESET_COPY, RESET_QUEUED, RESET_OFF, GRID_HINT, EPISODE_DEF, ...GLYPH_ORDER.map((k) => GLYPH_NAME[k]), ...Object.values(LABEL_TEXT),
    ];
    for (const w of words) expect(checkHonesty(w), w).toEqual([]);
  });
});

describe('the panel is honest on every page, for every question (spec 7a §9.4)', () => {
  for (const ladder of [ladderFixture(), ladderLiveFixture()]) {
    for (const p of PAGES) {
      it(`${p} with a ${ladder.break_even.kind} ladder`, async () => {
        const f = mockApi(appRoutes({ '/api/paper/ladder': ladder }));
        render(
          <MemoryRouter initialEntries={[p]}>
            <App />
          </MemoryRouter>,
        );
        await settle(f);
        fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
        const panel = screen.getByRole('complementary', { name: 'explain' });
        await within(panel).findByText(breakEvenText(ladderModel(ladder)).big);
        for (const b of [...panel.querySelectorAll<HTMLButtonElement>('button.explain-q')]) {
          if (b.getAttribute('aria-expanded') !== 'true') fireEvent.click(b);
          await settle(f);
          scan(panel, `${p} · ${b.textContent}`);
        }
      });
    }
  }

  it('Radar with an incomplete set selected', async () => {
    const f = mockApi(appRoutes());
    render(
      <MemoryRouter initialEntries={['/']}>
        <App />
      </MemoryRouter>,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'show 1 incomplete set' }));
    fireEvent.click(screen.getByRole('link', { name: 'Who wins the cup?' }).closest('tr') as HTMLTableRowElement);
    fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
    fireEvent.click(screen.getByRole('button', { name: 'How can this make money?' }));
    await settle(f);
    const panel = screen.getByRole('complementary', { name: 'explain' });
    expect(within(panel).getByRole('columnheader', { name: 'an outcome not in the set ✗' })).toBeInTheDocument();
    scan(panel, 'radar, incomplete set');
  });

  it('the Sweet grid legend and every detail line', () => {
    const labels = ['candidate', 'negative', 'inconclusive', 'thin', 'not_tested', 'dead_on_paper'] as const;
    const rows = labels.map((label, i) =>
      sweetRow({ scenario: LADDER[i].scenario, latency_ms: LADDER[i].latency_ms, label, mean: '0.1', lo: '-0.1', hi: '0.3', n_exec: 30, clusters: 30 }),
    );
    const { container } = render(<SweetLadderGrid rows={rows} side="no" dim="all" label="scan" />);
    const wrap = container.querySelector('.lgrid-wrap') as HTMLElement;
    for (const b of screen.getAllByRole('button')) {
      fireEvent.focus(b);
      scan(wrap, b.getAttribute('aria-label') ?? '');
    }
  });

  it('the reset fold, in every state', async () => {
    const NOW = Date.UTC(2026, 8, 30, 12, 0);
    function Harness() {
      const e = useEpochs();
      return <ResetFold epochs={e} now={NOW} />;
    }
    const states: [Epochs, number | null, string][] = [
      [epochsFixture(), null, 'idle'],
      [epochsFixture({ pending_request: { id: 1, requested_ms: NOW } }), null, 'queued'],
      [epochsFixture({ next_allowed_ms: Date.UTC(2099, 0, 1) }), null, 'next allowed'],
      [epochsFixture(), 503, 'off'],
    ];
    // D-09: say which state rendered before scanning its text.
    const rendered = (name: string) => {
      const form = screen.queryByRole('textbox', { name: 'type reset to confirm' });
      const queued = screen.queryByText(RESET_QUEUED);
      const off = screen.queryByText(RESET_OFF);
      const allowed = screen.queryByText(nextAllowedText(Date.UTC(2099, 0, 1)));
      if (name === 'idle') {
        expect(form).toBeInTheDocument();
        expect([queued, off, allowed]).toEqual([null, null, null]);
      } else if (name === 'queued') {
        expect(queued).toBeInTheDocument();
        expect([form, off, allowed]).toEqual([null, null, null]);
      } else if (name === 'next allowed') {
        expect(allowed).toBeInTheDocument();
        expect([form, queued, off]).toEqual([null, null, null]);
      } else {
        expect(off).toBeInTheDocument();
        expect([queued, allowed]).toEqual([null, null]);
      }
    };
    for (const [e, status, name] of states) {
      mockApi({ '/api/paper/epochs': e, '/api/paper/reset': () => jsonResponse(status ?? 202, { error: 'reset_disabled', detail: 'x' }) });
      const { container, unmount } = render(<Harness />);
      await screen.findByText(RESET_COPY);
      if (status !== null) {
        await userEvent.type(screen.getByRole('textbox', { name: 'type reset to confirm' }), 'reset');
        await userEvent.click(screen.getByRole('button', { name: 'RESET' }));
        await screen.findByText(RESET_OFF);
      }
      rendered(name);
      scan(container, `reset fold: ${name}`);
      unmount();
    }
  });
});
