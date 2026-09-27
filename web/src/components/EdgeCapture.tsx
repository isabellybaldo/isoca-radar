import { useState } from 'react';
import { Link } from 'react-router';
import { api } from '../api/client';
import type { CardBody, CardRow } from '../api/types';
import { useApi } from '../api/useApi';
import { ON_TABLE, UNDER_REVIEW, fmtValue, statusLabel, statusTone, updatedText, warmingText, withThinEdge } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';
import { formatMoney, formatMoneyCents, signOf } from '../lib/money';
import { Box } from './Box';
import { CardMeta, CardTable } from './CardView';
import { EmptyState } from './EmptyState';
import { Fold } from './Fold';
import { LatencyAxis } from './LatencyAxis';
import { Loadable } from './Loadable';
import { SplitBar } from './SplitBar';
import { Tag } from './Tag';
import { type Tile, Tiles } from './Tiles';
import { type Win, WindowSwitch } from './WindowSwitch';

export const EDGE_SUBTITLE = 'The difference is physics and other people.';
export const EDGE_EMPTY = 'No Edge capture yet. analyze has not finished a tick under these premises.';

const row = (c: CardBody, starts: string) => c.columns.find((r) => r.label.startsWith(starts));

function tile(r: CardRow | undefined): Tile[] {
  if (!r) return [];
  const s = signOf(r.ours);
  const money = r.unit === 'USD';
  return [{
    label: r.label,
    value: money ? formatMoneyCents(r.ours) : fmtValue(r.ours, r.unit),
    title: money && r.ours !== null ? formatMoney(r.ours) : undefined,
    tone: s < 0 ? 'neg' : undefined,
  }];
}

function EdgeBody({ card, stale }: { card: CardBody; stale: boolean }) {
  const nowMs = useNow(30_000);
  return (
    <>
      {card.status === 'warming' && <p className="dim">{warmingText(card)}</p>}
      {card.status === 'under_review' && <p className="dim">{UNDER_REVIEW}</p>}
      <Tiles items={[...tile(row(card, ON_TABLE)), ...tile(row(card, 'captured at')), ...tile(row(card, 'capture rate')), ...tile(row(card, 'measured latency'))]} />
      {card.series.decay && card.series.decay.length > 0 && <LatencyAxis points={card.series.decay} measured={card.series.measured ?? null} />}
      <SplitBar
        label="on the table, by side"
        parts={[
          { name: 'NO cashable', value: row(card, 'NO side')?.ours ?? null, tone: 'p0' },
          { name: 'YES locked', value: row(card, 'YES side')?.ours ?? null, tone: 'p3' },
        ]}
      />
      <Fold id="edge:numbers" label="all numbers">
        <CardTable card={card} stale={stale} />
      </Fold>
      {card.episode_link && (
        <p>
          <Link to={`/episodes/${encodeURIComponent(card.episode_link)}`}>biggest opportunity →</Link>
        </p>
      )}
      <Fold id="edge:how" label="how it's computed">
        <CardMeta card={card} nowMs={nowMs} />
      </Fold>
    </>
  );
}

/** Redesign §4.2: Edge capture as tiles, the latency axis and the NO/YES split; details folded. */
export function EdgeCapture() {
  const [win, setWin] = useState<Win>('24h');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  const meta = useApi((s) => api.cardsMeta(s), [], [slow]);
  const now = useNow(30_000);
  const stale = meta.data !== undefined && updatedText(meta.data.tick?.last_tick_ms ?? null, now).stale;
  const edge = st.data?.cards.find((x) => x.card === 'EDGE');
  return (
    <Box
      title="EDGE CAPTURE"
      help="edge"
      hero
      actions={
        <>
          <WindowSwitch win={win} onChange={setWin} />
          {edge && <Tag tone={statusTone(edge.status)}>{statusLabel(edge.status)}</Tag>}
        </>
      }
    >
      <p className="quip">{EDGE_SUBTITLE}</p>
      <Loadable state={st} what="edge capture">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'EDGE');
          return c ? <EdgeBody card={withThinEdge(c)} stale={stale} /> : <EmptyState text={EDGE_EMPTY} />;
        }}
      </Loadable>
    </Box>
  );
}
