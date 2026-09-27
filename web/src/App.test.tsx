import { act, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { StreamProvider } from './api/StreamContext';
import { FakeEventSource } from './test/fakeEventSource';

describe('App shell', () => {
  beforeEach(() => {
    FakeEventSource.instances = [];
  });

  it('has the logo, the nav, a hollow live dot, the fake toggle and a deadpan 404', () => {
    render(
      <MemoryRouter initialEntries={['/nowhere']}>
        <App />
      </MemoryRouter>,
    );
    expect(screen.getByText('♥ isoca-radar')).toBeInTheDocument();
    for (const name of ['RADAR', 'SWEET', 'EPISODES', 'PAPER', 'PAPERS', 'HEALTH']) {
      expect(screen.getByRole('link', { name })).toBeInTheDocument();
    }
    expect(screen.getByText('RECONNECTING')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Toggle theme' })).toBeInTheDocument();
    expect(screen.getByText('This page does not exist. Neither does free money.')).toBeInTheDocument();
  });

  it('turns the live dot solid when the stream opens', () => {
    vi.stubGlobal('EventSource', FakeEventSource);
    render(
      <MemoryRouter initialEntries={['/nowhere']}>
        <StreamProvider>
          <App />
        </StreamProvider>
      </MemoryRouter>,
    );
    act(() => FakeEventSource.instances[0].open());
    expect(screen.getByText('LIVE')).toBeInTheDocument();
  });
});
