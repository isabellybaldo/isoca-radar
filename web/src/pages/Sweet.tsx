import { useState } from 'react';
import { api } from '../api/client';
import type { CardBody, SweetSeries } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Fold } from '../components/Fold';
import { Help } from '../components/Help';
import { Loadable } from '../components/Loadable';
import { SweetLadderGrid } from '../components/SweetLadderGrid';
import { Tag } from '../components/Tag';
import { Tiles } from '../components/Tiles';
import { type Win, WindowSwitch } from '../components/WindowSwitch';
import { computedText, shortHash, statusLabel, statusTone, warmingText } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';
import { formatMoney, formatPercent } from '../lib/money';
import {
  COLO_TITLE, DIMS, FEW_TRADES, HEADLINE_SCENARIO, LABEL_TAG, LABEL_TEXT, NOTHING_PAYS, ONE_LINE_CAVEAT, SWEET_CAVEAT,
  SWEET_EMPTY, type SweetDim, type SweetSide, bucketText, candidateCount, feeLine, footnote, grid, gridNote, interval, nothingPays,
  recomputingText,
} from '../lib/sweet';
import { fmtLocal, fmtUtcDateTime } from '../lib/time';

const scenarioTitle = (name: string) => (name === 'colo-idle' ? `${COLO_TITLE} · ${name}` : name);

function Chips<T extends string>({ label, options, value, onChange }: {
  label: string; options: [T, string][]; value: T; onChange: (v: T) => void;
}) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={v} type="button" className={v === value ? 'on' : undefined} aria-pressed={v === value} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

/** The old Sweet spots table: every scenario side by side (redesign §5.1 "all numbers"). */
function SweetGrid({ s, side, dim }: { s: SweetSeries; side: SweetSide; dim: SweetDim }) {
  const g = grid(s, side, dim);
  return (
    <div className="scroll">
      <table>
        <thead>
          <tr>
            <th scope="col">{dim === 'all' ? '' : dim.toUpperCase()}</th>
            {g.scenarios.map((sc) => (
              <th key={sc.name} scope="col">
                {scenarioTitle(sc.name)}
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
                const alt = r.captured_leftover0 !== r.captured ? `leftovers at 0, mean: ${formatMoney(r.mean_leftover0, { signed: true })}` : undefined;
                return (
                  <td key={sc.name} className={dimmed ? 'dim num' : 'num'} title={alt}>
                    <Tag tone={r.label === 'candidate' ? 'no' : r.label === 'negative' || r.label === 'dead_on_paper' ? 'flag' : 'plain'} title={LABEL_TEXT[r.label]}>
                      {LABEL_TAG[r.label]}
                    </Tag>{' '}
                    {interval(r)}
                    <br />
                    <span className="dim">n {r.n_exec} · C {r.clusters}</span>
                    {r.side === 'yes' && r.annualized !== null && <span className="dim"> · {formatPercent(r.annualized)}/yr</span>}
                    {r.few_trades && (<><br /><span className="dim">{FEW_TRADES}</span></>)}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** M1: side/dim live in Sweet() so a window switch (which replaces this card by identity in
 *  Loadable's render) never resets the chips the owner just picked. Spec 7a §8: the grid replaces
 *  the per-scenario chart and its chips; every scenario's numbers stay in "all numbers". */
function SweetBody({ card, s, side, setSide, dimAsked, setDim }: {
  card: CardBody; s: SweetSeries;
  side: SweetSide; setSide: (v: SweetSide) => void;
  dimAsked: SweetDim; setDim: (v: SweetDim) => void;
}) {
  const nowMs = useNow(30_000);
  const dims = DIMS.filter(([d]) => d !== 'category' || s.category_available);
  const dim = dimAsked === 'category' && !s.category_available ? 'all' : dimAsked;
  const extraNotes = card.notes.filter((n) => n !== SWEET_CAVEAT && n !== footnote(s.m));
  return (
    <>
      <Tiles
        items={[
          { label: 'NO candidates', value: String(candidateCount(s, 'no')) },
          { label: 'YES candidates', value: String(candidateCount(s, 'yes')) },
          { label: 'rows tested', value: String(s.m) },
          { label: 'headline scenario', value: HEADLINE_SCENARIO },
        ]}
      />
      {feeLine(s) && <p className="dim">{feeLine(s)}</p>}
      {nothingPays(s) && <p className="quip">{NOTHING_PAYS}</p>}
      <div className="filters">
        <Chips label="side" options={[['no', 'NO'], ['yes', 'YES']]} value={side} onChange={setSide} />
        {side === 'no' ? <Help id="sweetNo" /> : <Help id="sweetYes" />}
        <Chips label="dimension" options={dims} value={dim} onChange={setDim} />
      </div>
      <SweetLadderGrid rows={s.rows} side={side} dim={dim} label={`${side.toUpperCase()} · ${dim.toUpperCase()}`} />
      <p className="dim">{gridNote(s)}</p>
      <Fold id="sweet:numbers" label="all numbers">
        <SweetGrid s={s} side={side} dim={dim} />
      </Fold>
      <Fold id="sweet:how" label="how it's computed">
        <p>{SWEET_CAVEAT}</p>
        <p className="dim">
          {footnote(s.m)} z = {s.z}. <Help id="sweetBonferroni" /> <Help id="sweetThin" /> <Help id="sweetClusters" />
        </p>
        {extraNotes.map((n) => (
          <p key={n} className="note">{n}</p>
        ))}
        <p className="meta">
          <span title={card.hash}>#{shortHash(card.hash)}</span> ·{' '}
          <span title={fmtLocal(card.computed_ms)}>
            {computedText(card.computed_ms, nowMs)} ({fmtUtcDateTime(card.computed_ms)})
          </span>
        </p>
      </Fold>
    </>
  );
}

/** Redesign §5.1: Sweet spots on its own page, 30D by default. */
export function Sweet() {
  const [win, setWin] = useState<Win>('30d');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  const card = st.data?.cards.find((c) => c.card === 'SWEET');
  const [side, setSide] = useState<SweetSide>('no');
  const [dimAsked, setDim] = useState<SweetDim>('all');
  return (
    <Box
      title="SWEET SPOTS"
      help="sweet"
      hero
      actions={
        <>
          <WindowSwitch win={win} onChange={setWin} label="sweet spots window" />
          {card && <Tag tone={statusTone(card.status)}>{statusLabel(card.status)}</Tag>}
        </>
      }
    >
      <p className="quip">{ONE_LINE_CAVEAT}</p>
      <Loadable state={st} what="sweet spots">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'SWEET');
          if (!c?.series.sweet) return <EmptyState text={SWEET_EMPTY} />;
          if (c.status === 'under_review') return <EmptyState text={recomputingText(c.computed_ms)} />;
          if (c.status === 'warming') return <EmptyState text={warmingText(c)} />;
          return (
            <SweetBody
              card={c}
              s={c.series.sweet}
              side={side}
              setSide={setSide}
              dimAsked={dimAsked}
              setDim={setDim}
            />
          );
        }}
      </Loadable>
    </Box>
  );
}
