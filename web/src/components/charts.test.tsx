import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { CHART_EMPTY, LineChart, toColumns } from './LineChart';
import { Spark } from './Spark';

describe('charts', () => {
  it('Spark draws one crisp pixel bar per value, zero as empty', () => {
    const { container } = render(<Spark values={[0, 3, 6]} label="episodes per hour" />);
    const svg = container.querySelector('svg');
    expect(svg).toHaveAttribute('shape-rendering', 'crispEdges');
    const rects = [...container.querySelectorAll('rect')];
    expect(rects.map((r) => r.getAttribute('height'))).toEqual(['0', '6', '12']);
  });

  it('LineChart draws step rects, markers and a text legend', () => {
    const { container } = render(
      <LineChart
        label="equity"
        xMin={0}
        xMax={100}
        markers={[50]}
        series={[{ label: 'P0 converter', tone: 'p0', points: [{ x: 0, y: 1 }, { x: 50, y: 2 }, { x: 100, y: 1 }] }]}
      />,
    );
    expect(container.querySelector('svg')).toHaveAttribute('shape-rendering', 'crispEdges');
    expect(container.querySelectorAll('g.line rect').length).toBeGreaterThanOrEqual(3);
    expect(container.querySelectorAll('rect.marker')).toHaveLength(1);
    expect(screen.getByText('P0 converter')).toBeInTheDocument();
  });

  it('LineChart with no points says so instead of drawing nothing', () => {
    render(<LineChart label="empty" xMin={0} xMax={1} series={[{ label: 'a', tone: 'p0', points: [] }]} />);
    expect(screen.getByText(CHART_EMPTY)).toBeInTheDocument();
  });

  it('toColumns keeps at most one point per pixel column (the last one)', () => {
    const pts = Array.from({ length: 10_080 }, (_, i) => ({ x: i, y: i }));
    const cols = toColumns(pts, 0, 10_079, 240);
    expect(cols.length).toBeLessThanOrEqual(240);
    expect(cols[cols.length - 1]).toEqual({ x: 10_079, y: 10_079 });
  });
});
