import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { UNDER_REVIEW } from '../lib/cards';
import { cardFixture, CARD_NOW } from '../test/fixtures';
import type { CardBody } from '../api/types';
import { CardView } from './CardView';

const renderCard = (card: CardBody, stale = false) =>
  render(
    <MemoryRouter>
      <CardView card={card} stale={stale} />
    </MemoryRouter>,
  );

describe('CardView', () => {
  it('renders a live card: three columns, n, the short hash with the full one on hover, a (?)', () => {
    renderCard(cardFixture());
    const card = screen.getByRole('article', { name: 'G1 · Side asymmetry' });
    expect(within(card).getByText('LIVE')).toBeInTheDocument();
    for (const h of ['PAPER', 'THEIR METHOD', 'OUR METHOD']) expect(within(card).getByText(h)).toBeInTheDocument();
    expect(within(card).getByText('2,098')).toBeInTheDocument();
    expect(within(card).getByText('30.26×')).toBeInTheDocument();
    expect(within(card).getByText('#3f2a9c1b7d4e')).toHaveAttribute('title', cardFixture().hash);
    expect(within(card).getByText(/n = 1240/)).toBeInTheDocument();
    expect(within(card).getByRole('button', { name: 'Help: What is side asymmetry?' })).toBeInTheDocument();
    expect(within(card).getByRole('img', { name: 'YES 1150, NO 38' })).toBeInTheDocument();
  });

  it('renders warming with since and days of data', () => {
    renderCard(cardFixture({ status: 'warming', since_ms: CARD_NOW - 3 * 86_400_000 }));
    expect(screen.getByText('WARMING')).toBeInTheDocument();
    expect(screen.getByText(/· 3 days of data$/)).toBeInTheDocument();
  });

  it('renders thin: numbers dimmed, the reason stated, null as — and "0" as 0', () => {
    renderCard(
      cardFixture({
        status: 'thin',
        columns: [
          { label: 'NO-side episodes', unit: 'count', paper: '36', their: '0', ours: '0' },
          { label: 'YES : NO', unit: '×', paper: '58.28', their: null, ours: null },
        ],
        thin_reasons: ['NO side: n = 0 < 10, so the ratio is not reliable'],
      }),
    );
    expect(screen.getByText('THIN')).toBeInTheDocument();
    expect(screen.getByText('thin: NO side: n = 0 < 10, so the ratio is not reliable')).toBeInTheDocument();
    expect(screen.getAllByText('—')).toHaveLength(2);
    expect(screen.getAllByText('0')).toHaveLength(2);
    expect(screen.getByText('NO-side episodes').closest('tr')).toHaveClass('dim');
  });

  it('renders under review', () => {
    renderCard(cardFixture({ status: 'under_review' }));
    expect(screen.getByText('UNDER REVIEW')).toBeInTheDocument();
    expect(screen.getByText(UNDER_REVIEW)).toBeInTheDocument();
  });

  it('hides empty method columns and links the biggest opportunity', () => {
    renderCard(
      cardFixture({
        card: 'EDGE', title: 'Edge capture',
        columns: [{ label: 'edge on the table', unit: 'USD', paper: null, their: null, ours: '12.5' }],
        series: {}, episode_link: 'live-9-00000042',
      }),
    );
    expect(screen.queryByText('PAPER')).toBeNull();
    expect(screen.getByText('VALUE')).toBeInTheDocument();
    expect(screen.getByText('$12.50')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'biggest opportunity →' })).toHaveAttribute('href', '/episodes/live-9-00000042');
  });

  it('dims every number when the results are stale', () => {
    renderCard(cardFixture(), true);
    expect(screen.getByText('YES-side episodes').closest('tr')).toHaveClass('dim');
  });
});
