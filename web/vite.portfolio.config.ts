// Frozen-site plan, task 4: the portfolio's two site pages (the landing and Sweet spots) into dist-portfolio/
// (index.html, sweet.html, site-assets/). The math build follows into the same folder (package.json
// build:portfolio), then scripts/finish-portfolio.mjs adds the frozen files and checks the whole folder.
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  root: 'portfolio',
  // web/ (relative to root), where .env.frozen sets VITE_FROZEN for --mode frozen.
  envDir: '..',
  // Absolute: /sweet and /sweet.html load the same /site-assets/... URLs.
  base: '/',
  build: {
    outDir: '../dist-portfolio',
    emptyOutDir: true,
    assetsDir: 'site-assets',
    // One CSP for the whole site (plan 3): no inline scripts, so no modulepreload polyfill.
    modulePreload: { polyfill: false },
    sourcemap: false,
    rolldownOptions: { input: { index: 'portfolio/index.html', sweet: 'portfolio/sweet.html' } },
  },
});
