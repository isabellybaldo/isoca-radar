import { useState } from 'react';
import { api } from '../api/client';
import { useApi } from '../api/useApi';
import { updatedText } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';
import { Box } from './Box';
import { CardView } from './CardView';
import { EmptyState } from './EmptyState';
import { Loadable } from './Loadable';
import { type Win, WindowSwitch } from './WindowSwitch';

export const EDGE_SUBTITLE = 'The difference is physics and other people.';
export const EDGE_EMPTY = 'No Edge capture yet. analyze has not finished a tick under these premises.';

/**
 * Spec §6.2: the Edge capture box at the top of the Paper page — labelled as an upper bound that
 * assumes first-in-line execution, with the CLOB queue ignored (never "fantasy"). 24H by default
 * (the page's equity chart is 24 h). F6 (controller ruling): WINS/Win/the switch come from
 * WindowSwitch.tsx (Task 12), never redeclared here.
 */
export function EdgeCapture() {
  const [win, setWin] = useState<Win>('24h');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  // Stale results dim the rows the same way the Papers page does (until meta arrives, no dimming).
  const meta = useApi((s) => api.cardsMeta(s), [], [slow]);
  const now = useNow(30_000);
  const stale = meta.data !== undefined && updatedText(meta.data.tick?.last_tick_ms ?? null, now).stale;
  return (
    <Box title="EDGE CAPTURE" help="edge">
      <p className="quip">{EDGE_SUBTITLE}</p>
      <WindowSwitch win={win} onChange={setWin} />
      <Loadable state={st} what="edge capture">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'EDGE');
          return c ? <CardView card={c} stale={stale} /> : <EmptyState text={EDGE_EMPTY} />;
        }}
      </Loadable>
    </Box>
  );
}
