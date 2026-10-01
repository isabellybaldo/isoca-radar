// Spec 7b §7, §5.3, §9 (09-27 §4.2–§5.3): the checked-in build and deploy files carry what the public page
// needs. Plain text checks, read with node fs (as theme.test.ts reads theme.css); CRLF normalized.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { execFileSync } from 'node:child_process';
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
// @ts-ignore: node:os, as above
import { tmpdir } from 'node:os';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const repo = (p: string): string => (readFileSync(`${process.cwd()}/../${p}`, 'utf8') as string).replace(/\r\n/g, '\n');

/** One top-level job of a workflow: from `  <name>:` up to the next `  <job>:` line. */
function job(yml: string, name: string): string {
  const start = yml.indexOf(`\n  ${name}:\n`);
  expect(start, `no job ${name}`).toBeGreaterThanOrEqual(0);
  const rest = yml.slice(start + 1);
  const next = rest.slice(1).search(/\n {2}[a-z][\w-]*:\n/);
  return next < 0 ? rest : rest.slice(0, next + 2);
}

/** The `run:` lines of a job (comments may mention anything). */
const runLines = (j: string): string[] =>
  j.split('\n').filter((l) => /^\s*(- )?run:/.test(l));

describe('ci (09-27 §4.3)', () => {
  it('the web job builds the engine, runs the WASM side of parity and the full build, with WASM_REQUIRED', () => {
    const yml = repo('.github/workflows/ci.yml');
    const web = job(yml, 'web');
    const runs = runLines(web).join('\n');
    expect(web).toContain('WASM_REQUIRED: "1"');
    expect(web).toContain('rustup target add wasm32-unknown-unknown');
    expect(web).toContain('cargo install wasm-bindgen-cli --locked --version');
    expect(web).toContain('sh scripts/wasm-bindgen-version.sh'); // one source for the CLI version
    expect(web).not.toContain('awk');
    expect(runs).not.toContain('cargo test -p arb-wasm'); // 7b-A's wasm job runs the native side
    expect(yml).toContain('\n  wasm:\n');
    expect(job(yml, 'wasm')).toContain('cargo test -p arb-wasm');
    // the engine ships as the release package, never the debug one
    expect(runs).not.toContain('PROFILE=dev');
    expect(web).toContain('npx tsc --noEmit -p tsconfig.math.json');
    const steps = [
      'sh scripts/build-wasm.sh',
      'npm ci',
      'npx tsc --noEmit\n',
      'npx tsc --noEmit -p tsconfig.math.json',
      'npm test',
      'npm run build',
    ].map((s) => web.indexOf(s));
    expect(steps.every((i) => i >= 0)).toBe(true);
    expect([...steps].sort((a, b) => a - b)).toEqual(steps);
  });
});

