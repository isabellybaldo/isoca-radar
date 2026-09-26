import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import type { LedgerEntry, PaperAccount, Position } from '../api/types';
import { useApi, type ApiState } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { LineChart, type Series } from '../components/LineChart';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { formatDecimal, formatMoney, isZero } from '../lib/money';
import { fmtUtcDateTime } from '../lib/time';

export const PAPER_QUIP = "The naive strategy is doing great. Until 'Other' wins.";
export const PAPER_EMPTY = 'No paper trades yet. Nothing ventured, nothing lost.';
export const NOTHING_AWAITING = 'Nothing awaiting resolution. Patience is free.';
const TONES: Record<string, string> = { 'P0-converter': 'p0', 'P1-settlement': 'p1', 'P2-combo': 'p2', 'P3-naive': 'p3' };

/** "P0-converter" → "P0 converter" (+ the fee sensitivity when set). */
export function policyLabel(a: Pick<PaperAccount, 'policy' | 'fee_bips'>): string {
  const base = a.policy.replace('-', ' ');
  return isZero(a.fee_bips) ? base : `${base} (fee ${a.fee_bips} bp)`;
}

/** "P0-converter" → "P0 · CONVERTER". */
export function policyCardTitle(policy: string): string {
  const [p, name = ''] = policy.split('-');
  return `${p} · ${name.toUpperCase()}`;
}

/** Min and max of decimal strings: compared as numbers, returned exactly as sent (for labels). */
export function extremes(values: string[]): { min: string; max: string } | null {
  if (values.length === 0) return null;
  let min = values[0];
  let max = values[0];
  for (const v of values) {
    if (Number(v) < Number(min)) min = v;
    if (Number(v) > Number(max)) max = v;
  }
  return { min, max };
}

const ledgerColumns: Column<LedgerEntry>[] = [
  { key: 't', label: 'TIME', render: (l) => fmtUtcDateTime(l.t_ms) },
  { key: 'kind', label: 'KIND', render: (l) => l.kind },
  { key: 'qty', label: 'QTY', render: (l) => formatDecimal(l.qty) },
  { key: 'cash', label: 'CASH', render: (l) => <Money value={l.cash_delta} signed tone /> },
  { key: 'fees', label: 'FEES', render: (l) => <Money value={l.fees} /> },
  { key: 'realized', label: 'REALIZED', render: (l) => <Money value={l.realized_delta} signed tone /> },
];

const positionColumns: Column<Position>[] = [
  { key: 'acct', label: 'ACCT', render: (p) => p.account_id.split('-')[0] },
  { key: 'market', label: 'MARKET', render: (p) => <span title={p.market_id}>{p.event_title ?? p.market_id}</span> },
  { key: 'side', label: 'SIDE', render: (p) => `${p.outcome.toUpperCase()}${p.returned ? ' (returned)' : ''}` },
  { key: 'qty', label: 'QTY', render: (p) => formatDecimal(p.qty) },
  { key: 'basis', label: 'BASIS', render: (p) => <Money value={p.basis} /> },
];

function EquityChart({ accounts }: { accounts: PaperAccount[] }) {
  const ids = accounts.map((a) => a.account_id);
  const eq = useApi((s) => Promise.all(ids.map((id) => api.paperEquity(id, 24, s))), [ids.join('|')]);
  return (
    <Box title={`EQUITY · ${formatMoney(accounts[0]?.bankroll ?? null)} EACH · LAST 24 H`}>
      <Loadable state={eq} what="equity">
        {(all) => {
          const series: Series[] = all.map((e, i) => ({
            label: policyLabel(accounts[i]),
            tone: TONES[accounts[i].policy] ?? 'p0',
            // geometry only: the numbers never reach the screen as text
            points: e.points.map((p) => ({ x: p.t_ms, y: Number(p.equity) })),
          }));
          const ext = extremes(all.flatMap((e) => e.points.map((p) => p.equity)));
          return (
            <>
              <LineChart
                label="equity per account"
                xMin={Math.min(...all.map((e) => e.from_ms))}
                xMax={Math.max(...all.map((e) => e.to_ms))}
                series={series}
                yLabels={ext ? { bottom: formatMoney(ext.min), top: formatMoney(ext.max) } : undefined}
              />
              <p className="quip">{PAPER_QUIP}</p>
            </>
          );
        }}
      </Loadable>
    </Box>
  );
}

