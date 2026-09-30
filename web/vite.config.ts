/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // No inline scripts anywhere (spec §3 CSP): no modulepreload polyfill.
  build: { modulePreload: { polyfill: false }, sourcemap: false },
  // fs.allow: web itself, plus ../config only: ladder.test.ts reads ../config/policies.toml (?raw) to check the LADDER.
  server: { proxy: { '/api': 'http://127.0.0.1:8081' }, fs: { allow: ['.', '../config'] } },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    restoreMocks: true,
    unstubGlobals: true,
  },
});
