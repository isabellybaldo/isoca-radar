import { Link } from 'react-router';
import { api } from '../api/client';
import { useStream } from '../api/StreamContext';
import type { EpisodeSummary, HealthNow } from '../api/types';
import { useApi } from '../api/useApi';
import { Age } from '../components/Age';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Hearts, troubles } from '../components/Hearts';
import { Help } from '../components/Help';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Spark } from '../components/Spark';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { useNow, useTicker } from '../lib/hooks';
import { kindTag } from '../lib/labels';
import { formatDecimal } from '../lib/money';

export const EMPTY_RADAR = '0 opportunities. The market is efficient, or we are slow. Probably both.';
export const NO_HEALTH = 'No health rows yet. The scanner may be asleep.';
export const OPEN_STALE = 'scanner not reporting — list may be stale';
/** Past this age (ms) the last health minute is old enough that the open-episodes list itself may
 * no longer reflect reality (the scanner could be down, and nothing would ever close these rows). */
const STALE_HEALTH_MS = 180_000;

/** Spec §5: N gross, M net. "Fees: N−M. Us: M." */
export function todayQuip(gross: number, net: number): string {
  return `${gross} gross, ${net} net. "Fees: ${gross - net}. Us: ${net}."`;
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

export function Radar() {
  const { episodeTick, healthTick } = useStream();
  const slow = useTicker(30_000);
  const health = useApi((s) => api.healthNow(s), [], [healthTick, slow]);
  const open = useApi((s) => api.openEpisodes(s), [], [episodeTick, slow]);
  const today = useApi((s) => api.today(s), [], [episodeTick, slow]);
  const now = useNow(100);
  const rows = open.data?.items ?? [];
  const staleHealth = (health.data?.health?.age_ms ?? 0) > STALE_HEALTH_MS;
  const maxEdge = Math.max(0, ...rows.map((r) => Number(r.edge)));
  const columns: Column<EpisodeSummary>[] = [
    {
      key: 'kind',
      label: 'KIND',
      render: (r) => {
        const t = kindTag(r.kind);
        return <Tag tone={t.tone}>{t.text}</Tag>;
      },
    },
    {
      key: 'event',
      label: 'EVENT',
      render: (r) => <Link to={`/episodes/${encodeURIComponent(r.id)}`}>{r.event_title ?? r.id}</Link>,
    },
    { key: 's', label: 'S', render: (r) => r.s_legs },
    { key: 'net', label: 'NET', render: (r) => <Money value={r.net} signed tone /> },
    { key: 'gross', label: 'GROSS', render: (r) => <Money value={r.gross} signed /> },
    { key: 'q', label: 'Q*', render: (r) => formatDecimal(r.q_star) },
    { key: 'age', label: 'AGE', render: (r) => <Age sinceMs={r.opened_ms} nowMs={now} /> },
    { key: 'edge', label: 'EDGE', render: (r) => <EdgeBar edge={r.edge} max={maxEdge} /> },
  ];
  return (
    <>
      <Loadable state={health} what="health">
        {(d) => (d.health === null ? <Box help="health"><EmptyState text={NO_HEALTH} /></Box> : <HealthBar h={d.health} />)}
      </Loadable>
      <Box title={`OPEN EPISODES · ${rows.length}`} help="open">
        {staleHealth && <Tag tone="flag">{OPEN_STALE}</Tag>}
        <Loadable state={open} what="open episodes">
          {(d) =>
            d.items.length === 0 ? (
              <EmptyState text={EMPTY_RADAR} />
            ) : (
              <Table columns={columns} rows={d.items} rowKey={(r) => r.id} />
            )
          }
        </Loadable>
      </Box>
      <Box title="TODAY (UTC)" help="today">
        <Loadable state={today} what="today">
          {(d) => (
            <>
              <Spark values={d.hourly.map((h) => h.gross)} label="gross episodes per hour today" />
              <p className="quip">{todayQuip(d.gross, d.net)}</p>
            </>
          )}
        </Loadable>
      </Box>
    </>
  );
}