describe('web image (09-27 §4.2)', () => {
  it('builds the engine on the rust image rust.Dockerfile pins, with cargo-chef, and hands pkg to the node stage', () => {
    const web = repo('docker/web.Dockerfile');
    const pin = /FROM (rust:\S+@sha256:[0-9a-f]{64})/.exec(repo('docker/rust.Dockerfile'))?.[1];
    expect(pin).toBeDefined();
    expect(web).toContain(`FROM ${pin} AS wasm-chef`);
    expect(web).toContain('cargo chef cook --release --target wasm32-unknown-unknown --recipe-path recipe.json --package arb-wasm');
    expect(web).toContain('rustup target add wasm32-unknown-unknown');
    expect(web).toContain('RUN sh scripts/build-wasm.sh');
    expect(web).toContain('COPY --from=wasm /src/web/src/wasm/pkg ./src/wasm/pkg');
    expect(web.indexOf('COPY --from=wasm ')).toBeLessThan(web.indexOf('RUN npm run build'));
    expect(repo('.dockerignore')).toMatch(/^web\/src\/wasm\/pkg$/m);
    for (const needed of ['Cargo.toml', 'Cargo.lock', 'crates', 'bins', 'rust-toolchain.toml', 'scripts']) {
      expect(repo('.dockerignore'), needed).not.toMatch(new RegExp(`^${needed}$`, 'm'));
    }
  });

  it('takes the wasm-bindgen CLI version from scripts/wasm-bindgen-version.sh, never a literal or awk', () => {
    const web = repo('docker/web.Dockerfile');
    expect(web).toContain('cargo install --locked --version "$(sh scripts/wasm-bindgen-version.sh)" wasm-bindgen-cli');
    expect(web).not.toContain('awk');
    expect(web).not.toMatch(/wasm-bindgen-cli[^\n]*\d+\.\d+\.\d+/);
  });

  it('ships math.html and math-assets from dist, exposes both server ports, and leaves the mount point to Docker', () => {
    const web = repo('docker/web.Dockerfile');
    expect(web).toContain('COPY --from=build /web/dist /usr/share/nginx/html');
    expect(web).not.toContain('mkdir');
    expect(web).not.toContain('USER root');
    expect(web).toMatch(/USER 101\n(?:#[^\n]*\n)?EXPOSE 8080 8081\n?$/);
  });

  it('the wasm-bindgen version script reads a CRLF Cargo.lock (a Windows checkout is the Docker build context)', () => {
    const dir = mkdtempSync(`${tmpdir()}/wbg-`) as string;
    mkdirSync(`${dir}/scripts`);
    writeFileSync(`${dir}/scripts/wasm-bindgen-version.sh`, repo('scripts/wasm-bindgen-version.sh'));
    const lock = '[[package]]\nname = "wasm-bindgen"\nversion = "9.8.7"\nsource = "registry"\n\n[[package]]\nname = "wasm-bindgen-macro"\nversion = "9.8.7"\n';
    for (const eol of ['\n', '\r\n']) {
      writeFileSync(`${dir}/Cargo.lock`, lock.replace(/\n/g, eol));
      const out = execFileSync('sh', [`${dir}/scripts/wasm-bindgen-version.sh`], { encoding: 'utf8' }) as string;
      expect(out, JSON.stringify(eol)).toBe('9.8.7\n');
    }
    // the CR is written as the visible escape, never a raw byte an editor could strip
    const src = repo('scripts/wasm-bindgen-version.sh');
    expect(src).toContain('sub(/' + String.fromCharCode(92) + 'r$/');
    expect(src.includes(String.fromCharCode(13))).toBe(false);
  });
});

describe('nginx (09-27 §5.2, 7b §5.3)', () => {
  /** The body of each `server { … }` block, by listen port (the blocks have no nested braces beyond locations). */
  const servers = (): Record<string, string> => {
    const n = repo('docker/web/nginx.conf');
    const out: Record<string, string> = {};
    for (const m of n.matchAll(/^server \{\n([\s\S]*?)\n\}$/gm)) {
      const port = /listen (\d+);/.exec(m[1])?.[1] ?? '';
      expect(out[port], `two servers on ${port}`).toBeUndefined();
      out[port] = m[1];
    }
    return out;
  };
  const locations = (body: string): string[] => [...body.matchAll(/^ {4}location (.*) \{$/gm)].map((m) => m[1]);

  it('the public server (8081) has exactly /math, /math-assets/, the exact public.json and a 404 catch-all', () => {
    const s = servers();
    expect(Object.keys(s).sort()).toEqual(['8080', '8081']);
    const p = s['8081'];
    expect(locations(p)).toEqual(['= /math', '/math-assets/', '= /math-data/public.json', '/']);
    expect(p).toMatch(/location = \/math \{[^}]*root \/usr\/share\/nginx\/html;[^}]*add_header Cache-Control "no-cache";[^}]*try_files \/math\.html =404;/);
    expect(p).toMatch(/location \/math-assets\/ \{[^}]*root \/usr\/share\/nginx\/html;[^}]*immutable[^}]*gzip_static on;[^}]*try_files \$uri =404;/);
    expect(p).toMatch(/location = \/math-data\/public\.json \{[^}]*root \/srv\/ct-public-root;[^}]*disable_symlinks on;[^}]*max-age=60[^}]*gzip on;[^}]*gzip_types application\/json;[^}]*try_files \$uri =404;/);
    expect(p).toMatch(/location \/ \{\n\s*return 404;\n\s*\}/);
    // no server-level root, no index, no other root than the two the locations need
    const outside = p.replace(/ {4}location [\s\S]*?\n {4}\}/g, '').replace(/^\s*#.*$/gm, '');
    expect(outside).not.toMatch(/\broot\b|\bindex\b|try_files/);
    // server level, so they cover gzip_static (the assets) and gzip (the snapshot): without `gzip_proxied any`
    // a request with a Via header gets neither
    expect(outside).toMatch(/^ {4}gzip_vary on;$/m);
    expect(outside).toMatch(/^ {4}gzip_proxied any;$/m);
    expect(p.match(/gzip_proxied/g)).toHaveLength(1);
    expect(p.match(/^\s*root /gm)).toHaveLength(3);
    expect(repo('docker/web/nginx.conf').match(/root \/srv\/ct-public-root;/g)).toHaveLength(1);
  });

  it('the login-gated server (8080) has no math location and no volume root', () => {
    const main = servers()['8080'];
    expect(locations(main)).toEqual(['/assets/', '/']);
    expect(main).not.toMatch(/math|ct-public-root/);
  });

  it('sets no security header itself (Traefik does), so the per-location add_header loses none', () => {
    const n = repo('docker/web/nginx.conf');
    const headers = [...n.matchAll(/^\s*add_header (\S+)/gm)].map((m) => m[1]);
    expect(new Set(headers)).toEqual(new Set(['Cache-Control']));
  });
});

