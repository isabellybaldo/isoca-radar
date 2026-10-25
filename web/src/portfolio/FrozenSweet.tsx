import { useState } from 'react';
import { getJson } from '../api/client';
import type { CardsResponse } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Loadable } from '../components/Loadable';
import { Tag } from '../components/Tag';
import { ONE_LINE_CAVEAT, SWEET_EMPTY, type SweetDim, type SweetSide } from '../lib/sweet';
import { fmtUtcDateTime } from '../lib/time';
import { SweetBody } from '../pages/Sweet';

/** Frozen-site plan, task 3: the ALL-window Sweet spots from the committed file. One fetch (no refresh deps),
 *  no window switch, no status tag and no "x min ago": it never moves. */
export function FrozenSweet() {
  const st = useApi((s) => getJson<CardsResponse>('/frozen/cards-all.json', s), []);
  const [side, setSide] = useState<SweetSide>('no');
  const [dimAsked, setDim] = useState<SweetDim>('all');
  return (
    <Box title="SWEET SPOTS" help="sweet" hero actions={<Tag tone="plain">FROZEN · ALL</Tag>}>
      <p className="quip">{ONE_LINE_CAVEAT}</p>
      <Loadable state={st} what="sweet spots">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'SWEET');
          if (!c?.series.sweet) return <EmptyState text={SWEET_EMPTY} />;
          return (
            <SweetBody
              card={c}
              s={c.series.sweet}
              computed={`computed ${fmtUtcDateTime(c.computed_ms)}`}
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
