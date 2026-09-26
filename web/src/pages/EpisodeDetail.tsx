import { Link, useParams } from 'react-router';
import { api } from '../api/client';
import type { Episode, EpisodeDetail as Detail, Leg, Observation } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { kindTag } from '../lib/labels';
import { formatDecimal, signOf } from '../lib/money';
import { fmtDuration, fmtLocal, fmtUtcTime } from '../lib/time';

export const FLAG_WORDS = {
  suspect:
    "SUSPECT: one leg's book disagreed with the server's top while this was open. The stream repaired it; the numbers may still be optimistic.",
  stale: "STALE: at least one leg's feed went quiet while this was open, so its book may have been out of date.",
  censor_left: 'CENSORED (LEFT): it was already open when we started watching, so the real start is earlier.',
  censor_right: 'CENSORED (RIGHT): we stopped watching before it closed, so the real end is later.',
  censor_gap: 'CENSORED (GAP): the feed had a gap while it was open; the duration spans the gap.',
  incomplete: 'INCOMPLETE SET: the listed outcomes may not cover every result. Not an arbitrage if "Other" wins.',
  below_min: 'BELOW MIN SIZE: the size at the best price is under the venue minimum order.',
};

export function flagSentences(e: Episode): string[] {
  const out: string[] = [];
  if (e.suspect) out.push(FLAG_WORDS.suspect);
  if (e.stale) out.push(FLAG_WORDS.stale);
  if (e.censor_left) out.push(FLAG_WORDS.censor_left);
  if (e.censor_right) out.push(FLAG_WORDS.censor_right);
  if (e.censor_gap) out.push(FLAG_WORDS.censor_gap);
  if (e.incomplete_set) out.push(FLAG_WORDS.incomplete);
  if (e.below_min_size) out.push(FLAG_WORDS.below_min);
  return out;
}

export function censorText(e: Episode): string {
  const parts: string[] = [];
  if (e.censor_left) parts.push('left');
  if (e.censor_right) parts.push('right');
  if (e.censor_gap) parts.push('gap');
  return parts.length > 0 ? parts.join(', ') : 'none';
}

/** 1000 → "+1 s"; the offset equal to secondsDelay (Δd, spec §8.2) → "+Δd (3 s)". */
export function recheckLabel(offsetMs: number, maxSecondsDelay: number | null): string {
  const s = offsetMs % 1000 === 0 ? `${offsetMs / 1000} s` : fmtDuration(offsetMs);
  if (maxSecondsDelay !== null && maxSecondsDelay > 0 && offsetMs === maxSecondsDelay * 1000) return `+Δd (${s})`;
  return `+${s}`;
}

export function actionSide(action: string): string {
  if (action === 'buy_yes') return 'YES';
  if (action === 'buy_no') return 'NO';
  return action.toUpperCase();
}

function signedDec(v: string | null): string {
  if (v === null) return '—';
  return signOf(v) > 0 ? `+${formatDecimal(v)}` : formatDecimal(v);
}

function shortId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 6)}…${id.slice(-4)}` : id;
}

const legColumns: Column<Leg>[] = [
  { key: 'leg', label: 'LEG', render: (l) => <span title={l.market_id}>{`#${l.leg_idx} ${shortId(l.market_id)}`}</span> },
  { key: 'buy', label: 'BUY', render: (l) => actionSide(l.action) },
  { key: 'best', label: 'BEST', render: (l) => formatDecimal(l.best_price) },
  { key: 'vwap', label: 'VWAP', render: (l) => formatDecimal(l.vwap) },
  { key: 'levels', label: 'LEVELS', render: (l) => l.levels },
  { key: 'size', label: 'SIZE@BEST', render: (l) => formatDecimal(l.size_at_best) },
  { key: 'fees', label: 'FEES', render: (l) => <Money value={l.fees} /> },
  { key: 's', label: 'IN S', render: (l) => (l.in_s ? '✓' : '·') },
];

interface TimelineRow {
  key: string;
  when: string;
  source: string;
  profitable: string;
  tone: 'pos' | 'neg' | undefined;
  edge: string | null;
  q: string | null;
}

const timelineColumns: Column<TimelineRow>[] = [
  { key: 'when', label: 'WHEN', render: (r) => r.when },
  { key: 'source', label: 'SOURCE', render: (r) => r.source },
  { key: 'ok', label: 'STILL PROFITABLE', render: (r) => r.profitable, className: (r) => r.tone },
  { key: 'edge', label: 'EDGE', render: (r) => signedDec(r.edge) },
  { key: 'q', label: 'Q*', render: (r) => formatDecimal(r.q) },
];

