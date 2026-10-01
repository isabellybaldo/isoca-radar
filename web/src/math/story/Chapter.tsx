import type { ReactNode } from 'react';
import { Seg } from '../sections/Section';
import type { ChapterCopy } from './copy';

/** Spec 2026-10-01 §3.1: a question, its lead, one two-state switch, the drawing, the takeaway for the state
 *  shown, then `after` (chapter 3's strip, chapter 7's extras line). Without options (the engine is not there,
 *  or the chapter is live data) it is prose and children only. */
export function Chapter({ copy, options, value = 0, onPick, take, children, after }: {
  copy: ChapterCopy;
  options?: readonly [string, string] | null;
  value?: 0 | 1;
  onPick?: (v: 0 | 1) => void;
  take?: string | null;
  children?: ReactNode;
  after?: ReactNode;
}) {
  const hid = `m-${copy.id}`;
  return (
    <section id={copy.id} className="m-ch" aria-labelledby={hid}>
      <h2 id={hid}>
        <span className="m-ch-n">{copy.n}</span> {copy.question}
      </h2>
      <p className="m-ch-lead">{copy.lead}</p>
      {options != null && onPick !== undefined && (
        <Seg<0 | 1> label={copy.switchLabel} options={[[0, options[0]], [1, options[1]]]} value={value} onPick={onPick} />
      )}
      {children}
      {take != null && <p className="m-ch-take">{take}</p>}
      {after}
    </section>
  );
}
