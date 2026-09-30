import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useEpochs } from '../api/useEpochs';
import { nextAllowedText } from '../lib/epochs';
import { epochsFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { RESET_COPY, RESET_OFF, RESET_QUEUED, ResetFold } from './ResetFold';

const NOW = Date.UTC(2026, 8, 30, 12, 0);
const TOMORROW = Date.UTC(2026, 9, 1);
const pending = epochsFixture({ pending_request: { id: 3, requested_ms: NOW - 5_000 }, next_allowed_ms: TOMORROW });

function Harness() {
  const e = useEpochs();
  return <ResetFold epochs={e} now={NOW} />;
}

const posts = (f: ReturnType<typeof mockApi>) => f.mock.calls.filter((c) => c[1]?.method === 'POST');

describe('ResetFold', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps RESET off until the box reads reset, posts once for a double click, then says queued (review focus 4)', async () => {
    let queued = false;
    const f = mockApi({
      '/api/paper/epochs': () => (queued ? pending : epochsFixture()),
      '/api/paper/reset': (_u: URL, init?: RequestInit) => {
        if (init?.method !== 'POST') return jsonResponse(405, { error: 'method_not_allowed', detail: 'x' });
        queued = true;
        return jsonResponse(202, { request_id: 3, requested_ms: NOW });
      },
    });
    render(<Harness />);
    expect(await screen.findByText('epoch 1, started 2026-09-29 19:58 UTC (policies)')).toBeInTheDocument();
    expect(screen.getByText(RESET_COPY)).toBeInTheDocument();
    const box = screen.getByRole('textbox', { name: 'type reset to confirm' });
    const btn = screen.getByRole('button', { name: 'RESET' });
    await userEvent.type(box, 'rese');
    expect(btn).toBeDisabled();
    await userEvent.type(box, 't');
    expect(btn).toBeEnabled();
    fireEvent.click(btn);
    fireEvent.click(btn);
    expect(await screen.findByText(RESET_QUEUED)).toBeInTheDocument();
    expect(posts(f)).toHaveLength(1);
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it.each([
    ['409 reset_pending → queued, from the server', 409, 'reset_pending', 'x', () => pending, RESET_QUEUED],
    ['429 reset_daily_limit → next allowed', 429, 'reset_daily_limit', 'x', () => epochsFixture({ next_allowed_ms: TOMORROW }), nextAllowedText(TOMORROW)],
    ['503 reset_disabled → off', 503, 'reset_disabled', 'x', () => epochsFixture(), RESET_OFF],
    ['500 → the api detail, form kept', 500, 'internal', 'Something broke on our side. It has been written down.', () => epochsFixture(), 'Something broke on our side. It has been written down.'],
  ])('%s (review focus 4)', async (_name, status, code, detail, after, text) => {
    let posted = false;
    mockApi({
      '/api/paper/epochs': () => (posted ? after() : epochsFixture()),
      '/api/paper/reset': () => {
        posted = true;
        return jsonResponse(status, { error: code, detail });
      },
    });
    render(<Harness />);
    await userEvent.type(await screen.findByRole('textbox', { name: 'type reset to confirm' }), 'reset');
    await userEvent.click(screen.getByRole('button', { name: 'RESET' }));
    expect(await screen.findByText(text)).toBeInTheDocument();
    // D-09: a 500 keeps the form; every other answer replaces it
    if (status === 500) expect(screen.getByRole('textbox', { name: 'type reset to confirm' })).toBeInTheDocument();
  });

  it('asks again every 10 s only while a request is pending (review focus 4)', async () => {
    vi.useFakeTimers();
    let n = 0;
    const applied = epochsFixture({
      epochs: [...epochsFixture().epochs, { epoch: 2, start_ms: NOW, cause: 'reset' }],
      current: 2,
      next_allowed_ms: TOMORROW,
    });
    const f = mockApi({ '/api/paper/epochs': () => (++n <= 2 ? pending : applied) });
    render(<Harness />);
    const calls = () => f.mock.calls.filter((c) => String(c[0]) === '/api/paper/epochs').length;
    const tick = async (ms: number) => {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });
    };
    await tick(0);
    await tick(0);
    expect(calls()).toBe(1);
    expect(screen.getByText(RESET_QUEUED)).toBeInTheDocument();
    await tick(10_000);
    expect(calls()).toBe(2);
    await tick(10_000);
    expect(calls()).toBe(3);
    expect(screen.getByText('epoch 2, started 2026-09-30 12:00 UTC (reset)')).toBeInTheDocument();
    expect(screen.getByText(nextAllowedText(TOMORROW))).toBeInTheDocument();
    await tick(60_000);
    expect(calls()).toBe(3);
  });

  it("says when the next reset is allowed once today's is used, and offers no box", async () => {
    mockApi({ '/api/paper/epochs': epochsFixture({ next_allowed_ms: TOMORROW }) });
    render(<Harness />);
    expect(await screen.findByText(nextAllowedText(TOMORROW))).toBeInTheDocument();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('D-02: next_allowed_ms equal to now, or within 60 s of it, is allowed now: the form shows', async () => {
    mockApi({ '/api/paper/epochs': epochsFixture({ next_allowed_ms: NOW }) });
    const { unmount } = render(<Harness />);
    expect(await screen.findByRole('textbox', { name: 'type reset to confirm' })).toBeInTheDocument();
    expect(screen.queryByText(nextAllowedText(NOW))).toBeNull();
    unmount();
    mockApi({ '/api/paper/epochs': epochsFixture({ next_allowed_ms: NOW + 59_000 }) });
    render(<Harness />);
    expect(await screen.findByRole('textbox', { name: 'type reset to confirm' })).toBeInTheDocument();
  });

  it('D-02: without a now prop the fold reads the clock at render', async () => {
    mockApi({ '/api/paper/epochs': epochsFixture({ next_allowed_ms: Date.now() }) });
    function Fresh() {
      return <ResetFold epochs={useEpochs()} />;
    }
    render(<Fresh />);
    expect(await screen.findByRole('textbox', { name: 'type reset to confirm' })).toBeInTheDocument();
  });
});
