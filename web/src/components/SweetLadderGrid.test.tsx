import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { GRID_HINT } from '../lib/sweet';
import { sweetRow } from '../test/fixtures';
import { SweetLadderGrid } from './SweetLadderGrid';

const rows = [
  sweetRow({ scenario: 'rung-0', latency_ms: 0, label: 'candidate', mean: '0.2', lo: '0.05', hi: '0.35', n_exec: 40, clusters: 33 }),
  sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'rung-0', latency_ms: 0, label: 'candidate', mean: '0.2', lo: '0.05', hi: '0.35', n_exec: 40, clusters: 33 }),
];

describe('SweetLadderGrid', () => {
  it('is a table of buttons named by bucket, rung and verdict; focus shows the detail line under it', () => {
    render(<SweetLadderGrid rows={rows} side="no" dim="category" label="NO · CATEGORY" />);
    const t = screen.getByRole('table', { name: 'NO · CATEGORY' });
    expect(within(t).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['ms', '0', '25', '52', '100', '200', '300']);
    expect(within(t).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['all', 'crypto']);
    expect(within(t).getAllByRole('button')).toHaveLength(12);
    const b = within(t).getByRole('button', { name: 'crypto at rung-0 (0 ms): candidate' });
    expect(b).toHaveTextContent('●');
    expect(within(t).getByRole('button', { name: 'crypto at rung-25 (25 ms): no attempts' })).toHaveTextContent('·');
    expect(screen.getByText(GRID_HINT)).toBeInTheDocument();
    fireEvent.focus(b);
    expect(b).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('crypto · rung-0 · n 40 · C 33 · captured at 0 ms, per $100: +$0.20 [+$0.05, +$0.35] · survives here; worth a real small-order test.')).toBeInTheDocument();
  });

  it('shows the legend beside the grid, in words', () => {
    render(<SweetLadderGrid rows={rows} side="no" dim="all" label="NO · ALL" />);
    const legend = screen.getByRole('list', { name: 'grid legend' });
    expect(within(legend).getAllByRole('listitem').map((li) => li.textContent)).toEqual([
      '● candidate', '○ negative', '◌ inconclusive', '◐ thin, mean > 0', '◑ thin, mean ≤ 0', '· no attempts', '✗ dead on paper', '– not tested',
    ]);
  });
});
