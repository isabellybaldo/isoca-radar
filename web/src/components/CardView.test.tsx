import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { UNDER_REVIEW } from '../lib/cards';
import { fmtLocal, fmtUtcDateTime } from '../lib/time';
import { cardFixture, CARD_NOW } from '../test/fixtures';
import type { CardBody } from '../api/types';
import { CardView } from './CardView';

const renderCard = (card: CardBody, stale = false) =>
  render(
    <MemoryRouter>
      <CardView card={card} stale={stale} />
    </MemoryRouter>,
  );

/** The row's label appears twice (the claim's dim caption and the folded table's rowlabel cell); find the <tr>. */
const tableRowFor = (text: string, count: number): HTMLTableRowElement => {
  const matches = screen.getAllByText(text);
  expect(matches).toHaveLength(count);
  const tr = matches.map((el) => el.closest('tr')).find((t): t is HTMLTableRowElement => t !== null);
  if (!tr) throw new Error(`no table row for ${text}`);
  return tr;
};

describe('CardView', () => {
  it('renders a live card: three columns, n, the short hash with the full one on hover, a (?)', () => {
    renderCard(cardFixture());
    const card = screen.getByRole('article', { name: 'G1 · Side asymmetry' });
    expect(within(card).getByText('LIVE')).toBeInTheDocument();
    for (const h of ['PAPER', 'THEIR METHOD', 'OUR METHOD']) expect(within(card).getByText(h)).toBeInTheDocument();
    expect(within(card).getAllByText('2,098').length).toBe(2);
    expect(within(card).getByText('30.26×')).toBeInTheDocument();
    expect(within(card).getByText('#3f2a9c1b7d4e')).toHaveAttribute('title', cardFixture().hash);
    expect(within(card).getByText(/n = 1240/)).toBeInTheDocument();
    expect(within(card).getByRole('button', { name: 'Help: What is side asymmetry?' })).toBeInTheDocument();
    expect(within(card).getByRole('img', { name: 'YES 1150, NO 38' })).toBeInTheDocument();
    const claim = card.querySelector('dl.claim') as HTMLElement;
    expect(within(claim).getByText('paper')).toBeInTheDocument();
    expect(within(claim).getByText('ours')).toBeInTheDocument();
    expect(within(card).getByText('all numbers').closest('details')).not.toHaveAttribute('open');
    expect(within(card).getByText("how it's computed").closest('details')).not.toHaveAttribute('open');
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
    expect(screen.getAllByText('0')).toHaveLength(4);
    expect(tableRowFor('NO-side episodes', 2)).toHaveClass('dim');
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
    // the claim (first row, big type) and the folded table both print the headline value.
    expect(screen.getAllByText('$12.50')).toHaveLength(2);
    expect(screen.getByRole('link', { name: 'biggest opportunity →' })).toHaveAttribute('href', '/episodes/live-9-00000042');
  });

  it('rounds a USD value row to the cent, with the exact value in a title tooltip', () => {
    renderCard(
      cardFixture({
        card: 'EDGE', title: 'Edge capture',
        columns: [
          { label: 'edge on the table', unit: 'USD', paper: null, their: null, ours: '12.567755' },
          { label: 'biggest single opportunity', unit: 'USD', paper: null, their: null, ours: null },
        ],
        series: {},
      }),
    );
    // the claim (first row) and the folded table both print the rounded headline value.
    const rounded = screen.getAllByText('$12.57');
    expect(rounded).toHaveLength(2);
    for (const el of rounded) expect(el).toHaveAttribute('title', '$12.567755');
    expect(screen.getByText('biggest single opportunity').closest('tr')).toHaveTextContent('—');
  });

  it('dims every number when the results are stale', () => {
    renderCard(cardFixture(), true);
    expect(tableRowFor('YES-side episodes', 2)).toHaveClass('dim');
  });

  it('shows the premise key in the footer, next to n and the hash, dim and wrapping', () => {
    renderCard(cardFixture());
    const card = screen.getByRole('article', { name: 'G1 · Side asymmetry' });
    const nLine = within(card).getByText(/n = 1240/);
    const premise = within(card).getByText(cardFixture().premise_key);
    expect(premise.tagName).toBe('P');
    expect(premise).toHaveClass('meta', 'premise');
    // "next to": the premise line immediately follows the n/hash line in the footer.
    expect(nLine.nextElementSibling).toBe(premise);
  });

  it('dims the rows while the premises are under review, so a catch-up 0 never reads as a result', () => {
    renderCard(cardFixture({ status: 'under_review' }));
    expect(tableRowFor('YES-side episodes', 2)).toHaveClass('dim');
  });

  it('says how long ago its own body was computed, UTC on the page and local time on hover', () => {
    const computed = CARD_NOW - 2 * 3_600_000; // a 30d/all body re-renders at most hourly
    render(
      <MemoryRouter>
        <CardView card={cardFixture({ computed_ms: computed })} now={CARD_NOW + 7 * 60_000} />
      </MemoryRouter>,
    );
    const age = screen.getByText(/^computed 127 min ago/);
    expect(age).toHaveTextContent(`computed 127 min ago (${fmtUtcDateTime(computed)})`);
    expect(age).toHaveAttribute('title', fmtLocal(computed));
  });
});
