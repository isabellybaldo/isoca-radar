import { useEffect } from 'react';
import { Link, useParams } from 'react-router';
import { api } from '../api/client';
import type { Episode, EpisodeDetail as Detail, EpisodeSummary, Leg, Observation } from '../api/types';
import { useApi } from '../api/useApi';
import { useExplainPage, useExplainSelect } from '../explain/ExplainContext';
import { Box } from '../components/Box';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { FLAG_WORDS } from '../lib/flags';
import { kindTag, shortId } from '../lib/labels';
import { formatDecimal, signOf } from '../lib/money';
import { fmtDuration, fmtLocal, fmtUtcTime } from '../lib/time';

export { FLAG_WORDS };

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
        <Box title="TIMING" help="timing">
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
        <Box title="AT PEAK" help="peak">
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
        <Box title="FLAGS, IN WORDS" help="flags">
          {words.length === 0 ? <p className="quip">No flags. Suspiciously clean.</p> : words.map((w) => <p key={w} className="quip">{w}</p>)}
        </Box>
      </div>
      <Box title={`LEGS AT PEAK · S = ${inS} of ${e.n_legs}`} help="legs">
        <Table columns={legColumns} rows={d.legs} rowKey={(l) => String(l.leg_idx)} rowClass={(l) => (l.in_s ? undefined : 'dim')} />
      </Box>
      <Box title="RE-CHECK TIMELINE" help="rechecks">
        <Timeline obs={d.observations} />
        <Table columns={timelineColumns} rows={timelineRows(e, d.observations)} rowKey={(r) => r.key} />
        {e.closed_ms !== null && e.duration_ms !== null && e.duration_ms < 10_000 && (
          <p className="quip">{`Gone in ${fmtDuration(e.duration_ms)}. Like most good ideas.`}</p>
        )}
      </Box>
    </>
  );
}

function summaryOf({ episode: e, legs }: Detail): EpisodeSummary {
  return {
    id: e.id, kind: e.kind, realization: e.realization, event_title: e.event_title, event_slug: e.event_slug,
    n_legs: e.n_legs, s_legs: legs.filter((l) => l.in_s).length, opened_ms: e.opened_ms, closed_ms: e.closed_ms,
    duration_ms: e.duration_ms, edge: e.edge, q_star: e.q_star, net: e.net, gross: e.gross, net_budget100: e.net_budget100,
    suspect: e.suspect, stale: e.stale, censored: e.censor_left || e.censor_right || e.censor_gap,
  };
}

export function EpisodeDetail() {
  const { id = '' } = useParams();
  const d = useApi((s) => api.episode(id, s), [id]);
  useExplainPage('episode', { detail: d.data });
  // D-12 (spec 7a §9.3): being on an episode's page selects it; leaving clears it (the provider clears on unmount).
  const select = useExplainSelect();
  const loaded = d.data;
  useEffect(() => {
    if (loaded) select({ kind: 'episode', episode: summaryOf(loaded) });
    return () => select(null);
  }, [loaded, select]);
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
