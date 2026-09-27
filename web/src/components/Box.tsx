import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/** A page section. `help` is required: every section of every page has a (?) (spec §8.3). */
export function Box({ title, help, className, children }: { title?: ReactNode; help: HelpId; className?: string; children: ReactNode }) {
  return (
    <section className={className ? `box ${className}` : 'box'}>
      <div className="box-head">
        {title !== undefined && <h2>{title}</h2>}
        <Help id={help} />
      </div>
      {children}
    </section>
  );
}
