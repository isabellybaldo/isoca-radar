import { useState } from 'react';
import { Link } from 'react-router';
import { api } from '../api/client';
import { useStream } from '../api/StreamContext';
import type { EpisodeSummary, HealthNow, Today } from '../api/types';
import { useApi } from '../api/useApi';
import { useExplainPage, useExplainSelect } from '../explain/ExplainContext';
import { Age } from '../components/Age';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Hearts, troubles } from '../components/Hearts';
import { Help } from '../components/Help';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Spark } from '../components/Spark';
import { SweetCard } from '../components/SweetCard';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { teaserAmountExact, teaserLine, withThinEdge } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';
import { kindTag } from '../lib/labels';
import { formatDecimal } from '../lib/money';

export const NO_ARB = 'No arbitrage open right now. They rarely last.';
export const NO_HEALTH = 'No health rows yet. The scanner may be asleep.';
export const OPEN_STALE = 'scanner not reporting — list may be stale';
export const INCOMPLETE_WHY = "An outcome outside the set can win (e.g. 'Other'). Shown for context, not money.";
/** Episodes are prices observed, not fills: what was on the table is not what was captured. */
export const TODAY_SUBTITLE = 'On the table, not in the bag. See Edge capture for what latency kept.';
/** Past this age (ms) the last health minute is old enough that the open-episodes list itself may
 * no longer reflect reality (the scanner could be down, and nothing would ever close these rows). */
const STALE_HEALTH_MS = 180_000;
const ARB_KINDS = ['yes_basket', 'no_basket'];

/** Complete YES or NO baskets are arbitrage; incomplete sets (and anything else) are not. */
export function isArbitrage(kind: string): boolean {
  return ARB_KINDS.includes(kind);
}

/**
 * "Arbitrage episodes today: N (NO k · YES m)". Counts only — the api's `edge_budget100` field is
 * unchanged, but the single money figure for today lives in the Edge teaser line below (the EDGE
 * `today` card), which is what the capture rate compares against.
 */
export function todayLine(t: Today): string {
  return `Arbitrage episodes today: ${t.arb} (NO ${t.no} · YES ${t.yes})`;
}

/** Messages per minute → per second, "4.3k" style. Counts, not money. */
export function msgsPerSecond(msgsPerMinute: number): string {
  const r = Math.round(msgsPerMinute / 60);
  return r >= 1000 ? `${Math.floor(r / 1000)}.${Math.floor((r % 1000) / 100)}k` : String(r);
}

function HealthBar({ h }: { h: HealthNow }) {
  return (
    <section className="box health" aria-label="scanner health">
      <Hearts health={h} />
      <span className="stat">
        shards <b>{`${h.shards - h.parked_shards}/${h.shards}`}</b>
      </span>
      <span className="stat">
        msgs/s <b>{msgsPerSecond(h.msgs)}</b>
      </span>
      <span className="stat">
        lag p50 <b>{h.lag_p50_ms === null ? '—' : `${h.lag_p50_ms} ms`}</b>
      </span>
      <span className="stat">
        drops <b>{h.drops_5m}</b>
      </span>
      {troubles(h).map((t) => (
        <Tag key={t} tone="flag">
          {t.toUpperCase()}
        </Tag>
      ))}
      <Help id="health" />
    </section>
  );
}

/** Edge bar: geometry only (the number never reaches the screen as text). */
function EdgeBar({ edge, max }: { edge: string; max: number }) {
  const v = Number(edge);
  const w = max > 0 && v > 0 ? Math.max(1, Math.round((v / max) * 60)) : 0;
  return (
    <svg className="edgebar" width="60" height="10" aria-hidden="true">
      <rect x="0" y="0" width={w} height="10" />
    </svg>
  );
}

/** Every cell names its column (`c-<key>`), so the < 600 px CSS can lay each row out as a card. */
const cell = (key: string) => () => `c-${key}`;

function episodeColumns(rows: EpisodeSummary[], now: number): Column<EpisodeSummary>[] {
  const maxEdge = Math.max(0, ...rows.map((r) => Number(r.edge)));
  return [
    {
      key: 'kind',
      label: 'KIND',
      className: cell('kind'),
      render: (r) => {
        const t = kindTag(r.kind);
        return <Tag tone={t.tone}>{t.text}</Tag>;
      },
    },
    {
      key: 'event',
      label: 'EVENT',
      className: cell('event'),
      render: (r) => {
        const title = r.event_title ?? r.id;
        return <Link to={`/episodes/${encodeURIComponent(r.id)}`} title={title}>{title}</Link>;
      },
    },
    { key: 's', label: 'S', className: cell('s'), num: true, render: (r) => r.s_legs },
    { key: 'b100', label: 'PROFIT @ $100', className: cell('b100'), num: true, render: (r) => <Money value={r.net_budget100} signed tone cents /> },
    { key: 'net', label: 'NET @ DEPTH', className: cell('net'), num: true, render: (r) => <Money value={r.net} signed tone cents /> },
    { key: 'gross', label: 'GROSS @ DEPTH', className: cell('gross'), num: true, render: (r) => <Money value={r.gross} signed cents /> },
    { key: 'q', label: 'Q* (SHARES)', className: cell('q'), num: true, render: (r) => formatDecimal(r.q_star) },
    { key: 'age', label: 'AGE', className: cell('age'), num: true, render: (r) => <Age sinceMs={r.opened_ms} nowMs={now} /> },
    { key: 'edge', label: 'EDGE', className: cell('edge'), render: (r) => <EdgeBar edge={r.edge} max={maxEdge} /> },
  ];
}

