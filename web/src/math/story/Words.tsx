import { WORDS } from './copy';

/** Spec 2026-10-01 §6: the story's terms, once, at the top of the playground. */
export function Words() {
  return (
    <dl className="m-words">
      {WORDS.map(([term, def]) => (
        <div key={term} className="m-word">
          <dt>{term}</dt>
          <dd>{def}</dd>
        </div>
      ))}
    </dl>
  );
}
