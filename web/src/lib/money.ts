/**
 * Money arrives as decimal strings (spec §4) and is formatted with string operations only.
 * Never parseFloat, Number(), toFixed, toLocaleString or Intl on money: a float rounds
 * 12345678901234567.89 and turns 0.000001 into 0.00.
 */
const DEC = /^(-?)(\d+)(?:\.(\d+))?$/;

export function groupThousands(int: string): string {
  let out = '';
  for (let i = 0; i < int.length; i++) {
    if (i > 0 && (int.length - i) % 3 === 0) out += ',';
    out += int[i];
  }
  return out;
}

export function isZero(value: string): boolean {
  return /^-?0+(\.0+)?$/.test(value.trim());
}

export function signOf(value: string | null | undefined): -1 | 0 | 1 {
  if (value === null || value === undefined || isZero(value)) return 0;
  return value.trim().startsWith('-') ? -1 : 1;
}

/** "1002.56" → "$1,002.56"; "0.1" → "$0.10" (padded, never rounded); null → "—" (not measured). */
export function formatMoney(value: string | null | undefined, opts: { signed?: boolean } = {}): string {
  if (value === null || value === undefined) return '—';
  const m = DEC.exec(value.trim());
  if (!m) return value;
  const [, minus, int, frac = ''] = m;
  const body = `$${groupThousands(int)}.${frac.length >= 2 ? frac : frac.padEnd(2, '0')}`;
  if (isZero(value)) return body;
  if (minus) return `−${body}`;
  return opts.signed ? `+${body}` : body;
}

/**
 * Headline money: rounds to the cent, half away from zero, using string/BigInt math only (never a
 * float). Pair with a `title` tooltip carrying the exact value from `formatMoney`. null → "—".
 */
export function formatMoneyCents(value: string | null | undefined, opts: { signed?: boolean } = {}): string {
  if (value === null || value === undefined) return '—';
  const m = DEC.exec(value.trim());
  if (!m) return value;
  const [, minus, int, frac = ''] = m;
  const roundUp = frac.length > 2 && frac[2] >= '5';
  let whole = BigInt(int);
  let cents = BigInt(frac.slice(0, 2).padEnd(2, '0')) + (roundUp ? 1n : 0n);
  if (cents >= 100n) {
    cents -= 100n;
    whole += 1n;
  }
  const body = `$${groupThousands(whole.toString())}.${cents.toString().padStart(2, '0')}`;
  if (whole === 0n && cents === 0n) return body;
  if (minus) return `−${body}`;
  return opts.signed ? `+${body}` : body;
}

/** Any other decimal (prices, edges, Q*): as sent, grouped, with a real minus sign. */
export function formatDecimal(value: string | null | undefined): string {
  if (value === null || value === undefined) return '—';
  const m = DEC.exec(value.trim());
  if (!m) return value;
  const [, minus, int, frac] = m;
  return `${minus && !isZero(value) ? '−' : ''}${groupThousands(int)}${frac ? `.${frac}` : ''}`;
}

/** Exact order of two decimal strings (-1, 0, 1), by string and BigInt math only. Not-a-decimal is 0. */
export function compareDecimal(a: string, b: string): -1 | 0 | 1 {
  const ma = DEC.exec(a.trim());
  const mb = DEC.exec(b.trim());
  if (!ma || !mb) return 0;
  const fa = ma[3] ?? '';
  const fb = mb[3] ?? '';
  const n = Math.max(fa.length, fb.length);
  const scaled = (m: RegExpExecArray, f: string) => {
    const v = BigInt(`${m[2]}${f.padEnd(n, '0')}`);
    return m[1] ? -v : v;
  };
  const x = scaled(ma, fa);
  const y = scaled(mb, fb);
  return x < y ? -1 : x > y ? 1 : 0;
}
