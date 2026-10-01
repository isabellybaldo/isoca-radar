import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Chapter } from './Chapter';
import { CHAPTERS, WORDS } from './copy';
import { Words } from './Words';

const C2 = CHAPTERS[1];

describe('Chapter (spec §3.1)', () => {
  it('is a section named by its question: h2, lead, switch, the child, takeaway, then after', () => {
    const { container } = render(
      <Chapter copy={C2} options={C2.options} value={0} onPick={() => {}} take="the takeaway" after={<aside className="the-after" />}>
        <figure aria-label="the drawing" />
      </Chapter>,
    );
    const sec = container.querySelector('section#s2') as HTMLElement;
    expect(sec).toHaveClass('m-ch');
    expect(within(sec).getByRole('heading', { level: 2 })).toHaveTextContent(`2 ${C2.question}`);
    expect(sec).toHaveAccessibleName(`2 ${C2.question}`);
    const kids = [...sec.children].map((k) => `${k.tagName.toLowerCase()}.${k.className}`);
    expect(kids).toEqual(['h2.', 'p.m-ch-lead', 'div.seg', 'figure.', 'p.m-ch-take', 'aside.the-after']);
    expect(within(sec).getByText('the takeaway')).toHaveClass('m-ch-take');
  });

  it('after still renders when there is no takeaway', () => {
    const { container } = render(<Chapter copy={C2} take={null} after={<aside className="the-after" />} />);
    const sec = container.querySelector('section#s2') as HTMLElement;
    expect(sec.querySelector('.m-ch-take')).toBeNull();
    expect(sec.lastElementChild).toHaveClass('the-after');
  });

  it('the switch is a named group of two buttons; the pressed one says so; picking the other reports 1', () => {
    const onPick = vi.fn();
    render(<Chapter copy={C2} options={C2.options} value={0} onPick={onPick} take={null} />);
    const group = screen.getByRole('group', { name: C2.switchLabel });
    const [a, b] = within(group).getAllByRole('button');
    expect([a.getAttribute('aria-pressed'), b.getAttribute('aria-pressed')]).toEqual(['true', 'false']);
    fireEvent.click(b);
    expect(onPick).toHaveBeenCalledWith(1);
  });

  it('with no options it is prose only: no group, no takeaway paragraph', () => {
    const { container } = render(<Chapter copy={C2} />);
    expect(screen.queryByRole('group')).toBeNull();
    expect(container.querySelector('.m-ch-take')).toBeNull();
    expect(screen.getByText(C2.lead)).toBeInTheDocument();
  });
});

describe('Words (spec §6)', () => {
  it('is one definition list with a term and a definition per word', () => {
    const { container } = render(<Words />);
    const dl = container.querySelector('dl.m-words') as HTMLElement;
    expect(dl.querySelectorAll('dt')).toHaveLength(WORDS.length);
    expect(dl.querySelectorAll('dd')).toHaveLength(WORDS.length);
    expect([...dl.querySelectorAll('dt')].map((d) => d.textContent)).toEqual(WORDS.map(([t]) => t));
  });
});
