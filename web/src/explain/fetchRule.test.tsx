import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { appRoutes } from '../test/appRoutes';
import { mockApi } from '../test/mockApi';
import { settle } from '../test/settle';

const PATHS = ['/', '/sweet', '/episodes', '/episodes/live-1-00000001', '/paper', '/papers', '/health', '/nowhere'];
const pathOf = (c: unknown[]) => new URL(String(c[0]), 'http://localhost').pathname;

describe('the explain panel fetch rule (spec 7a §9.3)', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  for (const p of PATHS) {
    it(`${p}: with ticks and SSE frozen, opening adds exactly one call, to /api/paper/ladder; opening every question adds none`, async () => {
      vi.useFakeTimers(); // no ticker fires; there is no StreamProvider, so no EventSource either
      const f = mockApi(appRoutes());
      render(
        <MemoryRouter initialEntries={[p]}>
          <App />
        </MemoryRouter>,
      );
      await settle(f);
      const before = f.mock.calls.length;
      fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
      await settle(f);
      expect(f.mock.calls.slice(before).map(pathOf)).toEqual(['/api/paper/ladder']);
      const panel = screen.getByRole('complementary', { name: 'explain' });
      for (const b of [...panel.querySelectorAll<HTMLButtonElement>('button.explain-q')]) {
        fireEvent.click(b);
        await settle(f);
      }
      expect(f.mock.calls.length).toBe(before + 1);
    });
  }
});
