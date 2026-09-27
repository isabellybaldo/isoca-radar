import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/** Spec §3.2: a former inner box becomes a flat sub-heading with a rule above it. */
export function SubHead({ children, help, right }: { children: ReactNode; help?: HelpId; right?: ReactNode }) {
  return (
    <div className="subhead-row">
      <h3 className="subhead">{children}</h3>
      {right}
      {help && <Help id={help} />}
    </div>
  );
}
