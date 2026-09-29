import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { App } from '../App';
import { appRoutes } from '../test/appRoutes';
import { episodeSummaryFixture, positionFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';

const renderApp = (path: string) =>
  render(
    <MemoryRouter initialEntries={[path]}>
      <App />
    </MemoryRouter>,
  );
const openPanel = () => {
  fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
  return screen.getByRole('complementary', { name: 'explain' });
};
const panelNow = () => screen.getByRole('complementary', { name: 'explain' });

describe('pages publish, rows select (spec 7a §9.3)', () => {
  it('a Radar row click selects it; the panel says so; "back to the page" clears it', async () => {
    mockApi(appRoutes());
    renderApp('/');
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    const panel = openPanel();
    expect(within(panel).getByText(/^as selected: Bitcoin price on September 26\?/)).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole('button', { name: 'back to the page' }));
    expect(within(panel).queryByText(/as selected/)).toBeNull();
  });

  it('leaving the page clears the selection (review focus 5)', async () => {
    mockApi(appRoutes());
    renderApp('/');
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    expect(within(openPanel()).getByText(/^as selected:/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('link', { name: 'EPISODES' }));
    expect(await screen.findByRole('link', { name: 'Bitcoin price on September 26?' })).toBeInTheDocument();
    expect(within(panelNow()).queryByText(/as selected/)).toBeNull();
  });

  it('a selection is a snapshot: it stays when its row leaves the list (review focus 5)', async () => {
    mockApi(appRoutes({ '/api/episodes': (u: URL) => ({ items: u.searchParams.get('kind') ? [] : [episodeSummaryFixture()], next_cursor: null }) }));
    renderApp('/episodes');
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    fireEvent.click(screen.getByRole('button', { name: 'NO' }));
    await screen.findByText(/no episodes|nothing/i);
    expect(screen.queryByRole('link', { name: 'Bitcoin price on September 26?' })).toBeNull();
    expect(within(openPanel()).getByText(/^as selected: Bitcoin price on September 26\?/)).toBeInTheDocument();
  });

  it('an Episodes row and a Paper position select too', async () => {
    mockApi(appRoutes());
    const a = renderApp('/episodes');
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    expect(within(openPanel()).getByText(/^as selected: Bitcoin price on September 26\?/)).toBeInTheDocument();
    a.unmount();
    renderApp('/paper?scenario=sp-idle');
    const cell = await screen.findByText('Fed decision in October?');
    fireEvent.click(cell.closest('tr') as HTMLTableRowElement);
    expect(within(panelNow()).getByText(/^as selected: Fed decision in October\?/)).toBeInTheDocument();
  });

  it('D-12: the Episode detail page sets the selection on mount and clears it on unmount', async () => {
    mockApi(appRoutes());
    renderApp('/episodes/live-1-00000001');
    const panel = openPanel();
    expect(await within(panel).findByText(/^as selected: /)).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole('link', { name: 'EPISODES' })[0]);
    await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    expect(within(panelNow()).queryByText(/as selected/)).toBeNull();
  });

  it('fallback labels: an episode without a title shows its id, a position without one its market id', async () => {
    mockApi(
      appRoutes({
        '/api/episodes': { items: [episodeSummaryFixture({ id: 'live-9-untitled', event_title: null })], next_cursor: null },
        '/api/paper/positions': { items: [positionFixture({ market_id: 'm-untitled', event_title: null })] },
      }),
    );
    const a = renderApp('/episodes');
    const link = await screen.findByRole('link', { name: /live-9-untitled/ });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    expect(within(openPanel()).getByText(/^as selected: live-9-untitled/)).toBeInTheDocument();
    a.unmount();
    renderApp('/paper?scenario=sp-idle');
    const cell = await screen.findByText(/m-untitled/);
    fireEvent.click(cell.closest('tr') as HTMLTableRowElement);
    expect(within(panelNow()).getByText(/^as selected: m-untitled/)).toBeInTheDocument();
  });
});
