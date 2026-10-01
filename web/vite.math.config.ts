// 09-27 §5.1: the public /math page, a second build into dist/ (math.html + math-assets/) after the
// dashboard's; no chunk is shared with it. §4.4: the build fails if a dashboard module gets in.
import react from '@vitejs/plugin-react';
import { type Plugin, defineConfig } from 'vite';

/**
 * What the public bundle must never hold (type-only imports are erased and never show up here).
 * Limit: a numeric constant imported from a dashboard module can be inlined by the bundler and drop out of
 * moduleIds; publicSafe.test.ts's import walk covers that case. Its own copy of this regex uses repo-relative
 * paths (^src/...) over source files, not absolute ids over bundle modules, and importing this config from
 * there would pull in vite, so the two stay separate.
 */
export const DASHBOARD = /\/src\/(pages|api|explain)\/|\/src\/help\.ts$/;

export function noDashboardCode(): Plugin {
  return {
    name: 'no-dashboard-code',
    generateBundle(_options, bundle) {
      for (const out of Object.values(bundle)) {
        if (out.type !== 'chunk') continue;
        for (const id of out.moduleIds) {
          const p = id.replace(/\\/g, '/').split('?')[0];
          if (DASHBOARD.test(p)) this.error(`math bundle includes dashboard module ${p}`);
        }
      }
    },
  };
}

export default defineConfig({
  plugins: [react(), noDashboardCode()],
  // Absolute: asset URLs read /math-assets/... whether the page is served at /math or /math/.
  base: '/',
  build: {
    outDir: 'dist',
    emptyOutDir: false,
    assetsDir: 'math-assets',
    // The math CSP: no inline scripts (no modulepreload polyfill) and no data: URLs (the .wasm stays a file).
    modulePreload: { polyfill: false },
    assetsInlineLimit: 0,
    sourcemap: false,
    rolldownOptions: { input: 'math.html' },
  },
});
