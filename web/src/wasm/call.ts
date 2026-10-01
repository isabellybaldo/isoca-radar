import type { Engine, EngineError, EngineFn } from './types';

/** A trap (a Rust panic aborts the instance) or an unreadable answer: one sentence, never a blank page. */
export const ENGINE_BROKE = 'The math engine stopped on this input. Reload the page to start it again.';

/** JSON in, JSON out (09-27 §4.1). Never throws: the engine's own {"error"} passes through; anything else
 *  that is not a JSON object becomes ENGINE_BROKE. */
export function call<T>(engine: Engine, fn: EngineFn, input: unknown): T | EngineError {
  let out: string;
  try {
    out = engine[fn](JSON.stringify(input));
  } catch {
    return { error: ENGINE_BROKE };
  }
  try {
    const v: unknown = JSON.parse(out);
    if (typeof v !== 'object' || v === null || Array.isArray(v)) return { error: ENGINE_BROKE };
    return v as T | EngineError;
  } catch {
    return { error: ENGINE_BROKE };
  }
}
