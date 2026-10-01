import type { Position } from '../api/types';
import { Money } from '../components/Money';
import { daysText, positionKey } from '../explain/models';
import { accountPolicyTag, shortId } from '../lib/labels';
import { geoPos } from './geo';

/** Spec 7a §9.2: open positions as blocks sized by basis (their cost; HTML flex, geometry only), each
 *  listed with its paper basis and days to resolution. */
export function LockedBlocks({ positions, selected, nowMs }: { positions: Position[]; selected: string | null; nowMs: number }) {
  const sizes = positions.map((p) => geoPos(p.basis));
  const total = sizes.reduce((a, b) => a + b, 0);
  return (
    <figure className="locked" aria-label="money locked until resolution">
      <div className="lk-row">
        {positions.map((p, i) => (
          <span
            key={positionKey(p)}
            className={positionKey(p) === selected ? 'lk-block sel' : 'lk-block'}
            style={{ flexGrow: total > 0 ? sizes[i] / total : 1 }}
            title={p.event_title ?? p.market_id}
          />
        ))}
      </div>
      <ul className="plain lk-list" aria-label="locked positions">
        {positions.map((p) => (
          <li key={positionKey(p)} className={positionKey(p) === selected ? 'sel' : undefined}>
            {p.event_title ?? shortId(p.market_id)} · {accountPolicyTag(p.account_id)} · <Money value={p.basis} cents /> locked on paper · {daysText(p.end_date, nowMs)}
          </li>
        ))}
      </ul>
    </figure>
  );
}
