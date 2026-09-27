export const DAY_MS = 86_400_000;

const pad = (n: number, w = 2) => String(n).padStart(w, '0');

export function fmtUtcTime(ms: number): string {
  return `${new Date(ms).toISOString().slice(11, 23)} UTC`;
}

export function fmtUtcDateTime(ms: number): string {
  const s = new Date(ms).toISOString();
  return `${s.slice(0, 10)} ${s.slice(11, 19)} UTC`;
}

export function fmtLocal(ms: number): string {
  return new Date(ms).toLocaleString();
}

/** 1_790_001_900_000 → "05:25" (UTC hours:minutes, zero-padded). */
export function fmtUtcHm(ms: number): string {
  const s = new Date(ms).toISOString();
  return s.slice(11, 16);
}

/** 4190 → "4.19 s"; 125000 → "2 min 5 s"; 7500000 → "2 h 5 min". Integer math only. */
export function fmtDuration(ms: number): string {
  const t = Math.max(0, Math.floor(ms));
  if (t < 60_000) return `${Math.floor(t / 1000)}.${pad(Math.floor((t % 1000) / 10))} s`;
  const s = Math.floor(t / 1000);
  if (s < 3600) return `${Math.floor(s / 60)} min ${s % 60} s`;
  return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`;
}

/** Radar age: 4200 → "00:04.2". */
export function fmtAge(ms: number): string {
  const tenths = Math.floor(Math.max(0, ms) / 100);
  return `${pad(Math.floor(tenths / 600))}:${pad(Math.floor((tenths % 600) / 10))}.${tenths % 10}`;
}

export function isoDay(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/** "2026-09-25" → ms at 00:00 UTC; anything else → null. */
export function parseIsoDay(s: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return null;
  const t = Date.parse(`${s}T00:00:00Z`);
  return Number.isNaN(t) ? null : t;
}
