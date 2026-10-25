import type { ReactNode } from 'react';
import { FROZEN_LINE } from '../frozenMode';

/** Frozen-site plan, task 4: the frame of the portfolio's pages. Plain links, no router: the landing, /sweet and
 *  /math are three separate pages. Every page ends on the frozen line (Global Constraints). */
export function Shell({ children }: { children: ReactNode }) {
  return (
    <div className="app pf">
      <header className="top">
        <a className="logo pf-home" href="/">♥ isoca-radar</a>
        <nav className="nav" aria-label="main">
          <a href="/math">MATH</a>
          <a href="/sweet">SWEET SPOTS</a>
        </nav>
      </header>
      <main className="body pf-body">{children}</main>
      <footer className="pf-foot">
        <p>{FROZEN_LINE}</p>
      </footer>
    </div>
  );
}
