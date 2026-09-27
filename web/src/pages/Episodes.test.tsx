import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { FLAG_WORDS } from '../lib/flags';
import { HELP } from '../help';
import { Episodes, NO_EPISODES } from './Episodes';

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route
          path="/episodes"
          element={
            <>
              <Episodes />
              <Loc />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );

describe('Episodes', () => {
  it('reads its filters from the URL and sends them to the api', async () => {
    const f = mockApi({
      '/api/episodes': { items: [episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190, suspect: true })], next_cursor: null },
    });
    renderAt('/episodes?kind=no_basket&net=1&flags=suspect&from=2026-09-20&to=2026-09-25');
    expect(await screen.findByRole('link', { name: 'Bitcoin price on September 26?' })).toHaveAttribute('href', '/episodes/live-1-00000001');
    const url = String(f.mock.calls[0][0]);
    for (const part of ['kind=no_basket', 'net=1', 'flags=suspect', `from=${Date.UTC(2026, 8, 20)}`, `to=${Date.UTC(2026, 8, 26)}`, 'limit=50']) {
      expect(url).toContain(part);
    }
    expect(screen.getByText('4.19 s')).toBeInTheDocument();
    expect(screen.getByText('SUSPECT')).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'net-positive only' })).toBeChecked();
  });

  it('changing a filter updates the URL and drops the cursor', async () => {
    const f = mockApi({ '/api/episodes': { items: [], next_cursor: null } });
    renderAt('/episodes?cursor=1_live-1-x');
    expect(await screen.findByText(NO_EPISODES)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'YES' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('?kind=yes_basket');
    await waitFor(() => expect(String(f.mock.calls.at(-1)?.[0])).toContain('kind=yes_basket'));
    expect(String(f.mock.calls.at(-1)?.[0])).not.toContain('cursor');
  });

  it('pages older with the cursor and back to the newest', async () => {
    mockApi({
      '/api/episodes': (url: URL) =>
        url.searchParams.get('cursor')
          ? { items: [episodeSummaryFixture({ id: 'live-1-00000000', event_title: 'Older one' })], next_cursor: null }
          : { items: [episodeSummaryFixture()], next_cursor: '1790000000000_live-1-00000001' },
    });
    renderAt('/episodes');
    await screen.findByText('Bitcoin price on September 26?');
    expect(screen.getByRole('button', { name: 'NEWEST' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'OLDER →' }));
    expect(await screen.findByText('Older one')).toBeInTheDocument();
    expect(screen.getByTestId('loc')).toHaveTextContent('cursor=1790000000000_live-1-00000001');
    expect(screen.getByRole('button', { name: 'OLDER →' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'NEWEST' }));
    expect(await screen.findByText('Bitcoin price on September 26?')).toBeInTheDocument();
  });

  it('shows the api detail for a range that is too long', async () => {
    mockApi({
      '/api/episodes': () => jsonResponse(400, { error: 'bad_range', detail: 'from must be before to, and seven days apart at most.' }),
    });
    renderAt('/episodes?from=2026-09-01&to=2026-09-25');
    expect(await screen.findByText('from must be before to, and seven days apart at most.')).toBeInTheDocument();
  });

  it('explains every flag it renders in a legend near the table, in the same words as the detail page', async () => {
    mockApi({ '/api/episodes': { items: [episodeSummaryFixture({ suspect: true, stale: true, censored: true })], next_cursor: null } });
    renderAt('/episodes');
    const legend = await screen.findByLabelText('flag legend');
    for (const w of [FLAG_WORDS.stale, FLAG_WORDS.suspect, FLAG_WORDS.censor_left, FLAG_WORDS.censor_right, FLAG_WORDS.censor_gap, FLAG_WORDS.incomplete]) {
      expect(legend).toHaveTextContent(w);
    }
    expect(screen.getByRole('button', { name: `Help: ${HELP.flags.title}` })).toBeInTheDocument();
    expect(HELP.flags.text).toContain(FLAG_WORDS.stale);
    expect(HELP.flags.text).toContain(FLAG_WORDS.censor_gap);
  });

  it('gives each flag tag a tooltip with its sentence', async () => {
    mockApi({ '/api/episodes': { items: [episodeSummaryFixture({ suspect: true, stale: true, censored: true })], next_cursor: null } });
    renderAt('/episodes');
    const row = (await screen.findByRole('link', { name: 'Bitcoin price on September 26?' })).closest('tr') as HTMLTableRowElement;
    expect(within(row).getByText('SUSPECT')).toHaveAttribute('title', FLAG_WORDS.suspect);
    expect(within(row).getByText('STALE')).toHaveAttribute('title', FLAG_WORDS.stale);
    const censored = within(row).getByText('CENSORED').getAttribute('title') ?? '';
    for (const w of [FLAG_WORDS.censor_left, FLAG_WORDS.censor_right, FLAG_WORDS.censor_gap]) expect(censored).toContain(w);
  });
});
