// 09-27 §4.4 + §5.2, 7b §11: the public page's assets stay at or under 350 KB gzip (what users receive),
// no "/api/" string ships in any math file but the wasm, no build-machine path in the wasm, the built files satisfy the math CSP, and every asset
// gets a deterministic .gz beside it for nginx gzip_static. MATH_DIST overrides the dist directory
// (checkBundle.test.ts).
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const raw = process.env.MATH_DIST ?? fileURLToPath(new URL('../dist/', import.meta.url));
const DIST = /[\\/]$/.test(raw) ? raw : `${raw}/`;
const ASSETS = `${DIST}math-assets/`;
const LIMIT = 350_000;
// The only origins the page may name: its own files, the inline favicon and the two Google Fonts hosts.
const FONT_HOSTS = ['https://fonts.googleapis.com', 'https://fonts.gstatic.com'];

// Inert strings React ships and never fetches: XML namespace identifiers and the error-decoder prefix.
const PATHS = ['\\Users\\', '/Users/', '/home/', 'C:\\'];
const INERT = new Set([
  'http://www.w3.org/2000/svg',
  'http://www.w3.org/1998/Math/MathML',
  'http://www.w3.org/1999/xlink',
  'http://www.w3.org/XML/1998/namespace',
  'https://react.dev/errors/',
]);

function fail(msg) {
  console.error(`check-math-bundle: ${msg}`);
  process.exit(1);
}

// gzip header mtime is 0 in node's zlib output, level is fixed: the same bytes give the same .gz.
let total = 0;
const bad = [];
const csp = [];
const names = readdirSync(ASSETS)
  .filter((n) => !n.endsWith('.gz'))
  .sort();
const texts = [];
for (const name of names) {
  const path = ASSETS + name;
  if (!statSync(path).isFile()) {
    csp.push(`math-assets/${name} is a directory (nothing may escape the size count and the scan)`);
    continue;
  }
  const bytes = readFileSync(path);
  const gz = gzipSync(bytes, { level: 9 });
  writeFileSync(`${path}.gz`, gz);
  total += gz.length;
  if (name.endsWith('.wasm')) {
    // The wasm is exempt from the "/api/" scan: its panic-message paths (crates/arb-wasm/src/api/...) hold it.
    // It must not carry the build machine's paths (build-wasm.sh remaps them to /cargo and /src).
    const ascii = bytes.toString('latin1');
    for (const p of PATHS) if (ascii.includes(p)) csp.push(`${name} embeds a build-machine path (${p})`);
  } else {
    const text = bytes.toString('utf8');
    texts.push([name, text]);
    if (text.includes('/api/')) bad.push(name);
  }
  console.log(`${name}: ${gz.length} B gzip (${bytes.length} B raw)`);
}
const html = readFileSync(`${DIST}math.html`, 'utf8');
if (html.includes('/api/')) bad.push('math.html');
console.log(`math-assets total: ${total} B gzip (limit ${LIMIT})`);
if (total === 0) fail('no math assets found');
if (!html.includes('/math-assets/')) fail('math.html does not load from /math-assets/');
if (bad.length > 0) fail(`"/api/" found in ${bad.join(', ')}`);

// The math CSP, on the built files.
for (const m of html.matchAll(/<script\b([^>]*)>/gi)) if (!/\bsrc\s*=\s*["']/i.test(m[1])) csp.push('inline <script> in math.html');
if (/<style\b/i.test(html)) csp.push('inline <style> in math.html');
if (/\sstyle\s*=/i.test(html)) csp.push('style= attribute in math.html');
if (/<[^>]*\son[a-z]+\s*=/i.test(html)) csp.push('on<event>= attribute in math.html');
if (/<meta\b[^>]*http-equiv/i.test(html)) csp.push('<meta http-equiv> in math.html');
for (const m of html.matchAll(/\b(?:src|href)\s*=\s*(?:"([^"]*)"|'([^']*)')/gi)) {
  const u = m[1] ?? m[2];
  const ok = u.startsWith('/math-assets/') || u === 'data:,' || FONT_HOSTS.some((h) => u === h || u.startsWith(`${h}/`));
  if (!ok) csp.push(`math.html points to ${u}`);
}
for (const [name, text] of texts) {
  for (const m of text.matchAll(/math-assets\/([\w.-]+\.wasm)/g)) if (!names.includes(m[1])) csp.push(`${name} names ${m[1]}, which is not a file under math-assets/`);
  if (/data:application\/wasm/.test(text)) csp.push(`${name} inlines the wasm as a data: URL`);
  for (const m of text.matchAll(/https?:\/\/[^\s"'`)\\]+/g)) if (!INERT.has(m[0])) csp.push(`${name} names the absolute URL ${m[0]}`);
}
if (csp.length > 0) fail(`math CSP: ${[...new Set(csp)].join('; ')}`);
if (total > LIMIT) fail('over the 350 KB gzip budget');
