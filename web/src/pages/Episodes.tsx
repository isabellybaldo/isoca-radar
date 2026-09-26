import { Link, useSearchParams } from 'react-router';
import { api, type EpisodeQuery } from '../api/client';
import type { EpisodeSummary } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { kindTag } from '../lib/labels';
import { formatDecimal } from '../lib/money';
import { DAY_MS, fmtDuration, fmtLocal, fmtUtcDateTime, parseIsoDay } from '../lib/time';

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
      {e.suspect && <Tag tone="flag">SUSPECT</Tag>}
      {e.stale && <Tag tone="flag">STALE</Tag>}
      {e.censored && <Tag tone="flag">CENSORED</Tag>}
    </>
  );
}

const columns: Column<EpisodeSummary>[] = [
  { key: 'opened', label: 'OPENED', render: (r) => <span title={fmtLocal(r.opened_ms)}>{fmtUtcDateTime(r.opened_ms)}</span> },
  {
    key: 'kind',
    label: 'KIND',
    render: (r) => {
      const t = kindTag(r.kind);
      return <Tag tone={t.tone}>{t.text}</Tag>;
    },
  },
  { key: 'event', label: 'EVENT', render: (r) => <Link to={`/episodes/${encodeURIComponent(r.id)}`}>{r.event_title ?? r.id}</Link> },
  { key: 's', label: 'S', render: (r) => `${r.s_legs}/${r.n_legs}` },
  { key: 'net', label: 'NET', render: (r) => <Money value={r.net} signed tone /> },
  { key: 'gross', label: 'GROSS', render: (r) => <Money value={r.gross} signed /> },
  { key: 'q', label: 'Q*', render: (r) => formatDecimal(r.q_star) },
  { key: 'dur', label: 'DURATION', render: (r) => (r.duration_ms === null ? 'open' : fmtDuration(r.duration_ms)) },
  { key: 'flags', label: 'FLAGS', render: (r) => <FlagTags e={r} /> },
];

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
    <>
      <Box title="FILTERS">
        <div className="filters">
          <div className="seg" role="group" aria-label="kind">
            {KINDS.map(([value, label]) => (
              <button key={label} type="button" className={kind === value ? 'on' : undefined} aria-pressed={kind === value} onClick={() => update('kind', value || null)}>
                {label}
              </button>
            ))}
          </div>
          <label>
            from <input type="date" value={sp.get('from') ?? ''} onChange={(e) => update('from', e.target.value || null)} />
          </label>
          <label>
            to <input type="date" value={sp.get('to') ?? ''} onChange={(e) => update('to', e.target.value || null)} />
          </label>
          <label>
            <input type="checkbox" checked={q.net ?? false} onChange={(e) => update('net', e.target.checked ? '1' : null)} />
            net-positive only
          </label>
          {FLAGS.map((f) => (
            <label key={f}>
              <input type="checkbox" checked={q.flags?.includes(f) ?? false} onChange={() => toggleFlag(f)} />
              {f}
            </label>
          ))}
        </div>
        <p className="dim">UTC days. Seven days at a time.</p>
      </Box>
      <Box title="EPISODES">
        <Loadable state={page} what="episodes">
          {(d) =>
            d.items.length === 0 ? (
              <EmptyState text={NO_EPISODES} />
            ) : (
              <>
                <Table columns={columns} rows={d.items} rowKey={(r) => r.id} />
                <div className="pager">
                  <button type="button" disabled={!q.cursor} onClick={() => update('cursor', null)}>
                    NEWEST
                  </button>
                  <button type="button" disabled={!d.next_cursor} onClick={() => d.next_cursor && update('cursor', d.next_cursor)}>
                    OLDER →
                  </button>
                </div>
              </>
            )
          }
        </Loadable>
      </Box>
    </>
  );
}
