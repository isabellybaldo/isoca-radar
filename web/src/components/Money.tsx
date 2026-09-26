import { formatMoney, signOf } from '../lib/money';

/** A money decimal string, rendered as sent (spec §6). `tone` colours gains/losses (the sign is text too). */
export function Money({ value, signed = false, tone = false }: { value: string | null | undefined; signed?: boolean; tone?: boolean }) {
  const s = signOf(value);
  const cls = tone && s !== 0 ? (s > 0 ? 'pos' : 'neg') : undefined;
  return (
    <span className={cls} title={value === null || value === undefined ? 'not measured' : undefined}>
      {formatMoney(value, { signed })}
    </span>
  );
}
