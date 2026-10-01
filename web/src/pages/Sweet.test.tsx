import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { CardBody } from '../api/types';
import { warmingText } from '../lib/cards';
import { NOTHING_PAYS, ONE_LINE_CAVEAT, SWEET_CAVEAT, SWEET_EMPTY, footnote, recomputingText } from '../lib/sweet';
import { CARD_NOW, sweetFixture, sweetRow } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Sweet } from './Sweet';

const RECONCILE_NOTE =
  'Flagged episodes (engine_suspect, stale_transport) are excluded and counted, so SWEET does not reconcile with account equity.';
const renderPage = () => render(<MemoryRouter><Sweet /></MemoryRouter>);
const card = (rows: Parameters<typeof sweetFixture>[0], o: Partial<CardBody> = {}, s: Parameters<typeof sweetFixture>[1] = {}): CardBody => ({ ...sweetFixture(rows, s), ...o });

const rows = [
  sweetRow({ scenario: 'colo-idle', latency_ms: 52, label: 'candidate', mean: '0.2', lo: '0.05', hi: '0.35', n_exec: 40, clusters: 33 }),
  sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.1', lo: '-0.2', hi: '-0.01', n_exec: 41, clusters: 35, few_trades: true }),
];

describe('Sweet page', () => {
  it('asks for 30D, shows the one-line caveat, tiles, and the ladder grid', async () => {
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [card(rows)] }) });
    renderPage();
    expect(await screen.findByText(ONE_LINE_CAVEAT)).toBeInTheDocument();
    expect(String(fetch.mock.calls[0][0])).toBe('/api/cards?win=30d');
    const tiles = screen.getAllByRole('listitem').filter((li) => li.classList.contains('tile')).map((li) => li.textContent);
    expect(tiles).toEqual(['NO candidates0', 'YES candidates0', 'rows tested2', 'headline scenariosp-idle']); // dt comes before dd in the DOM
    const g = screen.getByRole('table', { name: 'NO · ALL' });
    expect(within(g).getByRole('button', { name: 'all at colo-idle (52 ms): candidate' })).toHaveTextContent('●');
    expect(within(g).getByRole('button', { name: 'all at sp-idle (300 ms): negative' })).toHaveTextContent('○');
    expect(within(g).getByRole('button', { name: 'all at rung-0 (0 ms): no attempts' })).toHaveTextContent('·');
    expect(screen.getByText('z = 1.96 across 2 rows tested: the more rows, the wider each interval.')).toBeInTheDocument();
    expect(screen.getByText(NOTHING_PAYS)).toBeInTheDocument();
    // no 10 bips fee entry in this fixture: I3's feeLine renders nothing.
    expect(screen.queryByText(/whole NO side at 10 bips/)).toBeNull();
  });

  it('shows the 10 bips fee line under the tiles when the backend sent one', async () => {
    mockApi({
      '/api/cards': {
        win: '30d',
        cards: [card(rows, {}, { fees: [{ fee_bips: 10, captured: '-1', mean: '-0.1', label: 'negative' }] })],
      },
    });
    renderPage();
    expect(await screen.findByText('whole NO side at 10 bips: NEGATIVE, mean −$0.10')).toBeInTheDocument();
  });

  it('has no scenario chips: six NO columns, two YES columns, CATEGORY hidden without tags', async () => {
    mockApi({
      '/api/cards': {
        win: '30d',
        cards: [card([...rows, sweetRow({ side: 'yes', label: 'dead_on_paper' }), sweetRow({ side: 'yes', scenario: 'sp-burst', latency_ms: 450, label: 'candidate' })], {}, { category_available: false })],
      },
    });
    renderPage();
    const g = await screen.findByRole('table', { name: 'NO · ALL' });
    expect(within(g).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['ms', '0', '25', '52', '100', '200', '300']);
    expect(screen.queryByRole('group', { name: 'scenario' })).toBeNull();
    expect(within(screen.getByRole('group', { name: 'dimension' })).queryByRole('button', { name: 'CATEGORY' })).toBeNull();
    await userEvent.click(within(screen.getByRole('group', { name: 'side' })).getByRole('button', { name: 'YES' }));
    const yes = screen.getByRole('table', { name: 'YES · ALL' });
    expect(within(yes).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['ms', '52', '300']);
    expect(within(yes).getByRole('button', { name: 'all at sp-idle (300 ms): dead on paper' })).toHaveTextContent('✗');
  });

  it('keeps the picked side chip pressed across a window switch (M1: state lives in Sweet(), not SweetBody)', async () => {
    mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [card(rows)] }) });
    renderPage();
    await screen.findByText(ONE_LINE_CAVEAT);
    const yes = () => within(screen.getByRole('group', { name: 'side' })).getByRole('button', { name: 'YES' });
    await userEvent.click(yes());
    expect(yes()).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(screen.getByRole('button', { name: '7D' }));
    await waitFor(() => expect(yes()).toHaveAttribute('aria-pressed', 'true'));
  });

  it('keeps every number and the method in folds', async () => {
    const c = card(rows, { hash: '3f2a9c1b7d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4', notes: [SWEET_CAVEAT, footnote(1), RECONCILE_NOTE] });
    mockApi({ '/api/cards': { win: '30d', cards: [c] } });
    renderPage();
    const numbers = (await screen.findByText('all numbers')).closest('details')!;
    expect(within(numbers).getByRole('columnheader', { name: /a co-located trader/ })).toBeInTheDocument();
    const how = screen.getByText("how it's computed").closest('details')!;
    expect(within(how).getByText(SWEET_CAVEAT)).toBeInTheDocument();
    expect(within(how).getByText(RECONCILE_NOTE)).toBeInTheDocument();
    expect(within(how).getByText('#3f2a9c1b7d4e')).toBeInTheDocument();
    expect(screen.getAllByText(SWEET_CAVEAT)).toHaveLength(1);
    expect(screen.getAllByText(footnote(1), { exact: false })).toHaveLength(1);
    expect(numbers.open).toBe(false);
  });

  it('state panels replace chips and chart: missing, warming, under review (review focus 2)', async () => {
    mockApi({ '/api/cards': { win: '30d', cards: [] } });
    const a = renderPage();
    expect(await screen.findByText(SWEET_EMPTY)).toBeInTheDocument();
    a.unmount();

    const warm = card(rows, { status: 'warming', since_ms: CARD_NOW - 3 * 86_400_000 });
    mockApi({ '/api/cards': { win: '30d', cards: [warm] } });
    const b = renderPage();
    expect(await screen.findByText(warmingText(warm))).toBeInTheDocument();
    expect(screen.getByText('WARMING')).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: 'side' })).toBeNull();
    b.unmount();

    const review = card(rows, { status: 'under_review' });
    mockApi({ '/api/cards': { win: '30d', cards: [review] } });
    renderPage();
    expect(await screen.findByText(recomputingText(review.computed_ms))).toBeInTheDocument();
    expect(screen.getByText('UNDER REVIEW')).toBeInTheDocument();
    expect(screen.queryByRole('table', { name: /NO · ALL/ })).toBeNull();
  });

  it('slices by category: the all row first, then its buckets; focus shows one detail line', async () => {
    const withCrypto = [
      ...rows,
      sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'rung-25', latency_ms: 25, label: 'thin', mean: '0.3', n_exec: 12, clusters: 9 }),
    ];
    mockApi({ '/api/cards': { win: '30d', cards: [card(withCrypto)] } });
    renderPage();
    await userEvent.click(within(await screen.findByRole('group', { name: 'dimension' })).getByRole('button', { name: 'CATEGORY' }));
    const g = screen.getByRole('table', { name: 'NO · CATEGORY' });
    expect(within(g).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['all', 'crypto']);
    const cell = within(g).getByRole('button', { name: 'crypto at rung-25 (25 ms): thin, mean > 0' });
    expect(cell).toHaveTextContent('◐');
    fireEvent.focus(cell);
    expect(screen.getByText('crypto · rung-25 · n 12 · C 9 · captured on paper at 25 ms, per $100: +$0.30 · too few trades or events to say anything.')).toBeInTheDocument();
  });
});
