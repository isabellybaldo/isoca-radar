// Spec 7b §7, §5.3, §9 (09-27 §4.2–§5.3): the checked-in build and deploy files carry what the public page
// needs. Plain text checks, read with node fs (as theme.test.ts reads theme.css); CRLF normalized.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
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

  it('ships math.html and math-assets from dist, and creates the read-only mount point for the snapshot volume', () => {
    const web = repo('docker/web.Dockerfile');
    expect(web).toContain('COPY --from=build /web/dist /usr/share/nginx/html');
    expect(web).toContain('mkdir -p /srv/ct-public-root/math-data');
    expect(web.trimEnd().split('\n').pop()).not.toBe('USER root');
    expect(web).toMatch(/USER 101\nEXPOSE 8080/);
  });
});

describe('nginx (09-27 §5.2, 7b §5.3)', () => {
  it('serves /math, /math-assets/ from .gz files, and from the volume only the exact public.json', () => {
    const n = repo('docker/web/nginx.conf');
    expect(n).toMatch(/location = \/math \{[^}]*add_header Cache-Control "no-cache";[^}]*try_files \/math\.html =404;/);
    expect(n).toMatch(/location \/math-assets\/ \{[^}]*immutable[^}]*gzip_static on;[^}]*try_files \$uri =404;/);
    expect(n).toMatch(/location = \/math-data\/public\.json \{[^}]*root \/srv\/ct-public-root;[^}]*max-age=60[^}]*gzip on;[^}]*gzip_types application\/json;[^}]*try_files \$uri =404;/);
    expect(n).not.toMatch(/location \/math-data/);
    expect(n.indexOf('location = /math-data/public.json')).toBeLessThan(n.indexOf('location / {'));
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
    expect(web()).toContain('- public-data:/srv/ct-public-root/math-data:ro');
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
      'routers.ct-math.service=ct-web',
      'routers.ct-math.priority=1000',
      'routers.ct-math.middlewares=ct-math-rl@docker,ct-math-if@docker,ct-sec@docker,ct-math-csp@docker',
      'middlewares.ct-math-rl.ratelimit.average=5',
      'middlewares.ct-math-rl.ratelimit.burst=20',
      'middlewares.ct-math-rl.ratelimit.sourcecriterion.ipstrategy.depth=0',
      'middlewares.ct-math-if.inflightreq.amount=8',
      'middlewares.ct-math-if.inflightreq.sourcecriterion.ipstrategy.depth=0',
      "middlewares.ct-math-csp.headers.contentSecurityPolicy=default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ];
    for (const l of lines) expect(w, l).toContain(`traefik.http.${l}`);
    expect(w).not.toMatch(/ct-math\.middlewares=[^\n]*ct-auth/);
    expect(w).toContain('traefik.http.routers.ct-web.middlewares=ct-rl@docker,ct-sec@docker,ct-csp@docker,ct-auth@docker');
    expect(w.match(/ct-math/g)).toHaveLength(15);
  });

  it('keeps login on every other router and the dashboard CSP unchanged', () => {
    const y = repo('compose.web.yml');
    const mw = (r: string): string => new RegExp(`routers\\.${r}\\.middlewares=(\\S+)`).exec(y)?.[1] ?? '';
    expect(mw('ct-api')).toBe('ct-rl@docker,ct-sec@docker,ct-csp@docker,ct-auth@docker');
    expect(mw('ct-web')).toBe('ct-rl@docker,ct-sec@docker,ct-csp@docker,ct-auth@docker');
    expect(mw('ct-oauth')).toBe('ct-rl@docker,ct-sec@docker,ct-csp@docker');
    expect(y).toContain("contentSecurityPolicy=default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com;");
    expect(y).not.toContain('wasm-unsafe-eval; ');
    // ct-math is the only router without ct-auth that is not oauth2-proxy's own, and ct-sec exists for it to name
    expect(y).toContain('traefik.http.middlewares.ct-sec.headers.stsSeconds=31536000');
    const noAuth = [...y.matchAll(/routers\.([\w-]+)\.middlewares=(\S+)/g)].filter((m) => !m[2].includes('ct-auth')).map((m) => m[1]);
    expect(noAuth.sort()).toEqual(['ct-http', 'ct-math', 'ct-oauth']);
  });
});
