import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/** A page's own title row (spec §4.3, §4.4): the window switch and page-level status live here. */
export function PageHead({ title, help, children }: { title: string; help: HelpId; children?: ReactNode }) {
  return (
    <div className="pagehead">
      <h1>{title}</h1>
      <div className="pagehead-actions">{children}</div>
      <Help id={help} />
    </div>
  );
}
