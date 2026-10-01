import type { ReactNode } from 'react';
import { type EngineError, isError } from '../../wasm/types';
import '../math.css';

/** 09-27 §6.1: title, the answer and its drawing, controls with a reset, one deadpan line, and a caption
 *  naming the function that computed it. `after` is f's live part (7b §6.2). */
export function Section({ id, title, quip, caption, controls, onReset, children, after }: {
  id: string;
  title: string;
  quip: string;
  caption: string;
  controls: ReactNode;
  onReset: () => void;
  children: ReactNode;
  after?: ReactNode;
}) {
  return (
    <section className="box m-sec" aria-labelledby={`m-${id}`}>
      <h2 id={`m-${id}`}>{title}</h2>
      {children}
      <div className="m-controls">
        {controls}
        <button type="button" className="m-btn" onClick={onReset}>reset</button>
      </div>
      <p className="quip">{quip}</p>
      <p className="dim m-cap">{caption}</p>
      {after}
    </section>
  );
}

/** The engine's model, or its one sentence in place of the drawing's numbers. */
export function Out<T>({ out, children }: { out: T | EngineError; children: (m: T) => ReactNode }) {
  return isError(out) ? <p className="error" role="alert">{out.error}</p> : <>{children(out)}</>;
}

export function Range({ label, min, max, step = 1, value, show, disabled, onChange }: {
  label: string;
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
      <span>{label}</span>
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
