import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { healthFixture, healthPointFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Health, NO_HEALTH_ROWS } from './Health';

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route
          path="/health"
          element={
            <>
              <Health />
              <Loc />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );

const integrity = {
  from_ms: 0, to_ms: 1, by_kind: [{ kind: 'top_mismatch', count: 2 }, { kind: 'drift', count: 1 }], total: 3, msgs: 258_000, per_million: '11.63',
};

describe('Health', () => {
  it('shows capture status, per-minute charts with reconnect markers, and integrity per million', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ capture_paused: true }) },
      '/api/health/series': { from_ms: 0, to_ms: 180_000, points: [healthPointFixture(0, 0), healthPointFixture(60_000, 2), healthPointFixture(120_000, 0)] },
      '/api/integrity/summary': integrity,
    });
    const { container } = renderAt('/health');
    expect(await screen.findByText('3 events in 258000 messages: 11.63 per million.')).toBeInTheDocument();
    expect(await screen.findByText('PAUSED')).toBeInTheDocument();
    expect(screen.getByText('top_mismatch')).toBeInTheDocument();
    await waitFor(() => expect(container.querySelectorAll('rect.marker')).toHaveLength(1));
  });

  it('keeps the window in the URL', async () => {
    const f = mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': { ...integrity, by_kind: [], total: 0, msgs: 0, per_million: null },
    });
    renderAt('/health?hours=168');
    expect(await screen.findByText(NO_HEALTH_ROWS)).toBeInTheDocument();
    expect(f.mock.calls.map((c) => String(c[0]))).toContain('/api/health/series?hours=168');
    await userEvent.click(screen.getByRole('button', { name: '6 H' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('?hours=6');
    await waitFor(() => expect(f.mock.calls.map((c) => String(c[0]))).toContain('/api/health/series?hours=6'));
  });
});
