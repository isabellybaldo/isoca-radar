import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router';
import { describe, expect, it } from 'vitest';
import { episodeDetailFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { EpisodeDetail, censorText, recheckLabel } from './EpisodeDetail';

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route path="/episodes/:id" element={<EpisodeDetail />} />
      </Routes>
    </MemoryRouter>,
  );

describe('EpisodeDetail', () => {
  it('shows timing, the peak, flags in words, legs with S dimming, and the re-check timeline', async () => {
    mockApi({ '/api/episodes/live-1-00000001': episodeDetailFixture() });
    const { container } = renderAt('/episodes/live-1-00000001');
    expect(await screen.findByText('Bitcoin price on September 26?')).toBeInTheDocument();
    expect(screen.getByText('NO BASKET')).toBeInTheDocument();
    expect(screen.getByText('SUSPECT')).toBeInTheDocument();
    expect(screen.getByText(/^SUSPECT: one leg's book disagreed with the server's top/)).toBeInTheDocument();
    expect(screen.getByText('01:12:04.210 UTC')).toBeInTheDocument();
    expect(screen.getByText('4.19 s')).toBeInTheDocument();
    expect(screen.getByText('84 ms')).toBeInTheDocument();
    expect(screen.getByText('none')).toBeInTheDocument();
    expect(screen.getByText('+$0.41')).toHaveClass('pos');
    expect(screen.getByText('$0.00')).toBeInTheDocument();
    expect(screen.getByText('83 sets')).toBeInTheDocument();
    expect(screen.getByText('+$0.33')).toBeInTheDocument();
    expect(screen.getByText('LEGS AT PEAK · S = 1 of 7')).toBeInTheDocument();
    expect(screen.getByText('#1 m1').closest('tr')).toHaveClass('dim');
    expect(screen.getByText('#0 m0').closest('tr')).not.toHaveClass('dim');
    for (const t of ['peak', '+1 s', '+Δd (3 s)', '+5 s', 'skipped: recheck capacity', '+0.0022', '−0.0008']) {
      expect(screen.getByText(t)).toBeInTheDocument();
    }
    expect(container.querySelectorAll('svg.tl rect.bad')).toHaveLength(1);
    expect(container.querySelectorAll('svg.tl rect.skip')).toHaveLength(1);
    expect(screen.getByText('Gone in 4.19 s. Like most good ideas.')).toBeInTheDocument();
  });

  it('a missing episode is a deadpan 404, not a blank page', async () => {
    mockApi({});
    renderAt('/episodes/nope');
    expect(await screen.findByText('Nothing here. It may have been a dream.')).toBeInTheDocument();
  });

  it('labels re-checks and censoring in words', () => {
    expect(recheckLabel(1000, 0)).toBe('+1 s');
    expect(recheckLabel(3000, 3)).toBe('+Δd (3 s)');
    expect(recheckLabel(1500, null)).toBe('+1.50 s');
    const e = episodeDetailFixture({ censor_left: true, censor_gap: true }).episode;
    expect(censorText(e)).toBe('left, gap');
  });
});
