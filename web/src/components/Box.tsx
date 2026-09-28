import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/**
 * A page section. `help` is required: every section of every page has a (?) (spec §8.3).
 * `hero` marks the page's one drawing section (the only one with the offset shadow, redesign §3.2);
 * `actions` sits in the title row (window switch, status tag, scenario chips).
 */
export function Box({ title, help, className, hero = false, actions, children }: {
  title?: ReactNode;
  help: HelpId;
  className?: string;
  hero?: boolean;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const cls = ['box', hero ? 'hero' : null, className ?? null].filter(Boolean).join(' ');
  return (
    <section className={cls}>
      <div className="box-head">
        {title !== undefined && <h2>{title}</h2>}
        {actions !== undefined && <div className="box-actions">{actions}</div>}
        <Help id={help} />
      </div>
      {children}
    </section>
  );
}
