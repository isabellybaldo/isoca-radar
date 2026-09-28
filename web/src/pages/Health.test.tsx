import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { healthFixture, healthPointFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Health, NO_ARCHIVE, NO_HEALTH_ROWS } from './Health';

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
  afterEach(() => {
    vi.useRealTimers();
  });

  it('refetches every 30 s even without an SSE event, so a stuck scanner or stream still updates', async () => {
    vi.useFakeTimers();
    const f = mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': integrity,
    });
    renderAt('/health');
    const calls = (path: string) => f.mock.calls.filter((c) => String(c[0]).startsWith(path)).length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(calls('/api/health/now')).toBe(1);
    expect(calls('/api/health/series')).toBe(1);
    expect(calls('/api/integrity/summary')).toBe(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(calls('/api/health/now')).toBe(2);
    expect(calls('/api/health/series')).toBe(2);
    expect(calls('/api/integrity/summary')).toBe(2);
  });

  it('shows capture status, per-minute charts with reconnect markers, and integrity per million', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ capture_paused: true }) },
      '/api/health/series': { from_ms: 0, to_ms: 180_000, points: [healthPointFixture(0, 0), healthPointFixture(60_000, 2), healthPointFixture(120_000, 0)] },
      '/api/integrity/summary': integrity,
    });
    const { container } = renderAt('/health');
    expect(await screen.findByText('3 events in 258,000 messages: 11.63 per million.')).toBeInTheDocument();
    expect(await screen.findByText('PAUSED')).toBeInTheDocument();
    expect(screen.getByText('top_mismatch')).toBeInTheDocument();
    await waitFor(() => expect(container.querySelectorAll('rect.marker')).toHaveLength(1));
    expect(screen.getByRole('heading', { level: 1, name: 'HEALTH' })).toBeInTheDocument();
    const tiles = screen.getAllByRole('listitem').filter((li) => li.classList.contains('tile')).map((li) => li.textContent);
    expect(tiles[1]).toBe('shards40/40');
    expect(screen.getByRole('list', { name: 'integrity by kind' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Help: What is capture?' })).toBeInTheDocument();
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

  it('sorts integrity by_kind by count descending', async () => {
    const unsorted = {
      from_ms: 0, to_ms: 1, by_kind: [{ kind: 'drift', count: 1 }, { kind: 'top_mismatch', count: 2 }], total: 3, msgs: 258_000, per_million: '11.63',
    };
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': unsorted,
    });
    const { container } = renderAt('/health');
    await screen.findByText('3 events in 258,000 messages: 11.63 per million.');
    const bars = [...container.querySelectorAll('.cb-bar')] as HTMLElement[];
    expect(bars[0].style.width).toBe('100%');
    expect(bars[1].style.width).toBe('50%');
  });

  it('shows the archive size, oldest day, compactor lag and last error', async () => {
    mockApi({
      '/api/health/now': {
        health: healthFixture({ archive_bytes: 12_345_678_901, archive_oldest_day: '2026-09-28', compact_lag_hours: 4, compact_last_error: 'read episodes: timeout', compact_age_ms: 120_000 }),
      },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': integrity,
    });
    renderAt('/health');
    expect(await screen.findByText('12.3 GB')).toBeInTheDocument();
    expect(screen.getByText('2026-09-28')).toBeInTheDocument();
    expect(screen.getByText('4 h')).toBeInTheDocument();
    expect(screen.getByText('BEHIND')).toBeInTheDocument();
    expect(screen.getByText('read episodes: timeout')).toBeInTheDocument();
  });

  it('says so when the compactor has not reported yet', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ archive_bytes: null, archive_oldest_day: null, compact_lag_hours: null, compact_last_error: null, compact_age_ms: null, compact_started_ms: null }) },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': integrity,
    });
    renderAt('/health');
    expect(await screen.findByText(NO_ARCHIVE)).toBeInTheDocument();
  });
});
