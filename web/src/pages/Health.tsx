import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import { useStream } from '../api/StreamContext';
import type { HealthNow, HealthSeries, Integrity } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { CountBars } from '../components/CountBars';
import { EmptyState } from '../components/EmptyState';
import { Fold } from '../components/Fold';
import { LineChart } from '../components/LineChart';
import { Loadable } from '../components/Loadable';
import { PageHead } from '../components/PageHead';
import { SubHead } from '../components/SubHead';
import { Tag } from '../components/Tag';
import { Tiles } from '../components/Tiles';
import { useTicker } from '../lib/hooks';
import { formatDecimal, groupThousands } from '../lib/money';
import { fmtDuration, fmtLocal, fmtUtcDateTime } from '../lib/time';

export const NO_HEALTH_ROWS = 'No health rows in this window. The scanner may be asleep.';
export const NO_INTEGRITY = 'No integrity events in this window. Either the books agree, or nobody checked.';
const WINDOWS = [6, 24, 168];

function CaptureStatus({ h }: { h: HealthNow }) {
  return (
    <>
      <Tiles
        items={[
          { label: 'capture', value: h.capture_paused ? <Tag tone="flag">PAUSED</Tag> : 'recording ✓' },
          { label: 'shards', value: `${h.shards - h.parked_shards}/${h.shards}` },
          { label: 'tokens', value: groupThousands(String(h.tokens)) },
          { label: 'last minute', value: `${fmtDuration(h.age_ms)} ago`, title: fmtLocal(h.minute_ms) },
        ]}
      />
      <Fold id="health:capture" label="all numbers">
        <dl className="kv">
          <dt>/ok</dt>
          <dd>{h.ok === null ? '—' : h.ok ? '✓ ok' : '✗ not ok'}</dd>
          <dt>clock offset</dt>
          <dd>{h.clock_offset_ms === null ? '—' : `${h.clock_offset_ms} ms`}</dd>
          <dt>last minute</dt>
          <dd title={fmtLocal(h.minute_ms)}>{fmtUtcDateTime(h.minute_ms)}</dd>
        </dl>
      </Fold>
    </>
  );
}

function SeriesCharts({ s }: { s: HealthSeries }) {
  if (s.points.length === 0) {
    return (
      <Box title="PER MINUTE" help="perMinute">
        <EmptyState text={NO_HEALTH_ROWS} />
      </Box>
    );
  }
  const lag = (k: 'lag_p50_ms' | 'lag_p99_ms') =>
    s.points.flatMap((p) => (p[k] === null ? [] : [{ x: p.minute_ms, y: p[k] as number }]));
  return (
    <Box title="PER MINUTE" help="perMinute" hero>
      <div className="grid3">
        <div>
          <SubHead help="msgs">msgs/s</SubHead>
          <LineChart label="messages per second" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'msgs/s', tone: 'p0', points: s.points.map((p) => ({ x: p.minute_ms, y: p.msgs / 60 })) }]} />
        </div>
        <div>
          <SubHead help="lag">lag (ms)</SubHead>
          <LineChart label="feed lag p50 and p99" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'p50', tone: 'p0', points: lag('lag_p50_ms') }, { label: 'p99', tone: 'p3', points: lag('lag_p99_ms') }]} />
        </div>
        <div>
          <SubHead help="drops">drops and reconnects</SubHead>
          <LineChart
            label="drops and reconnects per minute"
            xMin={s.from_ms}
            xMax={s.to_ms}
            markers={s.points.filter((p) => p.reconnects > 0).map((p) => p.minute_ms)}
            markerLabel="minutes with reconnects"
            series={[
              { label: 'drops', tone: 'p3', points: s.points.map((p) => ({ x: p.minute_ms, y: p.drops })) },
              { label: 'reconnects', tone: 'p1', points: s.points.map((p) => ({ x: p.minute_ms, y: p.reconnects })) },
            ]}
          />
        </div>
      </div>
    </Box>
  );
}

function IntegrityView({ i }: { i: Integrity }) {
  if (i.total === 0) return <EmptyState text={NO_INTEGRITY} />;
  const sorted = [...i.by_kind].sort((a, b) => b.count - a.count);
  return (
    <>
      <CountBars label="integrity by kind" items={sorted.map((k) => ({ label: k.kind, count: k.count }))} />
      <p className="quip">{`${groupThousands(String(i.total))} events in ${groupThousands(String(i.msgs))} messages: ${i.per_million === null ? '—' : formatDecimal(i.per_million)} per million.`}</p>
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
      <PageHead title="HEALTH" help="window">
        <div className="seg" role="group" aria-label="window">
          {WINDOWS.map((w) => (
            <button key={w} type="button" className={w === hours ? 'on' : undefined} aria-pressed={w === hours} onClick={() => setSp(new URLSearchParams({ hours: String(w) }))}>
              {w === 168 ? '7 D' : `${w} H`}
            </button>
          ))}
        </div>
      </PageHead>
      <Box title="CAPTURE" help="capture">
        <Loadable state={now} what="capture status">
          {(d) => (d.health === null ? <EmptyState text={NO_HEALTH_ROWS} /> : <CaptureStatus h={d.health} />)}
        </Loadable>
      </Box>
      <Loadable state={series} what="the health series">
        {(d) => <SeriesCharts s={d} />}
      </Loadable>
      <Box title="INTEGRITY" help="integrity">
        <Loadable state={integrity} what="integrity">
          {(d) => <IntegrityView i={d} />}
        </Loadable>
      </Box>
    </>
  );
}
