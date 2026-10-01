// The only module that imports the generated package (09-27 §2); src/math/main.tsx calls loadEngine once.
import init, { basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json } from './pkg/arb_wasm.js';
import wasmUrl from './pkg/arb_wasm_bg.wasm?url';
import type { Engine } from './types';

let loading: Promise<Engine> | null = null;

/** Streams and instantiates the .wasm once (CSP: 'wasm-unsafe-eval', connect-src 'self'); callers share one
 *  promise. A failed load clears it, so a later call retries. */
export function loadEngine(): Promise<Engine> {
  loading ??= init({ module_or_path: wasmUrl }).then(
    (): Engine => ({ mirror_json, payoff_json, classify_json, depth_walk_json, episode_json, basket_json }),
  );
  loading.catch(() => {
    loading = null;
  });
  return loading;
}