function timelineRows(e: Episode, obs: Observation[]): TimelineRow[] {
  // Open-time values aren't stored; the first row is the peak the episode reached.
  const peak: TimelineRow = { key: 'peak', when: 'peak', source: '—', profitable: 'yes', tone: 'pos', edge: e.edge, q: e.q_star };
  return [
    peak,
    ...obs.map((o, i): TimelineRow => ({
      key: `o${i}`,
      when: recheckLabel(o.offset_ms, e.max_seconds_delay),
      source: o.source,
      profitable: o.skipped
        ? `skipped: ${o.skip_reason ?? 'no reason given'}`
        : o.still_profitable === null
          ? '—'
          : o.still_profitable
            ? 'yes'
            : 'no',
      tone: o.skipped || o.still_profitable === null ? undefined : o.still_profitable ? 'pos' : 'neg',
      edge: o.edge,
      q: o.q_star,
    })),
  ];
}

function Timeline({ obs }: { obs: Observation[] }) {
  const max = Math.max(1, ...obs.map((o) => o.offset_ms));
  return (
    <svg className="tl" viewBox="0 0 100 14" preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label="re-check timeline">
      <rect className="axis" x={0} y={6} width={100} height={1} />
      <rect className="ok" x={1} y={3} width={2} height={7} />
      {obs.map((o, i) => {
        const x = Math.min(97, 4 + Math.round((o.offset_ms / max) * 93));
        const cls = o.skipped || o.still_profitable === null ? 'skip' : o.still_profitable ? 'ok' : 'bad';
        return <rect key={i} className={cls} x={x} y={3} width={2} height={7} />;
      })}
    </svg>
  );
}

function DetailView({ d }: { d: Detail }) {
  const e = d.episode;
  const k = kindTag(e.kind, true);
  const words = flagSentences(e);
  const inS = d.legs.filter((l) => l.in_s).length;
  return (
    <>
      <p className="title">
        {e.event_title ?? e.event_id} <Tag tone={k.tone}>{k.text}</Tag>
        {e.suspect && <Tag tone="flag">SUSPECT</Tag>}
        {e.stale && <Tag tone="flag">STALE</Tag>}
        {censorText(e) !== 'none' && <Tag tone="flag">CENSORED</Tag>}
        {e.source === 'replay' && <Tag>REPLAY</Tag>}
      </p>
      <div className="grid2">
        <Box title="TIMING">
          <dl className="kv">
            <dt>opened</dt>
            <dd title={fmtLocal(e.opened_ms)}>{fmtUtcTime(e.opened_ms)}</dd>
            <dt>closed</dt>
            <dd title={e.closed_ms === null ? undefined : fmtLocal(e.closed_ms)}>{e.closed_ms === null ? 'still open' : fmtUtcTime(e.closed_ms)}</dd>
            <dt>duration</dt>
            <dd>{e.duration_ms === null ? '—' : fmtDuration(e.duration_ms)}</dd>
            <dt>censoring</dt>
            <dd>{censorText(e)}</dd>
            <dt>detect lag</dt>
            <dd>{e.detect_lag_ms === null ? '—' : `${e.detect_lag_ms} ms`}</dd>
          </dl>
        </Box>
        <Box title="AT PEAK">
          <dl className="kv">
            <dt>net Π</dt>
            <dd><Money value={e.net} signed tone /></dd>
            <dt>gross Π</dt>
            <dd><Money value={e.gross} signed /></dd>
            <dt>fees</dt>
            <dd><Money value={e.fees} /></dd>
            <dt>Q*</dt>
            <dd>{`${formatDecimal(e.q_star)} sets`}</dd>
            <dt>$100 cap</dt>
            <dd><Money value={e.net_budget100} signed tone /></dd>
          </dl>
        </Box>
        <Box title="FLAGS, IN WORDS">
          {words.length === 0 ? <p className="quip">No flags. Suspiciously clean.</p> : words.map((w) => <p key={w} className="quip">{w}</p>)}
        </Box>
      </div>
      <Box title={`LEGS AT PEAK · S = ${inS} of ${e.n_legs}`}>
        <Table columns={legColumns} rows={d.legs} rowKey={(l) => String(l.leg_idx)} rowClass={(l) => (l.in_s ? undefined : 'dim')} />
      </Box>
      <Box title="RE-CHECK TIMELINE">
        <Timeline obs={d.observations} />
        <Table columns={timelineColumns} rows={timelineRows(e, d.observations)} rowKey={(r) => r.key} />
        {e.closed_ms !== null && e.duration_ms !== null && e.duration_ms < 10_000 && (
          <p className="quip">{`Gone in ${fmtDuration(e.duration_ms)}. Like most good ideas.`}</p>
        )}
      </Box>
    </>
  );
}

export function EpisodeDetail() {
  const { id = '' } = useParams();
  const d = useApi((s) => api.episode(id, s), [id]);
  return (
    <>
      <p className="crumb">
        <Link to="/episodes">EPISODES</Link> › {id}
      </p>
      <Loadable state={d} what="the episode">
        {(x) => <DetailView d={x} />}
      </Loadable>
    </>
  );
}
