import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { accountFixture, edgeFixture, ledgerFixture, positionFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { PAPER_EMPTY, PAPER_QUIP, Paper, extremes, policyLabel } from './Paper';

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
    expect(screen.getByText('$1,002.56')).toBeInTheDocument();
    expect(screen.getByText('−$286.60')).toHaveClass('neg');
    expect(screen.getByText('Fed decision in October?')).toBeInTheDocument();
    expect(await screen.findByText('−$99.84')).toBeInTheDocument();
    const urls = f.mock.calls.map((c) => String(c[0]));
    expect(urls.some((u) => u.includes('/api/paper/equity?account=P3-naive%2Fsp-idle&hours=24'))).toBe(true);
    expect(urls.some((u) => u.includes('/api/paper/ledger?account=P0-converter%2Fsp-idle'))).toBe(true);
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
    expect(await screen.findByRole('img', { name: 'capture rate by latency' })).toBeInTheDocument();
  });
});
