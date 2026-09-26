import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import { useStream } from '../api/StreamContext';
import type { HealthNow, HealthSeries, Integrity } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { LineChart } from '../components/LineChart';
import { Loadable } from '../components/Loadable';
import { Table } from '../components/Table';
import { Tag } from '../components/Tag';
import { useTicker } from '../lib/hooks';
import { formatDecimal } from '../lib/money';
import { fmtDuration, fmtLocal, fmtUtcDateTime } from '../lib/time';

export const NO_HEALTH_ROWS = 'No health rows in this window. The scanner may be asleep.';
export const NO_INTEGRITY = 'No integrity events in this window. Either the books agree, or nobody checked.';
const WINDOWS = [6, 24, 168];

function CaptureStatus({ h }: { h: HealthNow }) {
  return (
    <dl className="kv">
      <dt>capture</dt>
      <dd>{h.capture_paused ? <Tag tone="flag">PAUSED</Tag> : 'recording'}</dd>
      <dt>shards up</dt>
      <dd>{`${h.shards - h.parked_shards}/${h.shards}`}</dd>
      <dt>tokens</dt>
      <dd>{h.tokens}</dd>
      <dt>/ok</dt>
      <dd>{h.ok === null ? '—' : h.ok ? '✓ ok' : '✗ not ok'}</dd>
      <dt>clock offset</dt>
      <dd>{h.clock_offset_ms === null ? '—' : `${h.clock_offset_ms} ms`}</dd>
      <dt>last minute</dt>
      <dd title={fmtLocal(h.minute_ms)}>{`${fmtUtcDateTime(h.minute_ms)} (${fmtDuration(h.age_ms)} ago)`}</dd>
    </dl>
  );
}

function SeriesCharts({ s }: { s: HealthSeries }) {
  if (s.points.length === 0) {
    return (
      <Box title="PER MINUTE">
        <EmptyState text={NO_HEALTH_ROWS} />
      </Box>
    );
  }
  const lag = (k: 'lag_p50_ms' | 'lag_p99_ms') =>
    s.points.flatMap((p) => (p[k] === null ? [] : [{ x: p.minute_ms, y: p[k] as number }]));
  return (
    <div className="grid2">
      <Box title="MSGS/S">
        <LineChart label="messages per second" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'msgs/s', tone: 'p0', points: s.points.map((p) => ({ x: p.minute_ms, y: p.msgs / 60 })) }]} />
      </Box>
      <Box title="LAG (MS)">
        <LineChart label="feed lag p50 and p99" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'p50', tone: 'p0', points: lag('lag_p50_ms') }, { label: 'p99', tone: 'p3', points: lag('lag_p99_ms') }]} />
      </Box>
      <Box title="DROPS AND RECONNECTS">
        <LineChart
          label="drops and reconnects per minute"
          xMin={s.from_ms}
          xMax={s.to_ms}
          markers={s.points.filter((p) => p.reconnects > 0).map((p) => p.minute_ms)}
          series={[
            { label: 'drops', tone: 'p3', points: s.points.map((p) => ({ x: p.minute_ms, y: p.drops })) },
            { label: 'reconnects', tone: 'p1', points: s.points.map((p) => ({ x: p.minute_ms, y: p.reconnects })) },
          ]}
        />
        <p className="dim">Grey bars mark minutes with reconnects.</p>
      </Box>
    </div>
  );
}

function IntegrityView({ i }: { i: Integrity }) {
  if (i.total === 0) return <EmptyState text={NO_INTEGRITY} />;
  return (
    <>
      <Table
        columns={[
          { key: 'kind', label: 'KIND', render: (r: { kind: string; count: number }) => r.kind },
          { key: 'count', label: 'COUNT', render: (r: { kind: string; count: number }) => r.count },
        ]}
        rows={i.by_kind}
        rowKey={(r) => r.kind}
      />
      <p className="quip">{`${i.total} events in ${i.msgs} messages: ${i.per_million === null ? '—' : formatDecimal(i.per_million)} per million.`}</p>
    </>
  );
}

export function Health() {
  const [sp, setSp] = useSearchParams();
  const asked = Number(sp.get('hours'));
  const hours = WINDOWS.includes(asked) ? asked : 24;
  const { healthTick } = useStream();
  const slow = useTicker(30_000);
  const now = useApi((s) => api.healthNow(s), [], [healthTick, slow]);
  const series = useApi((s) => api.healthSeries(hours, s), [hours], [healthTick, slow]);
  const integrity = useApi((s) => api.integrity(hours, s), [hours], [healthTick, slow]);
  return (
    <>
      <Box title="WINDOW">
        <div className="seg" role="group" aria-label="window">
          {WINDOWS.map((w) => (
            <button key={w} type="button" className={w === hours ? 'on' : undefined} aria-pressed={w === hours} onClick={() => setSp(new URLSearchParams({ hours: String(w) }))}>
              {w === 168 ? '7 D' : `${w} H`}
            </button>
          ))}
        </div>
      </Box>
      <Box title="CAPTURE">
        <Loadable state={now} what="capture status">
          {(d) => (d.health === null ? <EmptyState text={NO_HEALTH_ROWS} /> : <CaptureStatus h={d.health} />)}
        </Loadable>
      </Box>
      <Loadable state={series} what="the health series">
        {(d) => <SeriesCharts s={d} />}
      </Loadable>
      <Box title="INTEGRITY">
        <Loadable state={integrity} what="integrity">
          {(d) => <IntegrityView i={d} />}
        </Loadable>
      </Box>
    </>
  );
}
