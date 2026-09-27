import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { sweetRow } from '../test/fixtures';
import { IntervalChart, NO_ROWS } from './IntervalChart';

const num = (el: Element | null, a: string) => Number(el?.getAttribute(a));

describe('IntervalChart', () => {
  const rows = [
    sweetRow({ dimension: 'hour', bucket: '12-16', label: 'candidate', lo: '0.04', mean: '0.31', hi: '0.58', n_exec: 40, clusters: 33 }),
    sweetRow({ dimension: 'hour', bucket: '00-04', label: 'negative', lo: '-0.3', mean: '-0.2', hi: '-0.1', few_trades: true }),
    sweetRow({ dimension: 'hour', bucket: '04-08', label: 'thin', lo: '-0.5', mean: '0.1', hi: '0.9' }),
    sweetRow({ dimension: 'hour', bucket: '08-12', label: 'not_tested' }),
  ];

  it('draws candidates right of zero, filled, and negatives left of zero, hollow', () => {
    const { container } = render(<IntervalChart rows={rows} label="NO by hour at sp-idle" />);
    expect(screen.getByRole('list', { name: 'NO by hour at sp-idle' })).toBeInTheDocument();
    const cand = container.querySelector('li[data-label="candidate"]')!;
    const neg = container.querySelector('li[data-label="negative"]')!;
    const zeroX = num(cand.querySelector('rect.zero'), 'x');
    expect(num(cand.querySelector('line.bar'), 'x1')).toBeGreaterThan(zeroX);
    expect(num(neg.querySelector('line.bar'), 'x2')).toBeLessThan(zeroX);
    expect(cand.querySelector('rect.dot')).not.toHaveClass('hollow');
    expect(neg.querySelector('rect.dot')).toHaveClass('hollow');
    expect(within(cand as HTMLElement).getByText('CANDIDATE')).toHaveAttribute('title', 'survives here; worth a real small-order test.');
    expect(within(cand as HTMLElement).getByText('12–16 UTC')).toBeInTheDocument();
    expect(within(cand as HTMLElement).getByText('n 40 · C 33')).toBeInTheDocument();
    expect(within(neg as HTMLElement).getByText('few trades carry this row')).toBeInTheDocument();
  });

  it('dashes thin rows and draws no bar without a mean', () => {
    const { container } = render(<IntervalChart rows={rows} label="x" />);
    expect(container.querySelector('li[data-label="thin"] line.bar')).toHaveClass('dashed');
    const nt = container.querySelector('li[data-label="not_tested"]')!;
    expect(nt.querySelector('line.bar')).toBeNull();
    expect(nt.querySelector('rect.dot')).toBeNull();
    expect(within(nt as HTMLElement).getByText('—')).toBeInTheDocument();
  });

  it('shows the YES annualized rate as a percent', () => {
    render(<IntervalChart rows={[sweetRow({ side: 'yes', label: 'candidate', lo: '0.01', mean: '0.1', hi: '0.2', annualized: '0.12' })]} label="x" />);
    expect(screen.getByText(/12%\/yr/)).toBeInTheDocument();
  });

  it('says so when the slice is empty', () => {
    render(<IntervalChart rows={[]} label="x" />);
    expect(screen.getByText(NO_ROWS)).toBeInTheDocument();
  });
});
