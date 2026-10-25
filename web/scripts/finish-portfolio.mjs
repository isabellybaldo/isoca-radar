// Frozen-site plan, task 4: the last step of build:portfolio. Adds the two frozen files, runs the math bundle's
// own check on dist-portfolio/, then checks the folder against plan 3's contract: the files nginx serves exist,
// the site pages load only their own assets, the inline favicon and Google Fonts, no site script names /api/ or
// opens an EventSource, and no page carries an inline script or style (one CSP serves it all).
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const WEB = fileURLToPath(new URL('../', import.meta.url));
const DIST = `${WEB}dist-portfolio/`;
const FONT_HOSTS = ['https://fonts.googleapis.com', 'https://fonts.gstatic.com'];
const FROZEN = ['public.json', 'cards-all.json'];
const PAGES = ['index.html', 'sweet.html'];

function fail(msg) {
  console.error(`finish-portfolio: ${msg}`);
  process.exit(1);
}

mkdirSync(`${DIST}frozen`, { recursive: true });
for (const f of FROZEN) copyFileSync(`${WEB}frozen/${f}`, `${DIST}frozen/${f}`);

try {
  execFileSync(process.execPath, ['scripts/check-math-bundle.mjs'], {
    cwd: WEB,
    env: { ...process.env, MATH_DIST: 'dist-portfolio/' },
    stdio: 'inherit',
  });
} catch {
  fail('the math bundle check failed');
}

const missing = [...PAGES, 'math.html', ...FROZEN.map((f) => `frozen/${f}`)].filter((f) => !existsSync(DIST + f));
if (missing.length > 0) fail(`missing ${missing.join(', ')}`);

const bad = [];
for (const page of PAGES) {
  const html = readFileSync(DIST + page, 'utf8');
  for (const m of html.matchAll(/\b(?:src|href)\s*=\s*(?:"([^"]*)"|'([^']*)')/gi)) {
    const u = m[1] ?? m[2];
    const ok = u.startsWith('/site-assets/') || u === 'data:,' || FONT_HOSTS.some((h) => u === h || u.startsWith(`${h}/`));
    if (!ok) bad.push(`${page} points to ${u}`);
  }
  for (const m of html.matchAll(/<script\b([^>]*)>/gi)) if (!/\bsrc\s*=\s*["']/i.test(m[1])) bad.push(`inline <script> in ${page}`);
  if (/<style\b/i.test(html)) bad.push(`inline <style> in ${page}`);
  if (/\sstyle\s*=/i.test(html)) bad.push(`style= attribute in ${page}`);
  if (/<[^>]*\son[a-z]+\s*=/i.test(html)) bad.push(`on<event>= attribute in ${page}`);
}

const scripts = existsSync(`${DIST}site-assets`) ? readdirSync(`${DIST}site-assets`).filter((n) => n.endsWith('.js')) : [];
if (scripts.length === 0) bad.push('no scripts under site-assets/');
for (const name of scripts) {
  const text = readFileSync(`${DIST}site-assets/${name}`, 'utf8');
  if (text.includes('/api/')) bad.push(`site-assets/${name} names /api/`);
  if (text.includes('EventSource')) bad.push(`site-assets/${name} names EventSource`);
}
if (bad.length > 0) fail(bad.join('; '));
console.log('portfolio ok');
