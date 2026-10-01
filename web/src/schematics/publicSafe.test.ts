// Spec 7b §7.2: the schematics the public /math bundle reuses must not reach dashboard code through a
// value import (type-only imports are erased). vite.math.config.ts's guard proves it on the built bundle
// (Task 13); this walks the source, so `npm test` shows it without a build.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const ROOT = `${process.cwd()}/`;
/** 09-27 §4.4: what the math bundle must never hold. */
const DASHBOARD = /^src\/(pages|api|explain)\/|^src\/help\.ts$/;
/** The story (spec 2026-10-01 §7): on the public page through MathPage. */
const STORY = ['src/math/story/Story.tsx', 'src/math/story/Chapter.tsx', 'src/math/story/Words.tsx', 'src/math/story/copy.ts', 'src/math/story/presets.ts'];
/** The public schematics; Tasks 6–8 add theirs. */
const ENTRIES = [
  'src/schematics/LatencyLadder.tsx', 'src/schematics/DurationBars.tsx', 'src/schematics/PayoffGrid.tsx',
  'src/schematics/MirrorBooks.tsx', 'src/schematics/ClassifyTree.tsx', 'src/schematics/PriceStack.tsx',
  'src/schematics/DepthStairs.tsx', 'src/schematics/BasketLegs.tsx', 'src/schematics/UpdateBars.tsx',
  'src/math/sections/MirrorSection.tsx', 'src/math/sections/PayoffSection.tsx', 'src/math/sections/ClassifySection.tsx',
  'src/math/sections/DepthSection.tsx', 'src/math/sections/EpisodeSection.tsx', 'src/math/sections/LegMissSection.tsx',
  'src/math/useSnapshot.ts', 'src/math/LiveStrip.tsx', 'src/math/LiveLadder.tsx',
  'src/math/LiveBoundary.tsx', 'src/math/MathPage.tsx', 'src/math/main.tsx',
  ...STORY,
];
/** `import x from '…'`, `import { type A, b } from '…'`, `export … from '…'`, `import '…'`; never `import type` / `export type`. */
const IMPORT = /^\s*(?:import|export)\s+(?!type\b)[^'"]*?\s*from\s*'([^']+)'|^\s*import\s+'([^']+)'/gm;

function resolve(from: string, spec: string): string | null {
  if (!spec.startsWith('.')) return null; // a package (react, …)
  const parts: string[] = [];
  for (const p of `${from.slice(0, from.lastIndexOf('/') + 1)}${spec}`.split('/')) {
    if (p === '..') parts.pop();
    else if (p !== '.' && p !== '') parts.push(p);
  }
  const path = parts.join('/');
  for (const c of [path, `${path}.ts`, `${path}.tsx`]) {
    if (/\.(tsx?|css)$/.test(c) && existsSync(ROOT + c)) return c;
  }
  return null;
}

/** Every file reached from `entries` by value imports, sorted. */
function reach(entries: string[]): string[] {
  const seen = new Set<string>();
  const todo = [...entries];
  while (todo.length > 0) {
    const f = todo.pop() as string;
    if (seen.has(f)) continue;
    seen.add(f);
    if (!/\.tsx?$/.test(f)) continue;
    const src: string = readFileSync(ROOT + f, 'utf8');
    for (const m of src.matchAll(IMPORT)) {
      const r = resolve(f, (m[1] ?? m[2]).split('?')[0]);
      if (r !== null) todo.push(r);
    }
  }
  return [...seen].sort();
}

/** A JSX `style` prop becomes a style attribute, which the math CSP (style-src without 'unsafe-inline') blocks. */
const STYLE_PROP = /\bstyle=\{/;

describe('public-safe schematics (spec 7b §7.2)', () => {
  it('the reused schematics reach no dashboard module by a value import (review focus 5)', () => {
    expect(reach(ENTRIES).filter((f) => DASHBOARD.test(f))).toEqual([]);
    expect(reach(['src/math/main.tsx']).filter((f) => DASHBOARD.test(f))).toEqual([]);
  });

  it('the walker follows value imports, so the check can fail', () => {
    expect(reach(['src/components/EmptyState.tsx'])).toContain('src/api/client.ts');
    expect(reach(['src/schematics/LockedBlocks.tsx'])).toContain('src/explain/models.ts');
  });

  it('no file reachable from the math entry sets a style attribute (the math CSP)', () => {
    const files = reach(['src/math/main.tsx']).filter((f) => /\.tsx?$/.test(f));
    expect(files).toContain('src/schematics/LatencyLadder.tsx'); // the walk really leaves src/math
    for (const f of [...STORY, 'src/schematics/PriceStack.tsx']) expect(files, f).toContain(f); // and reaches the story from the entry, through MathPage
    expect(files.filter((f) => STYLE_PROP.test(readFileSync(ROOT + f, 'utf8')))).toEqual([]);
  });

  it('the style matcher catches a style prop, and only that', () => {
    expect(STYLE_PROP.test('<div style={{ width: 1 }} />')).toBe(true);
    expect(STYLE_PROP.test('<div style={s} />')).toBe(true);
    expect(STYLE_PROP.test('<Bar fontStyle={x} className="style" />')).toBe(false);
  });
});
