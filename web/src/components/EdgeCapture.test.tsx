import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { edgeFixture, metaFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { EDGE_EMPTY, EDGE_SUBTITLE, EdgeCapture } from './EdgeCapture';

const renderBox = () =>
  render(
    <MemoryRouter>
      <EdgeCapture />
    </MemoryRouter>,
  );

describe('EdgeCapture', () => {
  it('shows the decay chart, the measured marker, the split, the capital and the subtitle for 24H', async () => {
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [edgeFixture({ win: '24h' })] }) });
    renderBox();
    expect(await screen.findByRole('figure', { name: 'capture rate by latency' })).toBeInTheDocument();
    expect(String(fetch.mock.calls[0][0])).toBe('/api/cards?win=24h');
    expect(screen.getByText(EDGE_SUBTITLE)).toBeInTheDocument();
    expect(EDGE_SUBTITLE).toBe('The difference is physics and other people.');
    expect(screen.getByText(/^measured: vps-kvm2/)).toHaveTextContent('measured: vps-kvm2 ≈ 200 ms (an estimate)');
    expect(screen.getByText('sp-idle · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.getByText('sp-burst · 450 ms → —')).toBeInTheDocument();
    expect(screen.getByText('$9.25')).toBeInTheDocument();
    expect(screen.getByText('$180.00')).toBeInTheDocument();
    expect(screen.getByText('upper bound: assumes first-in-line execution; CLOB queue ignored')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Help: Why is the NO side cashable and the YES side locked?' })).toBeInTheDocument();
  });

  it('switches windows and says so when there is nothing yet', async () => {
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [] }) });
    renderBox();
    expect(await screen.findByText(EDGE_EMPTY)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: '7D' }));
    expect(fetch.mock.calls.some((c) => String(c[0]) === '/api/cards?win=7d')).toBe(true);
    expect(screen.getByRole('button', { name: '7D' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('dims the Edge rows when the last tick is stale, as the Papers page does', async () => {
    mockApi({
      '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [edgeFixture({ win: '24h' })] }),
      '/api/cards/meta': metaFixture({ tick: { last_tick_ms: Date.now() - 2 * 3_600_000, analyze_version: 'abc1234', tick_ms: 42 } }),
    });
    renderBox();
    await waitFor(() => expect(screen.getByText('edge on the table').closest('tr')).toHaveClass('dim'));
  });

  it('keeps the Edge rows undimmed when the last tick is fresh', async () => {
    mockApi({
      '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [edgeFixture({ win: '24h' })] }),
      '/api/cards/meta': metaFixture({ tick: { last_tick_ms: Date.now() - 60_000, analyze_version: 'abc1234', tick_ms: 42 } }),
    });
    renderBox();
    expect(await screen.findByText('edge on the table')).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 20)); // let the meta response land
    expect(screen.getByText('edge on the table').closest('tr')).not.toHaveClass('dim');
  });

  it('shows capture rates as — with the reason while there is too little edge on the table', async () => {
    const thin = edgeFixture({ win: '24h' });
    thin.columns = thin.columns.map((r) => (r.label === 'edge on the table' ? { ...r, ours: '0.5' } : r));
    mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [thin] }) });
    renderBox();
    const row = (await screen.findByText('capture rate, sp-idle')).closest('tr') as HTMLTableRowElement;
    expect(row).toHaveTextContent('—');
    expect(row).not.toHaveTextContent('3.2%');
    expect(screen.getByText('sp-idle · 300 ms → —')).toBeInTheDocument();
    expect(screen.getByText('thin: too little edge on the table yet')).toBeInTheDocument();
  });
});
