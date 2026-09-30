import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { Fold } from './Fold';

const details = () => screen.getByText('all numbers').closest('details') as HTMLDetailsElement;

describe('Fold', () => {
  it('starts closed, opens on click, and remembers it', async () => {
    const { unmount } = render(<Fold id="t" label="all numbers"><p>inside</p></Fold>);
    expect(details().open).toBe(false);
    expect(screen.getByText('inside')).toBeInTheDocument(); // closed content stays in the DOM
    await userEvent.click(screen.getByText('all numbers'));
    expect(details().open).toBe(true);
    expect(localStorage.getItem('fold:t')).toBe('1');
    unmount();
    render(<Fold id="t" label="all numbers"><p>inside</p></Fold>);
    expect(details().open).toBe(true);
    await userEvent.click(screen.getByText('all numbers'));
    expect(localStorage.getItem('fold:t')).toBeNull();
  });

  it('renders closed and still toggles when storage throws', async () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
    render(<Fold id="t" label="all numbers"><p>inside</p></Fold>);
    expect(details().open).toBe(false);
    await userEvent.click(screen.getByText('all numbers'));
    expect(details().open).toBe(true);
  });

  it('renders function children only while open (a lazy fold fetches nothing until opened)', async () => {
    render(<Fold id="lazy" label="earlier epochs">{(open) => (open ? <p>fetched</p> : null)}</Fold>);
    expect(screen.queryByText('fetched')).toBeNull();
    await userEvent.click(screen.getByText('earlier epochs'));
    expect(screen.getByText('fetched')).toBeInTheDocument();
  });
});
