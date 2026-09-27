import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { CardBody } from '../api/types';
import { UNDER_REVIEW, warmingText } from '../lib/cards';
import { SWEET_CAVEAT, SWEET_EMPTY, footnote } from '../lib/sweet';
import { sweetFixture, sweetRow } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { SweetSpots } from './SweetSpots';

/** Spec §4's SWEET-vs-equity caveat, verbatim (`cards::sweet::sweet_parts`). */
const RECONCILE_NOTE =
  'Flagged episodes (engine_suspect, stale_transport) are excluded and counted, so SWEET does not reconcile with account equity.';

const renderBox = () => render(<MemoryRouter><SweetSpots /></MemoryRouter>);

/** `sweetFixture` plus card-level overrides (status, notes, hash) the series-only helper can't set. */
function sweetCard(rows: Parameters<typeof sweetFixture>[0], card: Partial<CardBody> = {}): CardBody {
  const base = sweetFixture(rows);
  return { ...base, ...card };
}

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

  it('shows the card status and the warming/under-review text (CardView\'s own pieces, reused)', async () => {
    mockApi({ '/api/cards': { win: '30d', cards: [sweetCard([sweetRow()], { status: 'warming', since_ms: 1_789_000_000_000 })] } });
    const { unmount } = renderBox();
    expect(await screen.findByText('WARMING')).toBeInTheDocument();
    expect(screen.getByText(warmingText({ since_ms: 1_789_000_000_000, computed_ms: 1_790_000_000_000 } as CardBody))).toBeInTheDocument();
    unmount();

    mockApi({ '/api/cards': { win: '30d', cards: [sweetCard([sweetRow()], { status: 'under_review' })] } });
    renderBox();
    expect(await screen.findByText('UNDER REVIEW')).toBeInTheDocument();
    expect(screen.getByText(UNDER_REVIEW)).toBeInTheDocument();
  });

  it("carries the card's hash footer and its own notes not already shown, without duplicating the caveat or footnote", async () => {
    const card = sweetCard([sweetRow({ label: 'candidate' })], {
      hash: '3f2a9c1b7d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4',
      notes: [SWEET_CAVEAT, footnote(1), RECONCILE_NOTE],
    });
    mockApi({ '/api/cards': { win: '30d', cards: [card] } });
    renderBox();
    expect(await screen.findByText('#3f2a9c1b7d4e')).toBeInTheDocument();
    expect(screen.getByText(RECONCILE_NOTE)).toBeInTheDocument();
    // the caveat and the footnote are each shown exactly once (their own paragraphs), never twice
    expect(screen.getAllByText(SWEET_CAVEAT)).toHaveLength(1);
    expect(screen.getAllByText(footnote(1), { exact: false })).toHaveLength(1);
  });

  it('formats the YES annualized figure as a percent, not a raw fraction', async () => {
    const rows = [sweetRow({ side: 'yes', scenario: 'sp-idle', latency_ms: 300, label: 'candidate', annualized: '0.12' })];
    mockApi({ '/api/cards': { win: '30d', cards: [sweetFixture(rows)] } });
    renderBox();
    await userEvent.click(within(await screen.findByRole('group', { name: 'side' })).getByRole('button', { name: 'YES' }));
    expect(screen.getByText(/12%\/yr/)).toBeInTheDocument();
    expect(screen.queryByText(/0\.12\/yr/)).toBeNull();
  });

  it('labels the leftovers-at-0 hover with the at-0 mean, not the total', async () => {
    const rows = [sweetRow({ captured: '0.5', captured_leftover0: '0.3', mean_leftover0: '0.15' })];
    mockApi({ '/api/cards': { win: '30d', cards: [sweetFixture(rows)] } });
    renderBox();
    expect(await screen.findByTitle('leftovers at 0, mean: +$0.15')).toBeInTheDocument();
  });
});
