import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture, healthFixture, todayFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { EMPTY_RADAR, NO_HEALTH, Radar, msgsPerSecond, todayQuip } from './Radar';

const renderRadar = () =>
  render(
    <MemoryRouter>
      <Radar />
    </MemoryRouter>,
  );

describe('Radar', () => {
  it('shows the deadpan empty state and ten hearts on a quiet, healthy day', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(0, 0),
    });
    renderRadar();
    expect(await screen.findByText(EMPTY_RADAR)).toBeInTheDocument();
    expect(EMPTY_RADAR).toBe('0 opportunities. The market is efficient, or we are slow. Probably both.');
    expect(await screen.findByRole('img', { name: 'health 10 of 10' })).toBeInTheDocument();
    expect(screen.getByText('OPEN EPISODES · 0')).toBeInTheDocument();
    expect(screen.getByText('40/40')).toBeInTheDocument();
    expect(screen.getByText('4.3k')).toBeInTheDocument();
  });

  it('lists open episodes with a link to the detail, signed money and Q*', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [episodeSummaryFixture()] },
      '/api/today': todayFixture(1, 1),
    });
    renderRadar();
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    expect(link).toHaveAttribute('href', '/episodes/live-1-00000001');
    expect(screen.getByText('+$0.41')).toHaveClass('pos');
    expect(screen.getByText('+$1.37')).toBeInTheDocument();
    expect(screen.getByText('83')).toBeInTheDocument();
    expect(screen.getByText('NO')).toBeInTheDocument();
    expect(screen.getByText('OPEN EPISODES · 1')).toBeInTheDocument();
  });

  it('says today in the spec voice', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(372, 41),
    });
    renderRadar();
    expect(await screen.findByText('372 gross, 41 net. "Fees: 331. Us: 41."')).toBeInTheDocument();
    expect(todayQuip(3, 0)).toBe('3 gross, 0 net. "Fees: 3. Us: 0."');
    expect(msgsPerSecond(59)).toBe('1');
  });

  it('names health troubles in text, not colour alone', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ drops_5m: 2, capture_paused: true }) },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(0, 0),
    });
    renderRadar();
    expect(await screen.findByRole('img', { name: /^health 8 of 10/ })).toBeInTheDocument();
    expect(screen.getByText('DROPS IN THE LAST 5 MIN')).toBeInTheDocument();
    expect(screen.getByText('CAPTURE PAUSED')).toBeInTheDocument();
  });

  it('explains a missing health row and a busy database instead of going blank', async () => {
    mockApi({
      '/api/health/now': { health: null },
      '/api/episodes/open': () =>
        jsonResponse(503, { error: 'db_unavailable', detail: 'The database is thinking. Try again in a moment.' }),
      '/api/today': todayFixture(0, 0),
    });
    renderRadar();
    expect(await screen.findByText(NO_HEALTH)).toBeInTheDocument();
    expect(await screen.findByText('The database is thinking. Try again in a moment.')).toBeInTheDocument();
  });
});
