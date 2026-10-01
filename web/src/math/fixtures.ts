// Test helper: the public snapshot 7b-A writes (spec 7b §5.1), built from the 7a ladder fixture minus the
// fields that never ship (account ids, the host), plus today's counts.
import { ladderFixture } from '../test/fixtures';
import type { PublicSnapshot } from './snapshot';

export function snapshotFixture(o: Partial<PublicSnapshot> = {}, writtenMs: number = Date.now()): PublicSnapshot {
  const l = ladderFixture();
  return {
    v: 1,
    written_ms: writtenMs,
    ladder: {
      epoch: l.epoch,
      start_ms: l.start_ms,
      as_of_ms: l.as_of_ms,
      break_even: l.break_even,
      measured: l.measured === null ? null : { latency_ms: l.measured.latency_ms },
      markets_status: l.markets_status,
      rungs: l.rungs.map((r) => ({
        scenario: r.scenario, latency_ms: r.latency_ms, extra: r.extra, n_exec: r.n_exec, n_leg_risk: r.n_leg_risk, n_missed: r.n_missed,
        gains: r.gains, leg_risk: r.leg_risk, settle: r.settle, unrealized: r.unrealized, net: r.net,
      })),
      // 7b-A: NO side, fee 0, the LADDER scenarios, `all` and `category` only.
      markets: l.markets.flatMap((r) =>
        r.side === 'no' && r.fee_bips === 0 && (r.dimension === 'all' || r.dimension === 'category')
          ? [{
              side: 'no' as const, scenario: r.scenario, fee_bips: 0 as const, latency_ms: r.latency_ms, dimension: r.dimension, bucket: r.bucket,
              label: r.label, n_exec: r.n_exec, clusters: r.clusters, mean: r.mean, lo: r.lo, hi: r.hi,
            }]
          : [],
      ),
    },
    today: {
      day_start_ms: Date.UTC(2026, 8, 30),
      arb: 15,
      no: 12,
      yes: 3,
      edge_budget100: '41.37',
      // 7b-A writes 24 entries, hours 0-23 UTC
      hourly: Array.from({ length: 24 }, (_, hour) => ({ hour, arb: [2, 0, 5, 8][hour] ?? 0 })),
    },
    ...o,
  };
}