function ColumnLegend() {
  return (
    <div className="legend-note" aria-label="column legend">
      <ul className="plain dim">
        <li>KIND: YES = buy every YES (exactly one pays $1); NO = buy every NO and convert.</li>
        <li>S: outcomes in the set.</li>
        <li>PROFIT @ $100: net profit if you spent at most $100.</li>
        <li>NET @ DEPTH / GROSS: after / before fees, taking the full available depth.</li>
        <li>Q*: executable size in shares. AGE: time since it opened. EDGE: per-share edge, bar relative to the biggest here.</li>
      </ul>
      <Help id="columns" />
    </div>
  );
}

function IncompleteSets({ rows, now, onPick }: { rows: EpisodeSummary[]; now: number; onPick: (r: EpisodeSummary) => void }) {
  const [open, setOpen] = useState(false);
  const noun = rows.length === 1 ? 'incomplete set' : 'incomplete sets';
  return (
    <Box title={`NOT ARBITRAGE: INCOMPLETE SETS · ${rows.length}`} help="incomplete" className="dimmed">
      <p className="dim">{INCOMPLETE_WHY}</p>
      {rows.length > 0 && (
        <>
          <div className="pager">
            <button type="button" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
              {`${open ? 'hide' : 'show'} ${rows.length} ${noun}`}
            </button>
          </div>
          {open && <Table className="arbtable" columns={episodeColumns(rows, now)} rows={rows} rowKey={(r) => r.id} rowClass={() => 'dim'} onRowClick={onPick} />}
        </>
      )}
    </Box>
  );
}

export function Radar() {
  const { episodeTick, healthTick } = useStream();
  const slow = useTicker(30_000);
  const health = useApi((s) => api.healthNow(s), [], [healthTick, slow]);
  const open = useApi((s) => api.openEpisodes(s), [], [episodeTick, slow]);
  useExplainPage('radar', { open: open.data?.items });
  const select = useExplainSelect();
  const pick = (r: EpisodeSummary) => select({ kind: 'episode', episode: r });
  const today = useApi((s) => api.today(s), [], [episodeTick, slow]);
  const edgeToday = useApi((s) => api.cards('today', s), [], [slow]);
  const edge = edgeToday.data?.cards.find((c) => c.card === 'EDGE');
  const now = useNow(100);
  const all = open.data?.items ?? [];
  const arb = all.filter((r) => isArbitrage(r.kind));
  const rest = all.filter((r) => !isArbitrage(r.kind));
  const staleHealth = (health.data?.health?.age_ms ?? 0) > STALE_HEALTH_MS;
  return (
    <>
      <Loadable state={health} what="health">
        {(d) => (d.health === null ? <Box help="health"><EmptyState text={NO_HEALTH} /></Box> : <HealthBar h={d.health} />)}
      </Loadable>
      <Box title="TODAY (UTC)" help="today">
        <Loadable state={today} what="today">
          {(d) => (
            <>
              <Spark values={d.hourly.map((h) => h.arb)} label="arbitrage episodes per hour today" />
              <p className="quip">{todayLine(d)}</p>
              <p className="dim">{TODAY_SUBTITLE}</p>
              {edge && (
                <p className="quip teaser" title={teaserAmountExact(withThinEdge(edge)) ?? undefined}>
                  {teaserLine(withThinEdge(edge))}
                </p>
              )}
            </>
          )}
        </Loadable>
      </Box>
      <SweetCard />
      <Box title={`ARBITRAGE (YES / NO) · ${arb.length}`} help="open">
        {staleHealth && <Tag tone="flag">{OPEN_STALE}</Tag>}
        <Loadable state={open} what="open episodes">
          {() =>
            arb.length === 0 ? (
              <EmptyState text={NO_ARB} />
            ) : (
              <>
                <Table className="arbtable" columns={episodeColumns(arb, now)} rows={arb} rowKey={(r) => r.id} onRowClick={pick} />
                <ColumnLegend />
              </>
            )
          }
        </Loadable>
      </Box>
      <IncompleteSets rows={rest} now={now} onPick={pick} />
    </>
  );
}