function Cards({ accounts }: { accounts: PaperAccount[] }) {
  return (
    <div className="cards">
      {accounts.map((a) => (
        <div key={a.account_id} className="card">
          <p className="p">
            {policyCardTitle(a.policy)}
            {!isZero(a.fee_bips) && ` · FEE ${a.fee_bips} BP`}
          </p>
          <p className="eq">
            <Money value={a.equity ?? a.bankroll} />
          </p>
          {a.marked_ms === null ? (
            <p className="dim">No marks yet. Nothing ventured, nothing lost.</p>
          ) : (
            <p>
              realized <Money value={a.realized} signed tone /> · unrealized <Money value={a.unrealized} signed tone /> · locked{' '}
              <Money value={a.locked} /> · max dd <Money value={a.max_drawdown} /> · {a.n_positions ?? 0} open
            </p>
          )}
        </div>
      ))}
    </div>
  );
}

function Ledger({ accounts, account, cursor, onAccount, onCursor }: {
  accounts: PaperAccount[];
  account: string;
  cursor: string | null;
  onAccount: (id: string) => void;
  onCursor: (c: string | null) => void;
}) {
  const page = useApi((s) => api.paperLedger(account, cursor, s), [account, cursor]);
  const current = accounts.find((a) => a.account_id === account);
  return (
    <Box title={`LEDGER · ${current ? policyCardTitle(current.policy) : account}`}>
      <div className="seg" role="group" aria-label="ledger account">
        {accounts.map((a) => (
          <button key={a.account_id} type="button" className={a.account_id === account ? 'on' : undefined} aria-pressed={a.account_id === account} onClick={() => onAccount(a.account_id)}>
            {policyCardTitle(a.policy)}
          </button>
        ))}
      </div>
      <Loadable state={page} what="the ledger">
        {(d) =>
          d.items.length === 0 ? (
            <EmptyState text="Empty ledger. Nothing ventured, nothing lost." />
          ) : (
            <>
              <Table columns={ledgerColumns} rows={d.items} rowKey={(l) => `${l.run_id}_${l.seq}`} />
              <div className="pager">
                <button type="button" disabled={!cursor} onClick={() => onCursor(null)}>
                  NEWEST
                </button>
                <button type="button" disabled={!d.next_cursor} onClick={() => d.next_cursor && onCursor(d.next_cursor)}>
                  OLDER →
                </button>
              </div>
            </>
          )
        }
      </Loadable>
    </Box>
  );
}

function PaperBody({ accounts, positions }: { accounts: PaperAccount[]; positions: ApiState<{ items: Position[] }> }) {
  const [sp, setSp] = useSearchParams();
  const scenarios = [...new Set(accounts.map((a) => a.scenario))].sort();
  const wanted = sp.get('scenario');
  const scenario = wanted !== null && scenarios.includes(wanted) ? wanted : scenarios[0];
  const inScenario = accounts.filter((a) => a.scenario === scenario);
  const ids = inScenario.map((a) => a.account_id);
  const wantedAccount = sp.get('account');
  const account = wantedAccount !== null && ids.includes(wantedAccount) ? wantedAccount : ids[0];
  const set = (changes: Record<string, string | null>) => {
    const next = new URLSearchParams(sp);
    for (const [k, v] of Object.entries(changes)) {
      if (v === null) next.delete(k);
      else next.set(k, v);
    }
    setSp(next);
  };
  return (
    <>
      <Box title="SCENARIO">
        <div className="seg" role="group" aria-label="latency scenario">
          {scenarios.map((s) => (
            <button key={s} type="button" className={s === scenario ? 'on' : undefined} aria-pressed={s === scenario} onClick={() => set({ scenario: s, account: null, lcursor: null })}>
              {s.toUpperCase()}
            </button>
          ))}
        </div>
      </Box>
      <EquityChart accounts={inScenario} />
      <Cards accounts={inScenario} />
      <div className="grid2">
        <Box title="AWAITING RESOLUTION">
          <Loadable state={positions} what="positions">
            {(d) => {
              const rows = d.items.filter((p) => ids.includes(p.account_id));
              return rows.length === 0 ? (
                <EmptyState text={NOTHING_AWAITING} />
              ) : (
                <Table columns={positionColumns} rows={rows} rowKey={(p) => `${p.account_id}|${p.market_id}|${p.outcome}|${p.returned}`} />
              );
            }}
          </Loadable>
        </Box>
        {account !== undefined && (
          <Ledger
            accounts={inScenario}
            account={account}
            cursor={sp.get('lcursor')}
            onAccount={(id) => set({ account: id, lcursor: null })}
            onCursor={(c) => set({ lcursor: c })}
          />
        )}
      </div>
    </>
  );
}

export function Paper() {
  const accounts = useApi((s) => api.paperAccounts(s), []);
  const positions = useApi((s) => api.paperPositions(s), []);
  return (
    <Loadable state={accounts} what="paper accounts">
      {(d) =>
        d.items.length === 0 ? (
          <Box title="PAPER">
            <EmptyState text={PAPER_EMPTY} />
          </Box>
        ) : (
          <PaperBody accounts={d.items} positions={positions} />
        )
      }
    </Loadable>
  );
}
