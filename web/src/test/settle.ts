import { act } from '@testing-library/react';
import { vi } from 'vitest';

/** Let pending fetches land: flush with fake timers, or wait 10 ms with real ones, until the fetch mock
 *  has been quiet for three rounds (pages chain fetches: accounts, then one equity per account). */
export async function settle(f: { mock: { calls: unknown[] } }): Promise<void> {
  let quiet = 0;
  let n = f.mock.calls.length;
  for (let i = 0; i < 100 && quiet < 3; i++) {
    await act(async () => {
      if (vi.isFakeTimers()) await vi.advanceTimersByTimeAsync(0);
      else await new Promise((r) => setTimeout(r, 10));
    });
    if (f.mock.calls.length === n) quiet += 1;
    else {
      quiet = 0;
      n = f.mock.calls.length;
    }
  }
}
