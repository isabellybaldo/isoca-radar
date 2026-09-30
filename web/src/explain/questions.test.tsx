import { fireEvent, render, screen, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { EpisodeSummary } from '../api/types';
import { EPISODE_DEF } from '../help';
import { accountFixture, episodeDetailFixture, episodeSummaryFixture, ladderFixture, positionFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { COPY, caughtText } from './copy';
import { Explain } from './Explain';
import { type ExplainPage, ExplainProvider, type PageData, type Selection, useExplainPage, useExplainSelect } from './ExplainContext';
import { OTHER } from './models';

function Pub<P extends ExplainPage>({ page, data, sel }: { page: P; data: NonNullable<PageData[P]>; sel?: NonNullable<Selection> }) {
  useExplainPage(page, data);
  const select = useExplainSelect();
  return sel ? (
    <button type="button" onClick={() => select(sel)}>
      pick
    </button>
  ) : null;
}

/** Render a page stand-in with the panel, select (when asked), open the panel and one question. */
function ask(node: ReactElement, q: string): HTMLElement {
  mockApi({ '/api/paper/ladder': ladderFixture() });
  render(
    <MemoryRouter>
      <ExplainProvider>
        {node}
        <Explain />
      </ExplainProvider>
    </MemoryRouter>,
  );
  const pick = screen.queryByText('pick');
  if (pick) fireEvent.click(pick);
  fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
  fireEvent.click(screen.getByRole('button', { name: q }));
  return screen.getByRole('complementary', { name: 'explain' });
}

// Stable identities, as a page's useApi data.
const TOP = { open: [episodeSummaryFixture()] };
const NONE = { open: [] as EpisodeSummary[] };
const UNMEASURED = { open: [episodeSummaryFixture({ net_budget100: null })] };
const LIST = { items: [episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190 })] };
const YES_ROW = episodeSummaryFixture({ id: 'y', kind: 'yes_basket', s_legs: 3, event_title: 'Who wins the cup?', net_budget100: '1.2', net: '4.5' });
const INCOMPLETE = episodeSummaryFixture({ id: 'i', kind: 'yes_basket_incomplete', s_legs: 2, event_title: 'Best AI model?' });

describe('Radar: How can this make money?', () => {
  it('draws the top open row (a NO basket, S = 5) with its figures on the table', () => {
    const p = ask(<Pub page="radar" data={TOP} />, 'How can this make money?');
    const t = within(p).getByRole('table', { name: 'payoff of Bitcoin price on September 26?' });
    expect(within(t).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['pays $ if →', 'A', 'B', 'C', 'D', 'E']);
    expect(within(t).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['NO A', 'NO B', 'NO C', 'NO D', 'NO E', 'basket']);
    expect(within(p).getByText(COPY.makeMoneyNo)).toBeInTheDocument();
    expect(within(p).getByText('On the table at $100 spent: +$0.33, at the peak.')).toBeInTheDocument();
    expect(within(p).getByText('On the table at full depth: +$0.41.')).toBeInTheDocument();
  });

  it('uses the selected row: a YES basket', () => {
    const p = ask(<Pub page="radar" data={TOP} sel={{ kind: 'episode', episode: YES_ROW }} />, 'How can this make money?');
    expect(within(p).getByRole('table', { name: 'payoff of Who wins the cup?' })).toBeInTheDocument();
    expect(within(p).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['YES A', 'YES B', 'YES C', 'basket']);
    expect(within(p).getByText(COPY.makeMoneyYes)).toBeInTheDocument();
    expect(within(p).getByText('On the table at $100 spent: +$1.20, at the peak.')).toBeInTheDocument();
  });

  it('an incomplete set gets the ✗ column and no money figure', () => {
    const p = ask(<Pub page="radar" data={TOP} sel={{ kind: 'episode', episode: INCOMPLETE }} />, 'How can this make money?');
    expect(within(p).getByRole('columnheader', { name: `${OTHER} ✗` })).toBeInTheDocument();
    expect(within(p).getByText(COPY.makeMoneyIncomplete)).toBeInTheDocument();
    expect(within(p).queryByText(/On the table at/)).toBeNull();
  });

  it('an unmeasured figure reads "not measured", never $0.00', () => {
    const p = ask(<Pub page="radar" data={UNMEASURED} />, 'How can this make money?');
    expect(within(p).getByText('On the table at $100 spent: not measured, at the peak.')).toBeInTheDocument();
    expect(p).not.toHaveTextContent('$0.00');
  });

  it('with nothing open, a labelled example and no money figure', () => {
    const p = ask(<Pub page="radar" data={NONE} />, 'How can this make money?');
    expect(within(p).getByText(COPY.example)).toBeInTheDocument();
    expect(within(p).getByRole('table', { name: 'payoff of the example basket' })).toBeInTheDocument();
    expect(within(p).queryByText(/On the table at/)).toBeNull();
  });

  it('is asked on Radar only', () => {
    ask(<Pub page="episodes" data={LIST} />, 'What is an episode?');
    expect(screen.queryByRole('button', { name: 'How can this make money?' })).toBeNull();
  });
});

