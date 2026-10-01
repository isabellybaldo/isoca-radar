import type { ReactNode } from 'react';
import { type EngineError, isError } from '../../wasm/types';
import '../math.css';

/** 09-27 §6.1: title, the answer and its drawing, controls with a reset, one deadpan line, and a caption
 *  naming the function that computed it. `id` is the link target (#a to #f). */
export function Section({ id, title, chapter, quip, caption, controls, onReset, children }: {
  id: string;
  title: string;
  chapter: string;
  quip: string | null;
  caption: string;
  controls: ReactNode;
  onReset: () => void;
  children: ReactNode;
}) {
  return (
    <section id={id} className="box m-sec" aria-labelledby={`m-${id}`}>
      <h3 id={`m-${id}`}>{title}</h3>
      <p className="dim m-back">{chapter}</p>
      {children}
      <div className="m-controls">
        {controls}
        <button type="button" className="m-btn" onClick={onReset}>reset</button>
      </div>
      {quip !== null && <p className="quip">{quip}</p>}
      <p className="dim m-cap">{caption}</p>
    </section>
  );
}

/** The engine's model, or its one sentence in place of the drawing's numbers. The sentence is a polite status,
 *  not an alert: it is redrawn while a slider moves, and an alert would be read out again on every step. */
export function Out<T>({ out, children }: { out: T | EngineError; children: (m: T) => ReactNode }) {
  return isError(out) ? <p className="error" role="status">{out.error}</p> : <>{children(out)}</>;
}

/** `label` is the slider's full accessible name. Inside a group whose legend already says the first words
 *  ("leg A"), `text` is the shorter text shown beside it. */
export function Range({ label, text, min, max, step = 1, value, show, disabled, onChange }: {
  label: string;
  text?: string;
  min: number;
  max: number;
  step?: number;
  value: number;
  show?: string;
  disabled?: boolean;
  onChange: (v: number) => void;
}) {
  return (
    <label className="m-range">
      <span>{text ?? label}</span>
      <input type="range" aria-label={label} aria-valuetext={show ?? String(value)} disabled={disabled} min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="num">{show ?? String(value)}</span>
    </label>
  );
}

export function Seg<T extends string | number>({ label, options, value, onPick }: {
  label: string;
  options: [T, string][];
  value: T;
  onPick: (v: T) => void;
}) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={String(v)} type="button" className={v === value ? 'on' : undefined} aria-pressed={v === value} onClick={() => onPick(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

export function Check({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="m-check">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} /> {label}
    </label>
  );
}
