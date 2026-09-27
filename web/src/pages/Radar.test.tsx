import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { edgeFixture, episodeSummaryFixture, healthFixture, todayFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import {
  INCOMPLETE_WHY, NO_ARB, NO_HEALTH, OPEN_STALE, Radar, TODAY_SUBTITLE, isArbitrage, msgsPerSecond, todayLine,
} from './Radar';

const renderRadar = () =>
  render(
    <MemoryRouter>
      <Radar />
    </MemoryRouter>,
  );

const incomplete = (id: string, title: string) =>
  episodeSummaryFixture({ id, kind: 'yes_basket_incomplete', realization: 'not_arbitrage', event_title: title });

const h2s = () => screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);

describe('Radar', () => {
  it('shows the deadpan empty arbitrage line and ten hearts on a quiet, healthy day', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(),
    });
    renderRadar();
    expect(await screen.findByText(NO_ARB)).toBeInTheDocument();
    expect(NO_ARB).toBe('No arbitrage open right now. They rarely last.');
    expect(await screen.findByRole('img', { name: 'health 10 of 10' })).toBeInTheDocument();
    expect(screen.getByText('ARBITRAGE (YES / NO) · 0')).toBeInTheDocument();
    expect(screen.getByText('40/40')).toBeInTheDocument();
    expect(screen.getByText('4.3k')).toBeInTheDocument();
  });

  it('puts TODAY above the open episodes', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(),
    });
    renderRadar();
    await screen.findByText(NO_ARB);
    expect(h2s()).toEqual(['TODAY (UTC)', 'ARBITRAGE (YES / NO) · 0', 'NOT ARBITRAGE: INCOMPLETE SETS · 0']);
  });

  it('lists arbitrage with PROFIT @ $100 first, the depth figures second, and a link to the detail', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [episodeSummaryFixture()] },
      '/api/today': todayFixture(1, 0, '0.33'),
    });
    renderRadar();
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    expect(link).toHaveAttribute('href', '/episodes/live-1-00000001');
    const table = link.closest('table') as HTMLTableElement;
    const heads = within(table).getAllByRole('columnheader').map((h) => h.textContent);
    expect(heads).toEqual(['KIND', 'EVENT', 'S', 'PROFIT @ $100', 'NET @ DEPTH', 'GROSS @ DEPTH', 'Q* (SHARES)', 'AGE', 'EDGE']);
    expect(within(table).getByText('+$0.33')).toHaveClass('pos');
    expect(within(table).getByText('+$0.41')).toHaveClass('pos');
    expect(within(table).getByText('+$1.37')).toBeInTheDocument();
    expect(within(table).getByText('83')).toBeInTheDocument();
    expect(within(table).getByText('NO')).toBeInTheDocument();
    expect(screen.getByText('ARBITRAGE (YES / NO) · 1')).toBeInTheDocument();
  });

  it('shows a missing $100 figure as not measured, never as zero', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [episodeSummaryFixture({ net_budget100: null })] },
      '/api/today': todayFixture(1),
    });
    renderRadar();
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    const row = link.closest('tr') as HTMLTableRowElement;
    expect(within(row).getByTitle('not measured')).toHaveTextContent('—');
  });

  it('explains every column under the arbitrage table', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [episodeSummaryFixture()] },
      '/api/today': todayFixture(1),
    });
    renderRadar();
    const legend = await screen.findByLabelText('column legend');
    for (const bit of ['KIND', 'S', 'PROFIT @ $100', 'NET @ DEPTH', 'GROSS', 'Q*', 'AGE', 'EDGE']) {
      expect(legend).toHaveTextContent(bit);
    }
    expect(screen.getByRole('button', { name: 'Help: What do the columns mean?' })).toBeInTheDocument();
  });

  it('splits incomplete sets into their own dimmed section, collapsed behind a toggle with the count', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': {
        now_ms: 0,
        items: [episodeSummaryFixture(), incomplete('live-1-2', 'Best AI model?'), incomplete('live-1-3', 'Oscars?')],
      },
      '/api/today': todayFixture(1),
    });
    renderRadar();
    expect(await screen.findByText('ARBITRAGE (YES / NO) · 1')).toBeInTheDocument();
    const section = screen.getByText('NOT ARBITRAGE: INCOMPLETE SETS · 2').closest('section') as HTMLElement;
    expect(section).toHaveClass('dimmed');
    expect(within(section).getByText(INCOMPLETE_WHY)).toBeInTheDocument();
    expect(INCOMPLETE_WHY).toBe("An outcome outside the set can win (e.g. 'Other'). Shown for context, not money.");
    expect(screen.queryByRole('link', { name: 'Best AI model?' })).not.toBeInTheDocument();
    const toggle = within(section).getByRole('button', { name: 'show 2 incomplete sets' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await userEvent.click(toggle);
    expect(within(section).getByRole('link', { name: 'Best AI model?' })).toBeInTheDocument();
    expect(within(section).getByRole('link', { name: 'Oscars?' })).toBeInTheDocument();
    expect(within(section).getByRole('button', { name: 'hide 2 incomplete sets' })).toHaveAttribute('aria-expanded', 'true');
  });

  it('only counts YES and NO complete baskets as arbitrage', () => {
    expect(isArbitrage('yes_basket')).toBe(true);
    expect(isArbitrage('no_basket')).toBe(true);
    expect(isArbitrage('yes_basket_incomplete')).toBe(false);
    expect(isArbitrage('implication')).toBe(false);
  });

  it('says today in arbitrage episodes per side and the capped edge, with a true subtitle and no fee line', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(5, 2, '3.1'),
    });
    renderRadar();
    expect(
      await screen.findByText('Arbitrage episodes today: 7 (NO 5 · YES 2) · edge on the table: +$3.10 (capped at $100 each)'),
    ).toBeInTheDocument();
    expect(screen.getByText(TODAY_SUBTITLE)).toBeInTheDocument();
    expect(screen.queryByText(/Fees:/)).not.toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'arbitrage episodes per hour today' })).toBeInTheDocument();
    expect(todayLine(todayFixture())).toBe('Arbitrage episodes today: 0 (NO 0 · YES 0) · edge on the table: — (capped at $100 each)');
    expect(msgsPerSecond(59)).toBe('1');
  });

  it('names health troubles in text, not colour alone', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ drops_5m: 2, capture_paused: true }) },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(),
    });
    renderRadar();
    expect(await screen.findByRole('img', { name: /^health 8 of 10/ })).toBeInTheDocument();
    expect(screen.getByText('DROPS IN THE LAST 5 MIN')).toBeInTheDocument();
    expect(screen.getByText('CAPTURE PAUSED')).toBeInTheDocument();
  });

  it('tags the open-episodes list as stale when the scanner has not reported in over 3 minutes', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ age_ms: 200_000 }) },
      '/api/episodes/open': { now_ms: 0, items: [episodeSummaryFixture()] },
      '/api/today': todayFixture(1),
    });
    renderRadar();
    expect(await screen.findByText(OPEN_STALE)).toBeInTheDocument();
  });

  it('does not tag the open-episodes list as stale when the scanner reported recently', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ age_ms: 5_000 }) },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(),
    });
    renderRadar();
    expect(await screen.findByText(NO_ARB)).toBeInTheDocument();
    expect(screen.queryByText(OPEN_STALE)).not.toBeInTheDocument();
  });

  it('explains a missing health row and a busy database instead of going blank', async () => {
    mockApi({
      '/api/health/now': { health: null },
      '/api/episodes/open': () =>
        jsonResponse(503, { error: 'db_unavailable', detail: 'The database is thinking. Try again in a moment.' }),
      '/api/today': todayFixture(),
    });
    renderRadar();
    expect(await screen.findByText(NO_HEALTH)).toBeInTheDocument();
    expect((await screen.findAllByText('The database is thinking. Try again in a moment.')).length).toBeGreaterThan(0);
  });

  it('gives every section a (?)', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(),
    });
    renderRadar();
    await screen.findByText(NO_ARB);
    const names = screen.getAllByRole('button', { name: /^Help: / }).map((b) => b.getAttribute('aria-label'));
    expect(names).toEqual([
      'Help: What are the hearts?',
      'Help: What does today count?',
      'Help: What is an episode?',
      'Help: Why is an incomplete set not arbitrage?',
    ]);
  });

  it('adds the Edge capture teaser to the today box when analyze has a today result', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(3, 1),
      '/api/cards': { win: 'today', cards: [edgeFixture()] },
    });
    renderRadar();
    expect(await screen.findByText('Edge on the table today: +$12.50 · captured at 145 ms: 3.2%.')).toBeInTheDocument();
  });

  it('shows the teaser rate as — while there is too little edge on the table', async () => {
    const thin = edgeFixture();
    thin.columns = thin.columns.map((r) => (r.label === 'edge on the table' ? { ...r, ours: '0.4' } : r));
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(1),
      '/api/cards': { win: 'today', cards: [thin] },
    });
    renderRadar();
    expect(await screen.findByText('Edge on the table today: +$0.40 · captured at 145 ms: —.')).toBeInTheDocument();
  });
});
