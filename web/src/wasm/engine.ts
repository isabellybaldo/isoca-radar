// The only module that imports the generated package (09-27 §2); src/math/main.tsx calls loadEngine once.
import init, { basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json } from './pkg/arb_wasm.js';
import wasmUrl from './pkg/arb_wasm_bg.wasm?url';
import type { Engine } from './types';

/** Streams and instantiates the .wasm (CSP: 'wasm-unsafe-eval', connect-src 'self'); rejects if it fails. */
export async function loadEngine(): Promise<Engine> {
  await init({ module_or_path: wasmUrl });
  return { mirror_json, payoff_json, classify_json, depth_walk_json, episode_json, basket_json };
}
