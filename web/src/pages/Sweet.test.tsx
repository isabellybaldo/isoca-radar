import { render, screen, within } from '@testing-library/react';
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
  it('asks for 30D, shows the one-line caveat, tiles, and the sp-idle chart', async () => {
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [card(rows)] }) });
    renderPage();
    expect(await screen.findByText(ONE_LINE_CAVEAT)).toBeInTheDocument();
    expect(String(fetch.mock.calls[0][0])).toBe('/api/cards?win=30d');
    const tiles = screen.getAllByRole('listitem').filter((li) => li.classList.contains('tile')).map((li) => li.textContent);
    expect(tiles).toEqual(['NO candidates0', 'YES candidates0', 'rows tested2', 'scenariosp-idle']); // dt comes before dd in the DOM
    const chart = screen.getByRole('list', { name: 'NO · ALL · sp-idle' });
    expect(within(chart).getByText('NEGATIVE')).toBeInTheDocument();
    expect(screen.getByText(NOTHING_PAYS)).toBeInTheDocument();
  });

  it('switches scenario, side and slice, hides CATEGORY without tags', async () => {
    mockApi({ '/api/cards': { win: '30d', cards: [card([...rows, sweetRow({ side: 'yes', label: 'dead_on_paper' })], {}, { category_available: false })] } });
    renderPage();
    const scen = await screen.findByRole('group', { name: 'scenario' });
    expect(within(scen).getByRole('button', { name: /co-located trader/i })).toBeInTheDocument();
    await userEvent.click(within(scen).getByRole('button', { name: /co-located trader/i }));
    expect(within(screen.getByRole('list', { name: 'NO · ALL · colo-idle' })).getByText('CANDIDATE')).toBeInTheDocument();
    expect(within(screen.getByRole('group', { name: 'dimension' })).queryByRole('button', { name: 'CATEGORY' })).toBeNull();
    await userEvent.click(within(screen.getByRole('group', { name: 'side' })).getByRole('button', { name: 'YES' }));
    // Review focus 3: colo-idle has no YES rows, so the chart falls back to the first YES scenario
    expect(within(screen.getByRole('list', { name: 'YES · ALL · sp-idle' })).getByText('DEAD ON PAPER')).toBeInTheDocument();
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
    expect(screen.queryByRole('list', { name: /sp-idle/ })).toBeNull();
  });
});
