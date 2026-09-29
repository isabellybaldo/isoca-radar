import { fireEvent, render, screen, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { EpisodeSummary } from '../api/types';
import { EPISODE_DEF } from '../help';
import { episodeSummaryFixture, ladderFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { COPY } from './copy';
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

describe('D-04: one drawing and at most two sentences per answer', () => {
  const sentences = (t: string) => t.split(/(?<=[.!?])\s+/).filter(Boolean).length;
  const answers: Record<string, string[]> = {
    'make money (yes)': [COPY.makeMoneyYes],
    'make money (no)': [COPY.makeMoneyNo],
    'make money (incomplete)': [COPY.makeMoneyIncomplete],
    'what is an episode': [COPY.whatEpisode, COPY.episodeBar],
  };
  for (const [name, copy] of Object.entries(answers)) {
    it(`${name}: at most 2 sentences`, () => {
      expect(copy.reduce((n, t) => n + sentences(t), 0)).toBeLessThanOrEqual(2);
    });
  }
});
