// @vitest-environment node
import { build } from 'vite';
import { describe, expect, it } from 'vitest';
import { DASHBOARD } from '../../vite.math.config';
import { HAVE_PKG, WASM_REQUIRED, skipReason } from '../wasm/testEngine';

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('math build'));

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('the math build (spec 7b §7.2, §8)', () => {
  it('bundles the reused schematics and nothing from the dashboard (review focus 5)', async () => {
    const out: unknown = await build({ configFile: 'vite.math.config.ts', mode: 'production', logLevel: 'silent', build: { write: false } });
    const outputs = (Array.isArray(out) ? out : [out]) as { output: { type: string; moduleIds?: string[] }[] }[];
    const ids = outputs
      .flatMap((o) => o.output.flatMap((c) => (c.type === 'chunk' ? (c.moduleIds ?? []) : [])))
      .map((id) => id.replace(/\\/g, '/').split('?')[0]);
    for (const f of ['src/schematics/LatencyLadder.tsx', 'src/schematics/PayoffGrid.tsx', 'src/schematics/DurationBars.tsx', 'src/components/SweetLadderGrid.tsx', 'src/math/MathPage.tsx', 'src/wasm/engine.ts']) {
      expect(ids.some((id) => id.endsWith(f)), f).toBe(true);
    }
    expect(ids.filter((id) => DASHBOARD.test(id))).toEqual([]);
  }, 120_000);
});
