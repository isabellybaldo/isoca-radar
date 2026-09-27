import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { cardFixture, CARD_NOW, metaFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { NO_CARDS, Papers } from './Papers';
import { HELP } from '../help';

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route path="/papers" element={<><Papers /><Loc /></>} />
      </Routes>
    </MemoryRouter>,
  );

const four = ['G1', 'G2', 'G3', 'G4'].map((c) => cardFixture({ card: c, title: `${c} · x` }));

describe('Papers', () => {
  it('asks for 7D by default and shows the Gebele cards, Saguillo later, Cheng dropped, and what is not replicated', async () => {
    const fetch = mockApi({
      '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: four }),
      '/api/cards/meta': metaFixture(),
    });
    const real = Date.now;
    Date.now = () => CARD_NOW; // metaFixture's tick is 5 minutes before CARD_NOW
    try {
    renderAt('/papers');
    expect(await screen.findByRole('article', { name: 'G4 · x' })).toBeInTheDocument();
    expect(screen.getAllByRole('article')).toHaveLength(4);
    expect(String(fetch.mock.calls.find((c) => String(c[0]).startsWith('/api/cards?'))?.[0])).toBe('/api/cards?win=7d');
    expect(screen.getByRole('heading', { level: 1, name: 'PAPERS' })).toBeInTheDocument();
    expect(HELP.papers.text.startsWith('Their number, their method on our data, our method. Same market, different weeks.')).toBe(true);
    expect(screen.getByRole('button', { name: '7D' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('coming later: needs the on-chain dataset and somewhere to put it that is not your laptop')).toBeInTheDocument();
    expect(screen.getByText('DROPPED')).toBeInTheDocument();
    expect(screen.getByText('not replicated (2)').closest('details')).not.toHaveAttribute('open');
    expect(screen.getByRole('button', { name: 'Help: What is not replicated, and why?' })).toBeInTheDocument();
    expect(screen.getByText(/308,416,666 messages/)).toBeInTheDocument();
    expect(screen.getByText('updated 5 min ago')).not.toHaveClass('stale');
    // M4: the live paper's box is the page's hero; the dropped/coming-later ones are not.
    expect(screen.getByText(/^Gebele, Mutzel, Matthes/).closest('section')).toHaveClass('hero');
    expect(screen.getByText('DROPPED').closest('section')).not.toHaveClass('hero');
    } finally {
      Date.now = real;
    }
  });

  it('switches the window in the URL and refetches', async () => {
    const fetch = mockApi({
      '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: four }),
      '/api/cards/meta': metaFixture(),
    });
    renderAt('/papers');
    await screen.findByRole('article', { name: 'G1 · x' });
    await userEvent.click(screen.getByRole('button', { name: '30D' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('?win=30d');
    expect(fetch.mock.calls.some((c) => String(c[0]) === '/api/cards?win=30d')).toBe(true);
  });

  it('dims the updated line when the last tick is older than 45 minutes and shows a failed tick', async () => {
    mockApi({
      '/api/cards': { win: '7d', cards: four },
      '/api/cards/meta': metaFixture({
        tick: { last_tick_ms: CARD_NOW - 50 * 60_000, analyze_version: 'abc', tick_ms: 1 },
        last_error: { ms: CARD_NOW, text: 'database: statement timeout' },
      }),
    });
    const real = Date.now;
    Date.now = () => CARD_NOW;
    try {
      renderAt('/papers');
      expect(await screen.findByText('updated 50 min ago')).toHaveClass('stale');
      expect(screen.getByText('last tick failed: database: statement timeout')).toBeInTheDocument();
    } finally {
      Date.now = real;
    }
  });

  it('says so when a live paper has no cards yet', async () => {
    mockApi({ '/api/cards': { win: '7d', cards: [] }, '/api/cards/meta': metaFixture() });
    renderAt('/papers?win=all');
    expect(await screen.findByText(NO_CARDS)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'ALL' })).toHaveAttribute('aria-pressed', 'true');
  });
});
