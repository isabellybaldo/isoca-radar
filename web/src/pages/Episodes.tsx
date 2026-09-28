import { Link, useSearchParams } from 'react-router';
import { api, type EpisodeQuery } from '../api/client';
import type { EpisodeSummary } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EpisodeStrip } from '../components/EpisodeStrip';
import { Fold } from '../components/Fold';
import { Help } from '../components/Help';
import { EmptyState } from '../components/EmptyState';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Tag } from '../components/Tag';
import { CENSORED_WORDS, FLAG_WORDS } from '../lib/flags';
import { kindTag } from '../lib/labels';
import { formatDecimal } from '../lib/money';
import { DAY_MS, fmtDuration, fmtLocal, fmtUtcDayMonth, fmtUtcHms, parseIsoDay } from '../lib/time';

export const NO_EPISODES = 'No episodes match. The filters are working; the market is not.';
const KINDS: [string, string][] = [
  ['', 'ALL'],
  ['no_basket', 'NO'],
  ['yes_basket', 'YES'],
  ['yes_basket_incomplete', 'YES?'],
  ['implication', 'IMPL'],
];
const FLAGS = ['suspect', 'stale', 'censored'];

/** URL → api query. Days are UTC; the URL's `to` day is inclusive, the api's `to` is exclusive. */
export function queryFromParams(sp: URLSearchParams): EpisodeQuery {
  const from = parseIsoDay(sp.get('from') ?? '');
  const to = parseIsoDay(sp.get('to') ?? '');
  return {
    kind: sp.get('kind') || undefined,
    from: from ?? undefined,
    to: to === null ? undefined : to + DAY_MS,
    net: sp.get('net') === '1',
    flags: (sp.get('flags') ?? '').split(',').filter((f) => f !== ''),
    cursor: sp.get('cursor'),
    limit: 50,
  };
}

function FlagTags({ e }: { e: EpisodeSummary }) {
  return (
    <>
      {e.suspect && <Tag tone="flag" title={FLAG_WORDS.suspect}>!</Tag>}
      {e.stale && <Tag tone="flag" title={FLAG_WORDS.stale}>S</Tag>}
      {e.censored && <Tag tone="flag" title={CENSORED_WORDS}>C</Tag>}
    </>
  );
}

const LEGEND = [
  FLAG_WORDS.stale,
  FLAG_WORDS.suspect,
  FLAG_WORDS.censor_left,
  FLAG_WORDS.censor_right,
  FLAG_WORDS.censor_gap,
  `YES? (kind): ${FLAG_WORDS.incomplete}`,
];

function FlagLegend() {
  return (
    <div className="legend-note" aria-label="flag legend">
      <ul className="plain dim">
        {LEGEND.map((w) => (
          <li key={w}>{w}</li>
        ))}
      </ul>
      <Help id="flags" />
    </div>
  );
}

const HEADS = ['OPENED', 'KIND', 'EVENT', 'S', 'NET', 'GROSS', 'Q*', 'DURATION', 'FLAGS'];

function EpisodeRow({ r }: { r: EpisodeSummary }) {
  const t = kindTag(r.kind);
  const title = r.event_title ?? r.id;
  return (
    <tr className="ep">
      <td className="num c-time" title={fmtLocal(r.opened_ms)}>{fmtUtcHms(r.opened_ms)}</td>
      <td className="c-kind"><Tag tone={t.tone}>{t.text}</Tag></td>
      <td className="c-event"><Link to={`/episodes/${encodeURIComponent(r.id)}`} title={title}>{title}</Link></td>
      <td className="num c-s">{`${r.s_legs}/${r.n_legs}`}</td>
      <td className="num c-net"><Money value={r.net} signed tone cents /></td>
      <td className="num c-gross"><Money value={r.gross} signed cents /></td>
      <td className="num c-q">{formatDecimal(r.q_star)}</td>
      <td className="num c-dur">{r.duration_ms === null ? 'open' : fmtDuration(r.duration_ms)}</td>
      <td className="c-flags"><FlagTags e={r} /></td>
    </tr>
  );
}

