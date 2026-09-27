import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { HELP } from '../help';
import { Help } from './Help';

describe('Help (?)', () => {
  it('toggles on click, is announced to screen readers, and closes on Escape', async () => {
    render(<Help id="open" />);
    const btn = screen.getByRole('button', { name: `Help: ${HELP.open.title}` });
    expect(btn).toHaveTextContent('(?)');
    expect(btn).toHaveAttribute('aria-expanded', 'false');
    const pop = document.getElementById(btn.getAttribute('aria-controls') ?? 'missing');
    expect(pop).not.toBeNull();
    expect(pop).not.toBeVisible();
    await userEvent.click(btn);
    expect(btn).toHaveAttribute('aria-expanded', 'true');
    expect(pop).toBeVisible();
    expect(pop).toHaveAttribute('aria-live', 'polite');
    expect(pop).toHaveTextContent(HELP.open.text);
    await userEvent.keyboard('{Escape}');
    expect(btn).toHaveAttribute('aria-expanded', 'false');
    expect(btn).toHaveFocus();
  });

  it('opens and closes from the keyboard with Enter and Space', async () => {
    render(<Help id="g2" />);
    const btn = screen.getByRole('button', { name: /^Help: / });
    btn.focus();
    await userEvent.keyboard('{Enter}');
    expect(btn).toHaveAttribute('aria-expanded', 'true');
    await userEvent.keyboard(' ');
    expect(btn).toHaveAttribute('aria-expanded', 'false');
  });

  it('has plain-language copy for every id, including the spec examples', () => {
    for (const h of Object.values(HELP)) {
      expect(h.title.endsWith('?')).toBe(true);
      expect(h.text.length).toBeGreaterThan(60);
    }
    expect(HELP.open.title).toBe('What is an episode?');
    expect(HELP.edge.title).toBe('Why is the NO side cashable and the YES side locked?');
    expect(HELP.g2.title).toBe('What is Kaplan–Meier?');
    expect(HELP.status.title).toBe('What does warming mean?');
  });

  it('closes on a click or tap outside it; a click inside the popover keeps it open', async () => {
    render(
      <div>
        <p>elsewhere on the page</p>
        <Help id="g3" />
      </div>,
    );
    const btn = screen.getByRole('button', { name: /^Help: / });
    const pop = document.getElementById(btn.getAttribute('aria-controls') ?? 'missing');
    await userEvent.click(btn);
    expect(btn).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(pop as HTMLElement);
    expect(btn).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(screen.getByText('elsewhere on the page'));
    expect(btn).toHaveAttribute('aria-expanded', 'false');
    expect(pop).not.toBeVisible();
    await userEvent.click(btn);
    expect(btn).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(btn);
    expect(btn).toHaveAttribute('aria-expanded', 'false');
  });
});
