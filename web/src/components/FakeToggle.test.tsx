import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { FakeToggle, NIGHT_ONLY } from './FakeToggle';

function themeSnapshot() {
  const html = document.documentElement;
  return {
    attrs: html.getAttributeNames().map((n) => [n, html.getAttribute(n)]),
    htmlClass: html.className,
    htmlStyle: html.style.cssText,
    bodyClass: document.body.className,
    bodyStyle: document.body.style.cssText,
    storage: localStorage.length,
  };
}

describe('FakeToggle', () => {
  it('says "Night mode only." and never changes the theme', async () => {
    const before = themeSnapshot();
    render(<FakeToggle />);
    const button = screen.getByRole('button', { name: 'Toggle theme' });
    await userEvent.click(button);
    expect(screen.getByRole('tooltip')).toHaveTextContent(NIGHT_ONLY);
    expect(NIGHT_ONLY).toBe('Night mode only.');
    expect(button).toHaveClass('wiggle');
    await userEvent.click(button);
    expect(themeSnapshot()).toEqual(before);
  });

  it('hides the tooltip again after a moment', () => {
    vi.useFakeTimers();
    render(<FakeToggle />);
    fireEvent.click(screen.getByRole('button', { name: 'Toggle theme' }));
    expect(screen.getByRole('tooltip')).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(screen.queryByRole('tooltip')).toBeNull();
    vi.useRealTimers();
  });
});
