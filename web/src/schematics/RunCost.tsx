import { PLOT_W as W } from './geo';

const H = 226;
/** The rail every stage hangs on, and where the words start. */
const RAIL = 14;
const TEXT_X = 34;
const NODE = 10;
/** The feed's band is BAND wide at 123 GB/day; after zstd it narrows to 33/123 of it. */
const BAND = 12;
const ZSTD = (BAND * 33) / 123;
/** The dashed outline of what sat on disk: raw capture down to the archive and its limits. */
const DISK_TOP = 78;
const DISK_BOTTOM = 192;

/** One stage: a square on the rail (salmon: it used CPU; teal: it held bytes) and its line of text, one text node
 *  so the label reads as one string. */
function Stage({ y, kind, name, rest }: { y: number; kind: 'cpu' | 'disk' | 'feed'; name: string; rest?: string }) {
  return (
    <>
      <rect className={`rc-node ${kind}`} x={RAIL - NODE / 2} y={y - NODE + 1} width={NODE} height={NODE} />
      <text className="rc-text rc-name" x={TEXT_X} y={y}>{`${name}${rest ?? ''}`}</text>
    </>
  );
}

/** A flow between two stages: a band as wide as the bytes, or a thin line where no rate is claimed. */
function Flow({ y1, y2, w }: { y1: number; y2: number; w: number }) {
  return <rect className="rc-flow" x={RAIL - w / 2} y={y1} width={w} height={y2 - y1} />;
}

/**
 * Frozen-site plan, task 4: what running the scanner all day took, top to bottom: the Polymarket feed, the
 * scanner, raw capture, compact, the Parquet archive, and what is left now it is off. Every number is a fixed
 * string checked against its source (spikes/probe/RESULTS.md Q7c, PROJECT.md "Where it runs", the 2026-10-19
 * CPU-limit episode, the 2026-10-25 backup); the two bands are drawn to their ratio (123 to 33 GB/day), nothing
 * else is to scale. Words are SVG text in a viewBox that keeps its shape, so they stay beside what they name.
 */
export function RunCost() {
  return (
    <figure className="runcost">
      <svg
        className="rc-svg"
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label="The Polymarket feed brought in 123 GB a day, 1.4 MB a second, all day. The scanner used about 65 % of a core all day and wrote 33 GB a day of zstd raw capture. Compact used 2 cores while it turned raw hours into a Parquet archive. 24 hours of raw, up to 30 GB, and an archive of up to 40 GB were kept on disk. 28 GB of data is left behind."
      >
        <Stage y={14} kind="feed" name="Polymarket feed" />
        <Flow y1={15} y2={44} w={BAND} />
        <text className="rc-text" x={TEXT_X} y={33}>123 GB/day in · 1.4 MB/s, all day</text>
        <Stage y={53} kind="cpu" name="scanner" rest=" ≈ 65 % of a core, all day" />
        <Flow y1={54} y2={95} w={ZSTD} />
        <text className="rc-text" x={TEXT_X} y={72}>33 GB/day after zstd</text>
        <rect className="rc-disk" x={4} y={DISK_TOP} width={W - 8} height={DISK_BOTTOM - DISK_TOP} />
        <Stage y={104} kind="disk" name="raw capture" />
        <Flow y1={105} y2={130} w={1} />
        <Stage y={139} kind="cpu" name="compact" rest=": 2 cores while compacting" />
        <Flow y1={140} y2={165} w={1} />
        <Stage y={174} kind="disk" name="Parquet archive" />
        <text className="rc-text rc-dim" x={TEXT_X} y={188}>kept: 24 h raw ≤ 30 GB · archive ≤ 40 GB</text>
        <line className="rc-off" x1={RAIL} x2={RAIL} y1={DISK_BOTTOM} y2={208} />
        <rect className="rc-node gone" x={RAIL - NODE / 2} y={211} width={NODE} height={NODE} />
        <text className="rc-text" x={TEXT_X} y={220}>left behind: 28 GB of data</text>
      </svg>
      <ul className="plain rc-legend" aria-label="legend">
        <li><span className="rc-sw cpu" aria-hidden="true" />used CPU</li>
        <li><span className="rc-sw disk" aria-hidden="true" />held bytes</li>
        <li><span className="rc-sw flow" aria-hidden="true" />bytes a day, to scale</li>
      </ul>
    </figure>
  );
}
