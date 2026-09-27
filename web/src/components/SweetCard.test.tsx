import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { CardBody } from '../api/types';
import { warmingText } from '../lib/cards';
import { SWEET_EMPTY } from '../lib/sweet';
import { CARD_NOW, sweetFixture, sweetRow } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { NOTHING_YET, RECOMPUTING, SWEET_CARD_TITLE, SweetCard, TOO_FEW } from './SweetCard';

const renderCard = () => render(<MemoryRouter><SweetCard /></MemoryRouter>);
const serve = (cards: CardBody[]) => mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards }) });

describe('SweetCard', () => {
  it('links its title to /sweet and asks for 30d', async () => {
    const f = serve([]);
    renderCard();
    expect(await screen.findByRole('link', { name: SWEET_CARD_TITLE })).toHaveAttribute('href', '/sweet');
    expect(String(f.mock.calls[0][0])).toBe('/api/cards?win=30d');
  });

  it('shows counts and the best candidate as one interval row', async () => {
    serve([sweetFixture([
      sweetRow({ dimension: 'hour', bucket: '12-16', label: 'candidate', lo: '0.04', mean: '0.31', hi: '0.58' }),
      sweetRow({ dimension: 'days', bucket: '1-7', label: 'candidate', lo: '0.01', mean: '0.1', hi: '0.2' }),
    ])]);
    renderCard();
    expect(await screen.findByText('NO 2 · YES 0 candidates')).toBeInTheDocument();
    expect(screen.getByText('12–16 UTC')).toBeInTheDocument();
    expect(screen.getByText('+$0.31 [+$0.04, +$0.58]')).toBeInTheDocument();
  });

  it('says nothing pays, too few, recomputing, warming, or missing', async () => {
    const cases: [CardBody[], string][] = [
      [[sweetFixture([sweetRow({ label: 'negative', lo: '-1', mean: '-0.5', hi: '-0.1' })])], NOTHING_YET],
      [[sweetFixture([sweetRow({ label: 'thin' })])], TOO_FEW],
      [[{ ...sweetFixture([sweetRow()]), status: 'under_review' }], RECOMPUTING],
      [[], SWEET_EMPTY],
    ];
    for (const [cards, text] of cases) {
      serve(cards);
      const r = renderCard();
      expect(await screen.findByText(text)).toBeInTheDocument();
      r.unmount();
    }
    const warm = { ...sweetFixture([sweetRow()]), status: 'warming' as const, since_ms: CARD_NOW - 2 * 86_400_000 };
    serve([warm]);
    renderCard();
    expect(await screen.findByText(warmingText(warm))).toBeInTheDocument();
  });
});
