// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { noDashboardCode } from '../../vite.math.config';

type Hook = (this: { error: (m: string) => never }, o: unknown, b: Record<string, unknown>, w: boolean) => void;
const run = (ids: string[]) => {
  const hook = noDashboardCode().generateBundle as unknown as Hook;
  const error = (m: string): never => {
    throw new Error(m);
  };
  hook.call({ error }, {}, { 'a.js': { type: 'chunk', moduleIds: ids }, 'a.css': { type: 'asset' } }, false);
};

describe('noDashboardCode (09-27 §4.4)', () => {
  it('accepts math, wasm, schematics, lib and components, with either path style', () => {
    expect(() =>
      run(['C:\\x\\web\\src\\math\\MathPage.tsx', '/x/web/src/schematics/LatencyLadder.tsx', '/x/web/src/wasm/pkg/arb_wasm_bg.wasm?url', '/x/web/src/lib/money.ts', '/x/web/src/components/Fold.tsx']),
    ).not.toThrow();
  });

  it('rejects pages, api, explain and help.ts (review focus 5)', () => {
    for (const bad of ['/x/web/src/pages/Radar.tsx', 'C:\\x\\web\\src\\api\\client.ts', '/x/web/src/explain/models.ts', '/x/web/src/help.ts']) {
      expect(() => run([bad]), bad).toThrow(/dashboard module/);
    }
  });
});