describe('Episodes: What is an episode?', () => {
  it('draws the first row next to the labelled §5.7 (e) example', () => {
    const p = ask(<Pub page="episodes" data={LIST} />, 'What is an episode?');
    expect(within(p).getByText('example: 3.80 s, from u2 to u5, peak at u3')).toBeInTheDocument();
    expect(within(p).getByText('Bitcoin price on September 26?: 4.19 s')).toBeInTheDocument();
    expect(within(p).getByText(EPISODE_DEF)).toBeInTheDocument();
    expect(p.querySelectorAll('rect.db-tick')).toHaveLength(4);
    expect(p.querySelectorAll('rect.db-bar')).toHaveLength(2);
  });

  it('a selected open row says it is still open', () => {
    const open = episodeSummaryFixture({ id: 'o', event_title: 'Still open one', opened_ms: Date.now() - 125_000, closed_ms: null, duration_ms: null });
    const p = ask(<Pub page="episodes" data={LIST} sel={{ kind: 'episode', episode: open }} />, 'What is an episode?');
    expect(within(p).getByText(/^Still open one: still open, 2 min \d+ s so far$/)).toBeInTheDocument();
  });
});

const DETAIL = { detail: episodeDetailFixture({ category: 'crypto' }) }; // a NO basket that lasted 4.19 s
const OPEN_DETAIL = { detail: episodeDetailFixture({ closed_ms: null, duration_ms: null }) };
const FAST = { detail: episodeDetailFixture({ duration_ms: 60, closed_ms: Date.UTC(2026, 8, 25, 1, 12, 4, 270) }) };
const POSITIONS = {
  accounts: [accountFixture()],
  positions: [
    positionFixture(),
    positionFixture({ market_id: 'm2', end_date: null, event_title: 'Who wins the cup?' }),
    positionFixture({ market_id: 'm3', returned: true, event_title: 'Returned one' }),
  ],
};
const NOTHING = { accounts: [accountFixture()], positions: [] };
const GONE = positionFixture({ market_id: 'm9', event_title: 'Left the list', basis: '7' });
const TODAY = { accounts: [], positions: [positionFixture({ end_date: new Date().toISOString().slice(0, 10) })] };
const NO_DETAIL = {};

describe('Episode detail: Would we have caught it?', () => {
  it('puts its duration on the ladder axis: every rung faster than it arrives in time', () => {
    const p = ask(<Pub page="episode" data={DETAIL} />, 'Would we have caught it?');
    const axis = within(p).getByRole('list', { name: 'rungs in time' });
    expect(within(axis).getAllByRole('listitem').map((li) => li.lastChild?.textContent)).toEqual(Array(6).fill('in time'));
    expect(within(p).getByText('It stayed on the table for 4.19 s: rungs faster than that arrive in time.')).toBeInTheDocument();
    expect(within(p).getByText(COPY.caughtNotFilled)).toBeInTheDocument();
    expect(p.querySelectorAll('svg.cb-svg')).toHaveLength(1);
    expect(within(p).queryByRole('table')).toBeNull();
  });

  it('a 60 ms episode: the rungs at 100 ms and beyond are too late', () => {
    const p = ask(<Pub page="episode" data={FAST} />, 'Would we have caught it?');
    const axis = within(p).getByRole('list', { name: 'rungs in time' });
    expect(within(axis).getAllByRole('listitem').map((li) => li.lastChild?.textContent)).toEqual([
      'in time', 'in time', 'in time', 'too late', 'too late', 'too late',
    ]);
    expect(p.querySelectorAll('rect.cb-rung.late')).toHaveLength(3);
    expect(within(p).getByText('It stayed on the table for 0.06 s: rungs faster than that arrive in time.')).toBeInTheDocument();
  });

  it('a still-open episode says so and every rung is in time so far', () => {
    const p = ask(<Pub page="episode" data={OPEN_DETAIL} />, 'Would we have caught it?');
    expect(within(p).getByText(caughtText(null))).toBeInTheDocument();
    expect(p.querySelectorAll('rect.cb-rung.in')).toHaveLength(6);
  });

  it('waits when the page has not published its episode', () => {
    const p = ask(<Pub page="episode" data={NO_DETAIL} />, 'Would we have caught it?');
    expect(within(p).getByText(COPY.caughtWaiting)).toBeInTheDocument();
  });
});

