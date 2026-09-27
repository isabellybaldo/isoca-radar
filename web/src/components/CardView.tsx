import { Link } from 'react-router';
import type { CardBody, CardRow } from '../api/types';
import type { HelpId } from '../help';
import { UNDER_REVIEW, computedText, fmtValue, shortHash, statusLabel, statusTone, warmingText } from '../lib/cards';
import { useNow } from '../lib/hooks';
import { formatMoney, formatMoneyCents } from '../lib/money';
import { fmtLocal, fmtUtcDateTime } from '../lib/time';
import { CardChart } from './CardCharts';
import { Fold } from './Fold';
import { SubHead } from './SubHead';
import { Tag } from './Tag';

const CARD_HELP: Record<string, HelpId> = { G1: 'g1', G2: 'g2', G3: 'g3', G4: 'g4' };
type Col = 'paper' | 'their' | 'ours';
const COLS: { key: Col; label: string }[] = [
  { key: 'paper', label: 'PAPER' },
  { key: 'their', label: 'THEIR METHOD' },
  { key: 'ours', label: 'OUR METHOD' },
];

/** A card value cell: USD headlines round to the cent with the exact value in a title; everything else as sent. */
function ValueCell({ value, unit }: { value: string | null; unit: string }) {
  if (unit !== 'USD') return <>{fmtValue(value, unit)}</>;
  if (value === null) return <span title="not measured">—</span>;
  return <span title={formatMoney(value)}>{formatMoneyCents(value)}</span>;
}

const CLAIM_LABEL: Record<Col, string> = { paper: 'paper', their: 'theirs', ours: 'ours' };

function usedCols(card: CardBody) {
  const used = COLS.filter((c) => card.columns.some((r: CardRow) => r[c.key] !== null));
  return used.length > 0 ? used : COLS.slice(2);
}

/** So a catch-up 0 (thin), a recompute (under_review) or a stale tick never reads as a live result. */
export function isDim(card: CardBody, stale: boolean): boolean {
  return card.status === 'thin' || card.status === 'under_review' || stale;
}

/** M5: why a card is thin, as dim lines under the claim/tiles — no longer buried in the folded method. */
export function ThinReasons({ reasons }: { reasons: string[] }) {
  return (
    <>
      {reasons.map((t) => (
        <p key={t} className="dim">
          thin: {t}
        </p>
      ))}
    </>
  );
}

/** Redesign §4.3: the card's headline claim (its first row) in big type, PAPER / THEIRS / OURS. */
export function Claim({ card, dim }: { card: CardBody; dim?: boolean }) {
  const r = card.columns[0];
  if (!r) return null;
  return (
    <div className="claim-wrap">
      <p className="dim">{r.label}</p>
      <dl className={dim ? 'claim dim' : 'claim'}>
        {usedCols(card).map((c) => (
          <div key={c.key}>
            <dt>{CLAIM_LABEL[c.key]}</dt>
            <dd>
              <ValueCell value={r[c.key]} unit={r.unit} />
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

export function CardTable({ card, stale }: { card: CardBody; stale: boolean }) {
  const dim = isDim(card, stale);
  const cols = usedCols(card);
  return (
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
                <td key={c.key} className="num">
                  <ValueCell value={r[c.key]} unit={r.unit} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function CardMeta({ card, nowMs }: { card: CardBody; nowMs: number }) {
  return (
    <>
      <p className="meta">
        n = {card.n} · <span title={card.hash}>#{shortHash(card.hash)}</span> ·{' '}
        <span title={fmtLocal(card.computed_ms)}>
          {computedText(card.computed_ms, nowMs)} ({fmtUtcDateTime(card.computed_ms)})
        </span>
      </p>
      <p className="meta premise">{card.premise_key}</p>
      {card.notes.map((n) => (
        <p key={n} className="note">
          {n}
        </p>
      ))}
    </>
  );
}

/** One card (redesign §4.3): header, status, drawing, claim; the table and the method folded. */
export function CardView({ card, stale = false, now }: { card: CardBody; stale?: boolean; now?: number }) {
  const ticking = useNow(30_000);
  const nowMs = now ?? ticking;
  return (
    <article className="cardv" aria-label={card.title}>
      <SubHead help={CARD_HELP[card.card] ?? 'status'} right={<Tag tone={statusTone(card.status)}>{statusLabel(card.status)}</Tag>}>
        {card.title}
      </SubHead>
      {card.status === 'warming' && <p className="dim">{warmingText(card)}</p>}
      {card.status === 'under_review' && <p className="dim">{UNDER_REVIEW}</p>}
      <CardChart card={card} />
      <Claim card={card} dim={isDim(card, stale)} />
      <ThinReasons reasons={card.thin_reasons} />
      <Fold id={`card:${card.card}:numbers`} label="all numbers">
        <CardTable card={card} stale={stale} />
      </Fold>
      {card.episode_link && (
        <p>
          <Link to={`/episodes/${encodeURIComponent(card.episode_link)}`}>biggest opportunity →</Link>
        </p>
      )}
      <Fold id={`card:${card.card}:how`} label="how it's computed">
        <CardMeta card={card} nowMs={nowMs} />
      </Fold>
    </article>
  );
}
