import { useState } from 'react';
import { api } from '../api/client';
import type { SweetSeries } from '../api/types';
import { useApi } from '../api/useApi';
import { useTicker } from '../lib/hooks';
import { formatDecimal } from '../lib/money';
import {
  COLO_TITLE, DIMS, FEW_TRADES, LABEL_TAG, LABEL_TEXT, NOTHING_PAYS, SWEET_CAVEAT, SWEET_EMPTY,
  type SweetDim, type SweetSide, bucketText, footnote, grid, headline, interval, nothingPays,
} from '../lib/sweet';
import { Box } from './Box';
import { EmptyState } from './EmptyState';
import { Help } from './Help';
import { Loadable } from './Loadable';
import { Tag } from './Tag';
import { type Win, WindowSwitch } from './WindowSwitch';

/** Phase 5d spec §8: below Edge capture on the Paper page; its own window switch, 30D by default. */
export function SweetSpots() {
  const [win, setWin] = useState<Win>('30d');
  const [side, setSide] = useState<SweetSide>('no');
  const [dim, setDim] = useState<SweetDim>('all');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  return (
    <Box title="SWEET SPOTS" help="sweet">
      <p className="quip">{SWEET_CAVEAT}</p>
      <WindowSwitch win={win} onChange={setWin} label="sweet spots window" />
      <Loadable state={st} what="sweet spots">
        {(d) => {
          const s = d.cards.find((c) => c.card === 'SWEET')?.series.sweet;
          return s ? <SweetBody s={s} side={side} dim={dim} onSide={setSide} onDim={setDim} /> : <EmptyState text={SWEET_EMPTY} />;
        }}
      </Loadable>
    </Box>
  );
}

function SweetBody({ s, side, dim, onSide, onDim }: {
  s: SweetSeries; side: SweetSide; dim: SweetDim; onSide: (v: SweetSide) => void; onDim: (v: SweetDim) => void;
}) {
  const dims = DIMS.filter(([d]) => d !== 'category' || s.category_available);
  const g = grid(s, side, dim === 'category' && !s.category_available ? 'all' : dim);
  return (
    <>
      <p>{headline(s, 'no')}</p>
      <p>{headline(s, 'yes')}</p>
      {nothingPays(s) && <p className="quip">{NOTHING_PAYS}</p>}
      <div className="seg" role="group" aria-label="side">
        {(['no', 'yes'] as const).map((v) => (
          <button key={v} type="button" className={v === side ? 'on' : undefined} aria-pressed={v === side} onClick={() => onSide(v)}>
            {v.toUpperCase()}
          </button>
        ))}
        {side === 'yes' && <Help id="sweetYes" />}
      </div>
      <div className="seg" role="group" aria-label="dimension">
        {dims.map(([d, label]) => (
          <button key={d} type="button" className={d === dim ? 'on' : undefined} aria-pressed={d === dim} onClick={() => onDim(d)}>
            {label}
          </button>
        ))}
      </div>
      <div className="scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">{dim === 'all' ? '' : dim.toUpperCase()}</th>
              {g.scenarios.map((sc) => (
                <th key={sc.name} scope="col">
                  {sc.name === 'colo-idle' ? `${COLO_TITLE} · ` : ''}
                  {sc.name}
                  {sc.latency_ms === null ? '' : ` · ${sc.latency_ms} ms`}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {g.buckets.map((b) => (
              <tr key={b}>
                <th scope="row">{bucketText(dim, b)}</th>
                {g.scenarios.map((sc) => {
                  const r = g.cell(b, sc.name);
                  if (!r) return <td key={sc.name}>—</td>;
                  const dimmed = r.label === 'thin' || r.label === 'not_tested';
                  const alt = r.captured_leftover0 !== r.captured ? `leftovers at 0: ${formatDecimal(r.captured_leftover0)} total` : undefined;
                  return (
                    <td key={sc.name} className={dimmed ? 'dim' : undefined} title={alt}>
                      <Tag tone={r.label === 'candidate' ? 'no' : r.label === 'negative' || r.label === 'dead_on_paper' ? 'flag' : 'plain'} title={LABEL_TEXT[r.label]}>
                        {LABEL_TAG[r.label]}
                      </Tag>{' '}
                      {interval(r)}
                      <br />
                      <span className="dim">n {r.n_exec} · C {r.clusters}</span>
                      {r.side === 'yes' && r.annualized !== null && <span className="dim"> · {formatDecimal(r.annualized)}/yr</span>}
                      {r.few_trades && (
                        <>
                          <br />
                          <span className="dim">{FEW_TRADES}</span>
                        </>
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="dim">
        {footnote(s.m)} z = {s.z}. <Help id="sweetBonferroni" /> <Help id="sweetThin" /> <Help id="sweetClusters" />
      </p>
    </>
  );
}
