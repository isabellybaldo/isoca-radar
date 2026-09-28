import { Link } from 'react-router';
import { api } from '../api/client';
import type { SweetSeries } from '../api/types';
import { useApi } from '../api/useApi';
import { warmingText } from '../lib/cards';
import { useTicker } from '../lib/hooks';
import { SWEET_EMPTY, bestCandidate, candidateCount, nothingPays } from '../lib/sweet';
import { Box } from './Box';
import { EmptyState } from './EmptyState';
import { IntervalChart } from './IntervalChart';
import { Loadable } from './Loadable';

export const SWEET_CARD_TITLE = 'SWEET SPOTS · LAST 30 DAYS';
export const NOTHING_YET = 'Nothing pays on paper yet.';
export const RECOMPUTING = 'Recomputing…';
export const TOO_FEW = 'Too few trades to say anything yet.';

function Candidates({ s }: { s: SweetSeries }) {
  const best = bestCandidate(s);
  if (best === null) return <EmptyState text={nothingPays(s) ? NOTHING_YET : TOO_FEW} />;
  return (
    <>
      <p className="quip">{`NO ${candidateCount(s, 'no')} · YES ${candidateCount(s, 'yes')} candidates`}</p>
      <IntervalChart rows={[best]} label="best candidate" />
    </>
  );
}

/** Redesign §5.2: Sweet spots in one glance on Radar; the title opens /sweet. */
export function SweetCard() {
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards('30d', s), [], [slow]);
  return (
    <Box title={<Link to="/sweet">{SWEET_CARD_TITLE}</Link>} help="sweet">
      <Loadable state={st} what="sweet spots">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'SWEET');
          if (!c?.series.sweet) return <EmptyState text={SWEET_EMPTY} />;
          if (c.status === 'under_review') return <EmptyState text={RECOMPUTING} />;
          if (c.status === 'warming') return <EmptyState text={warmingText(c)} />;
          return <Candidates s={c.series.sweet} />;
        }}
      </Loadable>
    </Box>
  );
}
