/** Bytes as GB (10^9 B, like the budgets) with one decimal, rounded down; integer math only. */
export function fmtGb(bytes: number): string {
  const tenths = Math.floor(bytes / 100_000_000);
  return `${Math.floor(tenths / 10)}.${tenths % 10} GB`;
}