/** Redesign §4.1: one line per episode, grouped under its UTC day. */
function EpisodeTable({ rows }: { rows: EpisodeSummary[] }) {
  const groups: { day: string; rows: EpisodeSummary[] }[] = [];
  for (const r of rows) {
    const day = fmtUtcDayMonth(r.opened_ms);
    if (groups.at(-1)?.day !== day) groups.push({ day, rows: [] });
    groups[groups.length - 1].rows.push(r);
  }
  return (
    <div className="scroll">
      <table className="eptable">
        <thead>
          <tr>{HEADS.map((h) => <th key={h} scope="col">{h}</th>)}</tr>
        </thead>
        {groups.map((g) => (
          <tbody key={g.day}>
            <tr className="day"><th colSpan={HEADS.length} scope="colgroup">{g.day}</th></tr>
            {g.rows.map((r) => <EpisodeRow key={r.id} r={r} />)}
          </tbody>
        ))}
      </table>
    </div>
  );
}

export function Episodes() {
  const [sp, setSp] = useSearchParams();
  const key = sp.toString();
  const q = queryFromParams(sp);
  const page = useApi((s) => api.episodes(queryFromParams(sp), s), [key]);
  const update = (name: string, value: string | null) => {
    const next = new URLSearchParams(sp);
    if (value) next.set(name, value);
    else next.delete(name);
    if (name !== 'cursor') next.delete('cursor');
    setSp(next);
  };
  const toggleFlag = (f: string) => {
    const on = new Set(q.flags);
    if (on.has(f)) on.delete(f);
    else on.add(f);
    update('flags', FLAGS.filter((x) => on.has(x)).join(',') || null);
  };
  const kind = q.kind ?? '';
  return (
    <Box title="EPISODES" help="episodes" hero>
      <div className="filters">
        <div className="seg" role="group" aria-label="kind">
          {KINDS.map(([value, label]) => (
            <button key={label} type="button" className={kind === value ? 'on' : undefined} aria-pressed={kind === value} onClick={() => update('kind', value || null)}>
              {label}
            </button>
          ))}
        </div>
        <label>from <input type="date" value={sp.get('from') ?? ''} onChange={(e) => update('from', e.target.value || null)} /></label>
        <label>to <input type="date" value={sp.get('to') ?? ''} onChange={(e) => update('to', e.target.value || null)} /></label>
        <details className="menu">
          <summary>{`flags${(q.net ? 1 : 0) + (q.flags?.length ?? 0) > 0 ? ` (${(q.net ? 1 : 0) + (q.flags?.length ?? 0)})` : ''}`}</summary>
          <div className="menu-body">
            <label className="check">
              <input type="checkbox" checked={q.net ?? false} onChange={(e) => update('net', e.target.checked ? '1' : null)} />
              net-positive only
            </label>
            {FLAGS.map((f) => (
              <label key={f} className="check">
                <input type="checkbox" checked={q.flags?.includes(f) ?? false} onChange={() => toggleFlag(f)} />
                {f}
              </label>
            ))}
          </div>
        </details>
        <Help id="filters" />
      </div>
      <Loadable state={page} what="episodes">
        {(d) =>
          d.items.length === 0 ? (
            <EmptyState text={NO_EPISODES} />
          ) : (
            <>
              <EpisodeStrip rows={d.items} />
              <EpisodeTable rows={d.items} />
              <div className="pager">
                <button type="button" disabled={!q.cursor} onClick={() => update('cursor', null)}>NEWEST</button>
                <button type="button" disabled={!d.next_cursor} onClick={() => d.next_cursor && update('cursor', d.next_cursor)}>OLDER →</button>
              </div>
              <Fold id="episodes:flags" label="what the flags mean">
                <FlagLegend />
              </Fold>
            </>
          )
        }
      </Loadable>
    </Box>
  );
}
