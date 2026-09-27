import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import type { CardsResponse, PaperEntry } from '../api/types';
import { type ApiState, useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { CardView } from '../components/CardView';
import { EmptyState } from '../components/EmptyState';
import { Loadable } from '../components/Loadable';
import { Tag } from '../components/Tag';
import { isWin, WINS, type Win, WindowSwitch } from '../components/WindowSwitch';
import { updatedText } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';

// F6 (controller ruling): WINS/Win/the switch live in components/WindowSwitch.tsx, shared with
// Task 13's EdgeCapture; re-exported here so `WINS` stays available where the brief names it.
export { WINS };
export const NO_CARDS = 'No cards yet. analyze has not finished a tick under these premises.';
export const PAPERS_SUBTITLE = 'Their number, their method on our data, our method. Same market, different weeks.';

function PaperSection({ paper, cards, stale }: { paper: PaperEntry; cards: ApiState<CardsResponse>; stale: boolean }) {
  const tag =
    paper.status === 'live' ? <Tag tone="no">LIVE</Tag> : paper.status === 'coming_later' ? <Tag>COMING LATER</Tag> : <Tag tone="flag">DROPPED</Tag>;
  return (
    <Box title={paper.cite} help={paper.status === 'live' ? 'gebele' : 'otherPapers'}>
      {tag}
      {paper.status !== 'live' ? (
        <p className="quip">{paper.note ?? ''}</p>
      ) : (
        <Loadable state={cards} what="the cards">
          {(d) => {
            const mine = d.cards.filter((c) => c.paper === paper.id);
            return mine.length === 0 ? (
              <EmptyState text={NO_CARDS} />
            ) : (
              <div className="cardgrid">
                {mine.map((c) => (
                  <CardView key={c.card} card={c} stale={stale} />
                ))}
              </div>
            );
          }}
        </Loadable>
      )}
    </Box>
  );
}

/** Spec §8.2: a window switch (7D by default, in the URL), one section per paper, Not replicated. */
export function Papers() {
  const [sp, setSp] = useSearchParams();
  const asked = sp.get('win');
  const win: Win = isWin(asked) ? asked : '7d';
  const slow = useTicker(60_000);
  const cards = useApi((s) => api.cards(win, s), [win], [slow]);
  const meta = useApi((s) => api.cardsMeta(s), [], [slow]);
  const now = useNow(30_000);
  const upd = updatedText(meta.data?.tick?.last_tick_ms ?? null, now);
  return (
    <>
      <Box title="PAPERS" help="papers">
        <p className="quip">{PAPERS_SUBTITLE}</p>
        <WindowSwitch win={win} onChange={(w) => setSp(new URLSearchParams({ win: w }))} />
        <p className={upd.stale ? 'dim stale' : 'dim'}>{upd.text}</p>
        {meta.data?.last_error && <p className="error">last tick failed: {meta.data.last_error.text}</p>}
      </Box>
      <Loadable state={meta} what="the papers">
        {(m) => (
          <>
            {m.papers.map((p) => (
              <PaperSection key={p.id} paper={p} cards={cards} stale={upd.stale} />
            ))}
            <Box title="NOT REPLICATED" help="notReplicated">
              <ul className="plain">
                {m.not_replicated.map((n) => (
                  <li key={n.item}>
                    <b>{n.item}.</b> {n.why}
                  </li>
                ))}
              </ul>
            </Box>
          </>
        )}
      </Loadable>
    </>
  );
}
