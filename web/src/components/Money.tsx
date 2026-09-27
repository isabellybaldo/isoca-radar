import { formatMoney, formatMoneyCents, signOf } from '../lib/money';

/**
 * A money decimal string (spec §6). `tone` colours gains/losses (the sign is text too). `cents`
 * rounds a headline amount to the cent (string/BigInt math only) and keeps the exact value in a
 * `title` tooltip; without it, the value is rendered as sent, in full precision.
 */
export function Money({
  value, signed = false, tone = false, cents = false,
}: { value: string | null | undefined; signed?: boolean; tone?: boolean; cents?: boolean }) {
  const s = signOf(value);
  const cls = tone && s !== 0 ? (s > 0 ? 'pos' : 'neg') : undefined;
  const missing = value === null || value === undefined;
  const title = missing ? 'not measured' : cents ? formatMoney(value, { signed }) : undefined;
  return (
    <span className={cls} title={title}>
      {cents ? formatMoneyCents(value, { signed }) : formatMoney(value, { signed })}
    </span>
  );
}
