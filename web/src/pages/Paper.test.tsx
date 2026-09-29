import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { EPOCH1_START, accountFixture, edgeFixture, epochsFixture, ledgerFixture, positionFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { PAPER_EMPTY, PAPER_LOSS_QUIP, PAPER_QUIP, Paper, equityAxis, extremes, policyLabel } from './Paper';

const equityRoute = (url: URL) => ({
  account_id: url.searchParams.get('account'),
  from_ms: 0,
  to_ms: 100,
  points: url.searchParams.get('account')?.endsWith('nodelay')
    ? []
    : [
        { t_ms: 0, equity: '1000' },
        { t_ms: 50, equity: url.searchParams.get('account')?.startsWith('P3') ? '713.4' : '1002.56' },
      ],
});

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route
          path="/paper"
          element={
            <>
              <Paper />
              <Loc />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );

const accounts = [
  accountFixture(),
  accountFixture({ account_id: 'P3-naive/sp-idle', policy: 'P3-naive', equity: '713.4', realized: '-286.6', n_positions: 1 }),
  accountFixture({ account_id: 'P0-converter/colo-idle', scenario: 'colo-idle', equity: '1000' }),
];

describe('Paper', () => {
  it('says nothing ventured when there are no paper accounts (S3c not deployed)', async () => {
    mockApi({ '/api/paper/accounts': { items: [] }, '/api/paper/positions': { items: [] } });
    renderAt('/paper');
    expect(await screen.findByText(PAPER_EMPTY)).toBeInTheDocument();
    expect(PAPER_EMPTY).toBe('No paper trades yet. Nothing ventured, nothing lost.');
  });

  it('switches scenario and shows one line per policy, cards, positions and the ledger', async () => {
    const f = mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [positionFixture()] },
      '/api/paper/equity': (url: URL) => ({
        account_id: url.searchParams.get('account'),
        from_ms: 0,
        to_ms: 100,
        points: [
          { t_ms: 0, equity: '1000' },
          { t_ms: 50, equity: url.searchParams.get('account')?.startsWith('P3') ? '713.4' : '1002.56' },
        ],
      }),
      '/api/paper/ledger': { items: [ledgerFixture()], next_cursor: null },
    });
    renderAt('/paper');
    expect(await screen.findByRole('button', { name: 'COLO-IDLE' })).toHaveClass('on');
    await userEvent.click(screen.getByRole('button', { name: 'SP-IDLE' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('scenario=sp-idle');
    expect(await screen.findByText('P3 naive')).toBeInTheDocument();
    expect(screen.getByText('P0 converter')).toBeInTheDocument();
    expect(screen.getByText(PAPER_QUIP)).toBeInTheDocument();
    expect(PAPER_QUIP).toBe("The naive strategy is doing great. Until 'Other' wins.");
    expect(screen.getByText(PAPER_LOSS_QUIP)).toBeInTheDocument();
    // I4: now also in the Accounts table's EQUITY column, beside the equity chart's end legend.
    expect(screen.getAllByText('$1,002.56')).toHaveLength(2);
    expect(screen.getByText('−$286.60')).toHaveClass('neg');
    expect(screen.getByText('Fed decision in October?')).toBeInTheDocument();
    expect(await screen.findByText('−$99.84')).toBeInTheDocument();
    // M6: money/qty columns in the ledger and positions tables get td.num (no mid-value wrap).
    expect(screen.getByText('−$99.84').closest('td')).toHaveClass('num');
    const posRow = screen.getByText('Fed decision in October?').closest('tr')!;
    expect(posRow.querySelectorAll('td.num')).toHaveLength(2); // QTY, BASIS
    const legend = screen.getByRole('list', { name: 'equity now' });
    expect(within(legend).getByText('P3 naive')).toBeInTheDocument();
    expect(within(legend).getByText('$713.40')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: /^ACCOUNTS/ })).toBeInTheDocument();
    for (const f2 of ['all numbers', 'ledger', 'awaiting resolution']) expect(screen.getByText(f2).closest('details')).not.toHaveAttribute('open');
    const urls = f.mock.calls.map((c) => String(c[0])); // f2 avoids shadowing the mockApi handle `f` above
    expect(urls.some((u) => u.includes('/api/paper/equity?account=P3-naive%2Fsp-idle&hours=24'))).toBe(true);
    expect(urls.some((u) => u.includes('/api/paper/ledger?account=P0-converter%2Fsp-idle'))).toBe(true);
  });

  it('orders the equity-now legend by final equity, highest first, regardless of the api order', async () => {
    mockApi({
      '/api/paper/accounts': {
        items: [
          accountFixture({ account_id: 'P3-naive/sp-idle', policy: 'P3-naive', equity: '713.4' }),
          accountFixture({ account_id: 'P0-converter/sp-idle', policy: 'P0-converter', equity: '1002.56' }),
        ],
      },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
    });
    renderAt('/paper');
    const legend = await screen.findByRole('list', { name: 'equity now' });
    const items = within(legend)
      .getAllByRole('listitem')
      .map((li) => li.textContent);
    expect(items[0]).toContain('P0 converter');
    expect(items[0]).toContain('$1,002.56');
    expect(items[1]).toContain('P3 naive');
    expect(items[1]).toContain('$713.40');
  });

  it('labels policies and finds extremes without printing floats', () => {
    expect(policyLabel({ policy: 'P0-converter', fee_bips: '0' })).toBe('P0 converter');
    expect(policyLabel({ policy: 'P0-converter', fee_bips: '10' })).toBe('P0 converter (fee 10 bp)');
    expect(extremes(['1000', '713.4', '1002.56'])).toEqual({ min: '713.4', max: '1002.56' });
    expect(extremes([])).toBeNull();
  });

  it('puts the Edge capture box at the top of the page', async () => {
    mockApi({
      '/api/paper/accounts': { items: [] },
      '/api/paper/positions': { items: [] },
      '/api/cards': { win: '24h', cards: [edgeFixture({ win: '24h' })] },
    });
    renderAt('/paper');
    await screen.findByText(PAPER_EMPTY);
    const headings = screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);
    expect(headings[0]).toBe('EDGE CAPTURE');
    expect(await screen.findByRole('figure', { name: 'capture rate by latency' })).toBeInTheDocument();
  });

  it('renders sp-idle-nodelay after another scenario, with fewer accounts there and no marks yet', async () => {
    mockApi({
      '/api/paper/accounts': {
        items: [
          ...accounts,
          accountFixture({
            account_id: 'P0-converter/sp-idle-nodelay', scenario: 'sp-idle-nodelay', marked_ms: null, cash: null, equity: null,
            realized: null, unrealized: null, fees_paid: null, locked: null, max_drawdown: null, n_positions: null,
          }),
        ],
      },
      '/api/paper/positions': { items: [positionFixture()] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
    });
    renderAt('/paper?scenario=sp-idle');
    expect(await screen.findByText('P3 naive')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'SP-IDLE-NODELAY' }));
    expect(screen.getByRole('button', { name: 'SP-IDLE-NODELAY' })).toHaveClass('on');
    expect(await screen.findByText('No marks yet. Nothing ventured, nothing lost.')).toBeInTheDocument();
    expect(await screen.findByText('Nothing to draw yet. The chart is patient.')).toBeInTheDocument();
    expect(screen.queryByText('P3 naive')).not.toBeInTheDocument();
    expect(await screen.findByText('Empty ledger. Nothing ventured, nothing lost.')).toBeInTheDocument();
    expect(screen.queryByText(PAPER_QUIP)).not.toBeInTheDocument();
    expect(screen.getByText(PAPER_LOSS_QUIP)).toBeInTheDocument();
  });

  it('names the outcome of each awaiting position by a short market id, not only the event', async () => {
    mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': {
        items: [
          positionFixture({ market_id: '0x5f2e11aa22bb33cc44dd9a01', event_title: 'Best AI model end of September?' }),
          positionFixture({ market_id: '0x77aa11aa22bb33cc44dd4c3d', event_title: 'Best AI model end of September?' }),
        ],
      },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
    });
    renderAt('/paper?scenario=sp-idle');
    expect(await screen.findByText('0x5f2e…9a01')).toBeInTheDocument();
    expect(screen.getByText('0x77aa…4c3d')).toBeInTheDocument();
    expect(screen.getAllByText('Best AI model end of September?')).toHaveLength(2);
  });

  it('hides minute marks in the ledger by default, shows them on request, and still pages', async () => {
    const f = mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': (url: URL) =>
        url.searchParams.get('hide_marks') === '1'
          ? { items: [ledgerFixture()], next_cursor: url.searchParams.get('cursor') ? null : '3_1' }
          : { items: [ledgerFixture({ seq: 2, kind: 'mark', cash_delta: '0', fees: '0', equity: '1002' }), ledgerFixture()], next_cursor: null },
    });
    renderAt('/paper?scenario=sp-idle');
    expect(await screen.findByText('−$99.84')).toBeInTheDocument();
    expect(screen.queryByText('mark')).not.toBeInTheDocument();
    const ledgerUrls = () => f.mock.calls.map((c) => String(c[0])).filter((u) => u.includes('/api/paper/ledger'));
    expect(ledgerUrls().every((u) => u.includes('hide_marks=1'))).toBe(true);
    await userEvent.click(screen.getByText('ledger'));
    await userEvent.click(screen.getByRole('button', { name: 'OLDER →' }));
    await waitFor(() => expect(ledgerUrls().at(-1)).toContain('cursor=3_1'));
    expect(ledgerUrls().at(-1)).toContain('hide_marks=1');
    const toggle = await screen.findByRole('checkbox', { name: 'show minute marks' });
    expect(toggle).not.toBeChecked();
    await userEvent.click(toggle);
    expect(await screen.findByText('mark')).toBeInTheDocument();
    expect(ledgerUrls().at(-1)).not.toContain('hide_marks');
    expect(ledgerUrls().at(-1)).not.toContain('cursor');
  });

  it('starts the equity x-axis at the first real mark, so a partial window fills the chart width', () => {
    const windowFrom = Date.UTC(2026, 8, 25, 0, 0, 0);
    const windowTo = Date.UTC(2026, 8, 26, 0, 0, 0); // a 24 h window
    const dataFrom = Date.UTC(2026, 8, 25, 21, 0, 0); // only the last 3 h have marks
    expect(equityAxis([dataFrom, dataFrom + 3_600_000], windowFrom, windowTo)).toEqual({
      xMin: dataFrom,
      xMax: windowTo,
      xLabels: { left: 'since 21:00 UTC', right: 'now' },
    });
  });

  it('keeps the full window with no label when the data already spans it, or there is none yet', () => {
    expect(equityAxis([0, 50, 100], 0, 100)).toEqual({ xMin: 0, xMax: 100 });
    expect(equityAxis([], 0, 100)).toEqual({ xMin: 0, xMax: 100 });
  });

  it('rounds the equity axis labels to the cent', async () => {
    mockApi({
      '/api/paper/accounts': { items: [accountFixture()] },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': (url: URL) => ({
        account_id: url.searchParams.get('account'), from_ms: 0, to_ms: 100,
        points: [{ t_ms: 0, equity: '1000.15581' }, { t_ms: 50, equity: '251.0719' }],
      }),
      '/api/paper/ledger': { items: [], next_cursor: null },
    });
    renderAt('/paper');
    expect(await screen.findByText('low $251.07')).toBeInTheDocument();
    expect(screen.getByText('high $1,000.16')).toBeInTheDocument();
  });

  it('can hide the P3 control line so its drawdown does not flatten the others', async () => {
    mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
    });
    renderAt('/paper?scenario=sp-idle');
    expect(await screen.findByText('low $713.40')).toBeInTheDocument();
    expect(screen.getByText('P3 naive')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('checkbox', { name: 'hide P3 (control: ignores fees)' }));
    expect(screen.getByText('low $1,000.00')).toBeInTheDocument();
    expect(screen.queryByText('P3 naive')).not.toBeInTheDocument();
    expect(screen.getAllByText('P3 · NAIVE').length).toBeGreaterThan(0); // its accounts row and ledger button stay
  });

  const S2 = Date.UTC(2026, 8, 30, 8, 0);
  const twoEpochs = epochsFixture({
    epochs: [{ epoch: 1, start_ms: EPOCH1_START, cause: 'policies' }, { epoch: 2, start_ms: S2, cause: 'reset' }],
    current: 2,
  });

  it('says since when and which epoch, above ACCOUNTS', async () => {
    mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
      '/api/paper/epochs': twoEpochs,
    });
    renderAt('/paper');
    const line = await screen.findByText('since reset 2026-09-30 08:00 UTC · epoch 2');
    const head = await screen.findByRole('heading', { level: 2, name: /^ACCOUNTS/ });
    expect(line.compareDocumentPosition(head) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('earlier epochs: nothing fetched until the fold opens, then each epoch with its P0 fee-0 final equity', async () => {
    const f = mockApi({
      '/api/paper/accounts': { items: [...accounts, accountFixture({ account_id: 'P0-converter/rung-0', scenario: 'rung-0' })] },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': (url: URL) =>
        url.searchParams.has('epoch')
          ? {
              account_id: url.searchParams.get('account'), from_ms: 0, to_ms: 1,
              points: [{ t_ms: 0, equity: '1000' }, { t_ms: 1, equity: url.searchParams.get('epoch') === '0' ? '476.2' : '1003.5' }],
            }
          : equityRoute(url),
      '/api/paper/ledger': { items: [], next_cursor: null },
      '/api/paper/epochs': twoEpochs,
    });
    renderAt('/paper');
    await screen.findByText('since reset 2026-09-30 08:00 UTC · epoch 2');
    const epochCalls = () => f.mock.calls.map((c) => String(c[0])).filter((u) => u.includes('epoch='));
    expect(epochCalls()).toEqual([]);
    await userEvent.click(screen.getByText('earlier epochs'));
    const fold = screen.getByText('earlier epochs').closest('details') as HTMLDetailsElement;
    await waitFor(() => expect(within(fold).getAllByText('$476.20')).toHaveLength(3));
    expect(within(fold).getAllByText('$1,003.50')).toHaveLength(3);
    expect(within(fold).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['EPOCH', 'FROM', 'TO', 'CAUSE', 'RUNG-0', 'COLO-IDLE', 'SP-IDLE']);
    expect(within(fold).getByText('before epochs')).toBeInTheDocument();
    expect(epochCalls().every((u) => u.includes('hours=6'))).toBe(true);
    expect(new Set(epochCalls().map((u) => new URL(u, 'http://x').searchParams.get('account')))).toEqual(
      new Set(['P0-converter/rung-0', 'P0-converter/colo-idle', 'P0-converter/sp-idle']),
    );
  });

  it('labels P3 as the control that ignores fees', async () => {
    mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
    });
    renderAt('/paper?scenario=sp-idle');
    expect(await screen.findByRole('checkbox', { name: 'hide P3 (control: ignores fees)' })).toBeInTheDocument();
    const legend = await screen.findByRole('list', { name: 'equity now' });
    expect(within(legend).getByText('· control: ignores fees')).toBeInTheDocument();
    expect(within(legend).getByText('P3 naive')).toBeInTheDocument();
  });

  describe('a reset applied while the page is open (D-03)', () => {
    afterEach(() => {
      vi.useRealTimers();
      localStorage.clear();
    });

    it('refetches accounts, positions, equity and the ledger together', async () => {
      localStorage.setItem('fold:paper:ledger', '1');
      vi.useFakeTimers({ shouldAdvanceTime: true });
      let epochAsks = 0;
      const f = mockApi({
        '/api/paper/accounts': { items: accounts },
        '/api/paper/positions': { items: [] },
        '/api/paper/equity': equityRoute,
        '/api/paper/ledger': { items: [], next_cursor: null },
        // first answer: epoch 1 with a reset pending (so it is polled); then the scanner applied it: epoch 2
        '/api/paper/epochs': () => (++epochAsks === 1 ? epochsFixture({ pending_request: { id: 1, requested_ms: 1 } }) : twoEpochs),
      });
      renderAt('/paper?scenario=sp-idle');
      await screen.findByText('since new policies 2026-09-29 19:58 UTC · epoch 1');
      await screen.findByRole('list', { name: 'equity now' });
      const count = (path: string) => f.mock.calls.filter((c) => String(c[0]).startsWith(path)).length;
      const keys = ['accounts', 'positions', 'equity', 'ledger'];
      await waitFor(() => expect(count('/api/paper/ledger')).toBeGreaterThan(0));
      const before = keys.map((k) => count(`/api/paper/${k}`));
      await vi.advanceTimersByTimeAsync(10_000);
      await screen.findByText('since reset 2026-09-30 08:00 UTC · epoch 2');
      await waitFor(() => keys.forEach((k, i) => expect(count(`/api/paper/${k}`)).toBeGreaterThan(before[i])));
    });
  });
});
