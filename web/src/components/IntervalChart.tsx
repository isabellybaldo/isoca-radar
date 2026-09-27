import type { SweetLabel, SweetRow } from '../api/types';
import { formatPercent } from '../lib/money';
import { FEW_TRADES, LABEL_TAG, LABEL_TEXT, interval, rowName } from '../lib/sweet';
import { EmptyState } from './EmptyState';
import { Tag, type TagTone } from './Tag';

export const NO_ROWS = 'No rows in this slice yet. Try ALL, or a wider window.';
const W = 200;
const H = 10;

/** Fill, dash and tag carry the label as well as colour (redesign §5.1, §9 item 4). */
const LOOK: Record<SweetLabel, { tone: 'good' | 'bad' | 'dim'; hollow: boolean; dashed: boolean; tag: TagTone }> = {
  candidate: { tone: 'good', hollow: false, dashed: false, tag: 'no' },
  negative: { tone: 'bad', hollow: true, dashed: false, tag: 'flag' },
  dead_on_paper: { tone: 'bad', hollow: true, dashed: false, tag: 'flag' },
  inconclusive: { tone: 'dim', hollow: false, dashed: false, tag: 'plain' },
  thin: { tone: 'dim', hollow: false, dashed: true, tag: 'plain' },
  not_tested: { tone: 'dim', hollow: false, dashed: true, tag: 'plain' },
};

/** One shared $/attempt axis for every row, always including zero. Numbers are geometry only. */
function domain(rows: SweetRow[]): { lo: number; hi: number } {
  const vs = rows.flatMap((r) => [r.lo, r.mean, r.hi]).filter((v): v is string => v !== null).map(Number);
  const lo = Math.min(0, ...vs);
  const hi = Math.max(0, ...vs);
  const pad = (hi - lo || 1) * 0.05;
  return { lo: lo - pad, hi: hi + pad };
}

export function IntervalChart({ rows, label }: { rows: SweetRow[]; label: string }) {
  if (rows.length === 0) return <EmptyState text={NO_ROWS} />;
  const d = domain(rows);
  const sx = (v: number) => Math.round(((v - d.lo) / (d.hi - d.lo)) * (W - 2)) + 1;
  const zero = sx(0);
  return (
    <ul className="plain ivchart" aria-label={label}>
      {rows.map((r) => {
        const look = LOOK[r.label];
        const hasBar = r.mean !== null;
        const x1 = sx(Number(r.lo ?? r.mean ?? 0));
        const x2 = sx(Number(r.hi ?? r.mean ?? 0));
        return (
          <li key={`${r.side}|${r.dimension}|${r.bucket}|${r.scenario}`} className={`iv ${look.tone}`} data-label={r.label}>
            <span className="iv-name">{rowName(r)}</span>
            <svg className="iv-track" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
              <rect className="zero" x={zero} y={0} width={1} height={H} />
              {hasBar && (
                <>
                  <line className={look.dashed ? 'bar dashed' : 'bar'} x1={x1} x2={Math.max(x2, x1 + 1)} y1={H / 2} y2={H / 2} />
                  <rect className={look.hollow ? 'dot hollow' : 'dot'} x={sx(Number(r.mean)) - 1} y={1} width={3} height={H - 2} />
                </>
              )}
            </svg>
            <span className="iv-num num">{interval(r)}</span>
            <span className="iv-meta">
              <Tag tone={look.tag} title={LABEL_TEXT[r.label]}>{LABEL_TAG[r.label]}</Tag>
              <span className="dim">{`n ${r.n_exec} · C ${r.clusters}`}</span>
              {r.side === 'yes' && r.annualized !== null && <span className="dim">{` · ${formatPercent(r.annualized)}/yr`}</span>}
              {r.few_trades && <span className="dim"> {FEW_TRADES}</span>}
            </span>
          </li>
        );
      })}
    </ul>
  );
}