describe('Paper: Why is money locked?', () => {
  it('draws open positions as blocks sized by basis, each with days to resolution; returned ones are not locked', () => {
    const p = ask(<Pub page="paper" data={POSITIONS} />, 'Why is money locked?');
    const items = within(within(p).getByRole('list', { name: 'locked positions' })).getAllByRole('listitem').map((li) => li.textContent);
    expect(items).toHaveLength(2);
    expect(items[0]).toMatch(/^Fed decision in October\? · P3 · \$18\.00 locked on paper · resolves in \d+ days?$/);
    expect(items[1]).toBe('Who wins the cup? · P3 · $18.00 locked on paper · resolution date unknown');
    expect(p.querySelectorAll('.lk-block')).toHaveLength(2);
    expect(within(p).queryByText(/Returned one/)).toBeNull();
    expect(within(p).getByText(COPY.locked)).toBeInTheDocument();
  });

  it('highlights the selected position', () => {
    const p = ask(<Pub page="paper" data={POSITIONS} sel={{ kind: 'position', position: POSITIONS.positions[1] }} />, 'Why is money locked?');
    expect(p.querySelectorAll('.lk-block.sel')).toHaveLength(1);
    expect(within(p).getByText(/^Who wins the cup\?/).closest('li')).toHaveClass('sel');
    expect(p.querySelectorAll('.lk-block')).toHaveLength(2);
  });

  it('a selected position that has left the list stays drawn, first (a snapshot)', () => {
    const p = ask(<Pub page="paper" data={POSITIONS} sel={{ kind: 'position', position: GONE }} />, 'Why is money locked?');
    const items = within(within(p).getByRole('list', { name: 'locked positions' })).getAllByRole('listitem');
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent(/^Left the list · P3 · \$7\.00 locked on paper/);
    expect(items[0]).toHaveClass('sel');
    expect(p.querySelectorAll('.lk-block.sel')).toHaveLength(1);
  });

  it('a snapshot of a position that was since returned is not drawn as locked', () => {
    const returned = positionFixture({ market_id: 'm9', returned: true, event_title: 'Left the list' });
    const p = ask(<Pub page="paper" data={NOTHING} sel={{ kind: 'position', position: returned }} />, 'Why is money locked?');
    expect(within(p).getByText(COPY.nothingLocked)).toBeInTheDocument();
  });

  it('says so when nothing is locked', () => {
    const p = ask(<Pub page="paper" data={NOTHING} />, 'Why is money locked?');
    expect(within(p).getByText(COPY.nothingLocked)).toBeInTheDocument();
    expect(p.querySelectorAll('.lk-block')).toHaveLength(0);
  });

  it('an end date that is today is not past (D-07)', () => {
    const p = ask(<Pub page="paper" data={TODAY} />, 'Why is money locked?');
    expect(p).toHaveTextContent('resolves in 1 day');
    expect(p).not.toHaveTextContent('past its end date');
  });
});

describe('D-04: one drawing and at most two sentences per answer', () => {
  const sentences = (t: string) => t.split(/(?<=[.!?])\s+/).filter(Boolean).length;
  const answers: Record<string, string[]> = {
    'make money (yes)': [COPY.makeMoneyYes],
    'make money (no)': [COPY.makeMoneyNo],
    'make money (incomplete)': [COPY.makeMoneyIncomplete],
    'what is an episode': [COPY.whatEpisode, COPY.episodeBar],
    'caught (closed)': [caughtText(4190), COPY.caughtNotFilled],
    'caught (open)': [caughtText(null), COPY.caughtNotFilled],
    locked: [COPY.locked],
    'nothing locked': [COPY.nothingLocked],
  };
  for (const [name, copy] of Object.entries(answers)) {
    it(`${name}: at most 2 sentences`, () => {
      expect(copy.reduce((n, t) => n + sentences(t), 0)).toBeLessThanOrEqual(2);
    });
  }
});