describe('compose.web.yml (7b §5.3)', () => {
  const web = (): string => {
    const y = repo('compose.web.yml');
    return y.slice(y.indexOf('\n  web:\n'), y.indexOf('\n  oauth2-proxy:\n'));
  };

  it('mounts the snapshot volume read-only into web and still read-write into the api', () => {
    const y = repo('compose.web.yml');
    expect(web()).toContain('- public-data:/srv/ct-public-root/math-data:ro,nocopy');
    expect(y).toContain('      - public-data:/var/lib/ct-public\n');
    expect(y).toMatch(/^volumes:\n(?: {2}\S.*\n)*? {2}public-data:/m);
    // the runbook's grep -c public-data: 3 before this task (api comment, api mount, declaration), 4 with the web mount
    expect(y.split('\n').filter((l) => l.includes('public-data'))).toHaveLength(4);
  });

  it('routes only the three public paths to web without login, with the per-IP limits and the math CSP', () => {
    const w = web();
    const lines = [
      'routers.ct-math.rule=Host(`${RADAR_DOMAIN}`) && (Path(`/math`) || PathPrefix(`/math-assets/`) || Path(`/math-data/public.json`))',
      'routers.ct-math.entrypoints=websecure',
      'routers.ct-math.tls.certresolver=letsencrypt',
      'routers.ct-math.service=ct-math',
      'services.ct-math.loadbalancer.server.port=8081',
      'routers.ct-math.priority=1000',
      // headers before the limits: a 429 written by Traefik's own limiters passes back through ct-sec and the CSP
      'routers.ct-math.middlewares=ct-sec@docker,ct-math-csp@docker,ct-math-rl@docker,ct-math-if@docker',
      'middlewares.ct-math-rl.ratelimit.average=5',
      'middlewares.ct-math-rl.ratelimit.burst=20',
      'middlewares.ct-math-rl.ratelimit.sourcecriterion.ipstrategy.depth=0',
      'middlewares.ct-math-if.inflightreq.amount=8',
      'middlewares.ct-math-if.inflightreq.sourcecriterion.ipstrategy.depth=0',
      "middlewares.ct-math-csp.headers.contentSecurityPolicy=default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ];
    for (const l of lines) expect(w, l).toContain(`traefik.http.${l}`);
    // the CSP label must END at the closing quote: an appended directive must fail
    expect(w).toContain(`      - "traefik.http.${lines[lines.length - 1]}"\n`);
    expect(w).not.toMatch(/ct-math\.middlewares=[^\n]*ct-auth/);
    expect(w).toContain('traefik.http.routers.ct-web.middlewares=ct-rl@docker,ct-sec@docker,ct-csp@docker,ct-auth@docker');
    expect(w).toContain('traefik.http.routers.ct-web.service=ct-web');
    expect(w.match(/ct-math/g)).toHaveLength(17);
  });

  it('keeps login on every other router and the dashboard CSP unchanged', () => {
    const y = repo('compose.web.yml');
    const mw = (r: string): string => new RegExp(`routers\\.${r}\\.middlewares=(\\S+)`).exec(y)?.[1] ?? '';
    expect(mw('ct-api')).toBe('ct-rl@docker,ct-sec@docker,ct-csp@docker,ct-auth@docker');
    expect(mw('ct-web')).toBe('ct-rl@docker,ct-sec@docker,ct-csp@docker,ct-auth@docker');
    expect(mw('ct-oauth')).toBe('ct-rl@docker,ct-sec@docker,ct-csp@docker');
    expect(y).toContain("contentSecurityPolicy=default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com;");
    expect(y).toContain(
      `traefik.http.middlewares.ct-csp.headers.contentSecurityPolicy=default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"`,
    );
    // only ct-math's CSP allows wasm
    expect(y.split('\n').filter((l) => l.includes('contentSecurityPolicy') && l.includes('wasm-unsafe-eval'))).toHaveLength(1);
    expect(y).toContain('traefik.http.middlewares.ct-sec.headers.stsSeconds=31536000');
    // every router (by its rule label) is accounted for: only these carry no ct-auth, and none lacks a middlewares label
    const routers = [...y.matchAll(/routers\.([\w-]+)\.rule=/g)].map((m) => m[1]).sort();
    expect(routers).toEqual(['ct-api', 'ct-http', 'ct-math', 'ct-oauth', 'ct-web']);
    const noAuth = routers.filter((r) => !mw(r).includes('ct-auth'));
    expect(noAuth).toEqual(['ct-http', 'ct-math', 'ct-oauth']);
    expect(mw('ct-math')).not.toBe('');
  });

  it('names its service on every router of the web container (it now has two)', () => {
    const w = web();
    expect(w).toContain('traefik.http.routers.ct-web.service=ct-web');
    expect(w).toContain('traefik.http.routers.ct-math.service=ct-math');
    expect(w).toContain('traefik.http.services.ct-web.loadbalancer.server.port=8080');
    expect(w).toContain('traefik.http.services.ct-math.loadbalancer.server.port=8081');
    expect([...w.matchAll(/routers\.([\w-]+)\.rule=/g)]).toHaveLength(2);
    expect([...w.matchAll(/routers\.([\w-]+)\.service=/g)]).toHaveLength(2);
  });
});
