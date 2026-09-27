import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { sweetFixture, sweetRow } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { SWEET_CAVEAT, SWEET_EMPTY } from '../lib/sweet';
import { SweetSpots } from './SweetSpots';

const renderBox = () => render(<MemoryRouter><SweetSpots /></MemoryRouter>);

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
});
