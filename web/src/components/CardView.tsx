import { Link } from 'react-router';
import type { CardBody, CardRow } from '../api/types';
import type { HelpId } from '../help';
import { UNDER_REVIEW, fmtValue, shortHash, statusLabel, statusTone, warmingText } from '../lib/cards';
import { CardChart } from './CardCharts';
import { Help } from './Help';
import { Tag } from './Tag';

// EDGE sits inside the Edge capture box, whose own (?) explains the edge: its card (?) explains the status.
const CARD_HELP: Record<string, HelpId> = { G1: 'g1', G2: 'g2', G3: 'g3', G4: 'g4', EDGE: 'status' };
type Col = 'paper' | 'their' | 'ours';
const COLS: { key: Col; label: string }[] = [
  { key: 'paper', label: 'PAPER' },
  { key: 'their', label: 'THEIR METHOD' },
  { key: 'ours', label: 'OUR METHOD' },
];

/** One card (spec §5, §8.2): status in words, the three columns, n, notes and the short hash. */
export function CardView({ card, stale = false }: { card: CardBody; stale?: boolean }) {
  const dim = card.status === 'thin' || stale;
  const used = COLS.filter((c) => card.columns.some((r: CardRow) => r[c.key] !== null));
  const cols = used.length > 0 ? used : COLS.slice(2);
  return (
    <article className="cardv" aria-label={card.title}>
      <div className="box-head">
        <h3>{card.title}</h3>
        <Tag tone={statusTone(card.status)}>{statusLabel(card.status)}</Tag>
        <Help id={CARD_HELP[card.card] ?? 'status'} />
      </div>
      {card.status === 'warming' && <p className="dim">{warmingText(card)}</p>}
      {card.status === 'under_review' && <p className="dim">{UNDER_REVIEW}</p>}
      <CardChart card={card} />
      <div className="scroll">
        <table>
          <thead>
            <tr>
              <th scope="col" aria-label="measure" />
              {cols.map((c) => (
                <th key={c.key} scope="col">
                  {cols.length === 1 ? 'VALUE' : c.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {card.columns.map((r) => (
              <tr key={r.label} className={dim ? 'dim' : undefined}>
                <td className="rowlabel">{r.label}</td>
                {cols.map((c) => (
                  <td key={c.key}>{fmtValue(r[c.key], r.unit)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="meta">
        n = {card.n} · <span title={card.hash}>#{shortHash(card.hash)}</span>
      </p>
      <p className="meta premise">{card.premise_key}</p>
      {card.episode_link && (
        <p>
          <Link to={`/episodes/${encodeURIComponent(card.episode_link)}`}>biggest opportunity →</Link>
        </p>
      )}
      {card.thin_reasons.map((t) => (
        <p key={t} className="dim">
          thin: {t}
        </p>
      ))}
      {card.notes.map((n) => (
        <p key={n} className="note">
          {n}
        </p>
      ))}
    </article>
  );
}
