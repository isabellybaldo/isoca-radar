import { useState } from 'react';
import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import type { LedgerEntry, PaperAccount, Position } from '../api/types';
import { useApi, type ApiState } from '../api/useApi';
import { Box } from '../components/Box';
import { EdgeCapture } from '../components/EdgeCapture';
import { EmptyState } from '../components/EmptyState';
import { Fold } from '../components/Fold';
import { LineChart, type Series } from '../components/LineChart';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { shortId } from '../lib/labels';
import { formatDecimal, formatMoney, formatMoneyCents, isZero } from '../lib/money';
import { fmtUtcDateTime, fmtUtcHm } from '../lib/time';

export const PAPER_QUIP = "The naive strategy is doing great. Until 'Other' wins.";
/** The flicker finding, in one line: P0 buys the basket, the flicker is gone before the fill, and it sells the pieces back at a loss. */
export const PAPER_LOSS_QUIP = 'Paper losses, real lessons. At 145 ms the arbitrage is gone before we arrive; we pay the spread to leave.';
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
  {
    key: 'market',
    label: 'MARKET',
    // No outcome name is stored anywhere (legs carry ids only), so the short market id tells rows apart.
    render: (p) => (
      <span title={p.market_id}>
        {p.event_title !== null && (
          <>
            <span>{p.event_title}</span>{' '}
          </>
        )}
        <span className="dim">{shortId(p.market_id)}</span>
      </span>
    ),
  },
  { key: 'side', label: 'SIDE', render: (p) => `${p.outcome.toUpperCase()}${p.returned ? ' (returned)' : ''}` },
  { key: 'qty', label: 'QTY', render: (p) => formatDecimal(p.qty) },
  { key: 'basis', label: 'BASIS', render: (p) => <Money value={p.basis} /> },
];

const CONTROL = 'P3-naive';

/**
 * The x-axis for the equity chart. The api always answers a fixed 24 h window (`windowFrom` ..
 * `windowTo`), but a run may only have a few hours of marks in it; starting the axis at the window's
 * nominal edge then squashes every line into the right-hand side. Starting it at the first mark
 * actually seen fills the width instead, and the label says so rather than still claiming 24 h.
 */
export function equityAxis(
  pointTimes: number[],
  windowFrom: number,
  windowTo: number,
): { xMin: number; xMax: number; xLabels?: { left: string; right: string } } {
  if (pointTimes.length === 0) return { xMin: windowFrom, xMax: windowTo };
  const dataFrom = Math.min(...pointTimes);
  if (dataFrom <= windowFrom) return { xMin: windowFrom, xMax: windowTo };
  return { xMin: dataFrom, xMax: windowTo, xLabels: { left: `since ${fmtUtcHm(dataFrom)} UTC`, right: 'now' } };
}

function AccountsChart({ accounts }: { accounts: PaperAccount[] }) {
  const ids = accounts.map((a) => a.account_id);
  // Each result carries its own account: a render that still holds the previous scenario's answer
  // (before the refetch lands) never pairs it with this scenario's accounts by position.
  const eq = useApi(
    (s) => Promise.all(accounts.map((a) => api.paperEquity(a.account_id, 24, s).then((e) => ({ a, e })))),
    [ids.join('|')],
  );
  const [hideControl, setHideControl] = useState(false);
  const hasControl = accounts.some((a) => a.policy === CONTROL);
  return (
    <>
      {hasControl && (
        <label className="check">
          <input type="checkbox" checked={hideControl} onChange={(e) => setHideControl(e.target.checked)} />
          hide P3 (control)
        </label>
      )}
      <Loadable state={eq} what="equity">
        {(loaded) => {
          const all = loaded.filter(({ a }) => ids.includes(a.account_id) && !(hideControl && a.policy === CONTROL));
          const series: Series[] = all.map(({ a, e }) => ({
            label: policyLabel(a),
            tone: TONES[a.policy] ?? 'p0',
            // geometry only: the numbers never reach the screen as text
            points: e.points.map((p) => ({ x: p.t_ms, y: Number(p.equity) })),
          }));
          const ext = extremes(all.flatMap(({ e }) => e.points.map((p) => p.equity)));
          const axis = equityAxis(
            all.flatMap(({ e }) => e.points.map((p) => p.t_ms)),
            Math.min(...loaded.map(({ e }) => e.from_ms)),
            Math.max(...loaded.map(({ e }) => e.to_ms)),
          );
          const ends = all.filter(({ e }) => e.points.length > 0);
          return (
            <>
              <div className="eqwrap">
                <LineChart
                  label="equity per account"
                  xMin={axis.xMin}
                  xMax={axis.xMax}
                  xLabels={axis.xLabels}
                  series={series}
                  legend={false}
                  yLabels={ext ? { bottom: formatMoney(ext.min), top: formatMoney(ext.max) } : undefined}
                />
                {ends.length > 0 && (
                  <ul className="plain endlegend" aria-label="equity now">
                    {ends.map(({ a, e }) => (
                      <li key={a.account_id}>
                        <span className={`sw ${TONES[a.policy] ?? 'p0'}`} aria-hidden="true" />
                        <span>{policyLabel(a)}</span> <b className="num">{formatMoneyCents(e.points[e.points.length - 1].equity)}</b>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
              {hasControl && <p className="quip">{PAPER_QUIP}</p>}
              <p className="quip">{PAPER_LOSS_QUIP}</p>
            </>
          );
        }}
      </Loadable>
    </>
  );
}

function AccountsTable({ accounts }: { accounts: PaperAccount[] }) {
  return (
    <div className="scroll">
      <table>
        <thead>
          <tr>
            {['ACCOUNT', 'REALIZED', 'UNREALIZED', 'LOCKED', 'MAX DD', 'OPEN'].map((h) => (
              <th key={h} scope="col">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {accounts.map((a) => (
            <tr key={a.account_id}>
              <td>
                {policyCardTitle(a.policy)}
                {!isZero(a.fee_bips) && ` · FEE ${a.fee_bips} BP`}
              </td>
              {a.marked_ms === null ? (
                <td colSpan={5} className="dim">
                  No marks yet. Nothing ventured, nothing lost.
                </td>
              ) : (
                <>
                  <td className="num">
                    <Money value={a.realized} signed tone cents />
                  </td>
                  <td className="num">
                    <Money value={a.unrealized} signed tone cents />
                  </td>
                  <td className="num">
                    <Money value={a.locked} cents />
                  </td>
                  <td className="num">
                    <Money value={a.max_drawdown} cents />
                  </td>
                  <td className="num">{a.n_positions === null ? '—' : a.n_positions}</td>
                </>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Ledger({ accounts, account, cursor, marks, onAccount, onCursor, onMarks }: {
  accounts: PaperAccount[];
  account: string;
  cursor: string | null;
  /** Show the minute `mark` rows (hidden by default). */
  marks: boolean;
  onAccount: (id: string) => void;
  onCursor: (c: string | null) => void;
  onMarks: (show: boolean) => void;
}) {
  const page = useApi((s) => api.paperLedger(account, cursor, !marks, s), [account, cursor, marks]);
  return (
    <>
      <div className="seg" role="group" aria-label="ledger account">
        {accounts.map((a) => (
          <button key={a.account_id} type="button" className={a.account_id === account ? 'on' : undefined} aria-pressed={a.account_id === account} onClick={() => onAccount(a.account_id)}>
            {policyCardTitle(a.policy)}
          </button>
        ))}
      </div>
      <label className="check">
        <input type="checkbox" checked={marks} onChange={(e) => onMarks(e.target.checked)} />
        show minute marks
      </label>
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
    </>
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
    <Box
      title={`ACCOUNTS · ${formatMoney(inScenario[0]?.bankroll ?? null)} EACH · LAST 24 H`}
      help="equity"
      actions={
        <div className="seg" role="group" aria-label="latency scenario">
          {scenarios.map((s) => (
            <button key={s} type="button" className={s === scenario ? 'on' : undefined} aria-pressed={s === scenario} onClick={() => set({ scenario: s, account: null, lcursor: null })}>
              {s.toUpperCase()}
            </button>
          ))}
        </div>
      }
    >
      <AccountsChart key={scenario} accounts={inScenario} />
      <Fold id="paper:accounts" label="all numbers">
        <AccountsTable accounts={inScenario} />
      </Fold>
      <Fold id="paper:ledger" label="ledger">
        {account !== undefined && (
          <Ledger
            accounts={inScenario}
            account={account}
            cursor={sp.get('lcursor')}
            marks={sp.get('marks') === '1'}
            onAccount={(id) => set({ account: id, lcursor: null })}
            onCursor={(c) => set({ lcursor: c })}
            onMarks={(show) => set({ marks: show ? '1' : null, lcursor: null })}
          />
        )}
      </Fold>
      <Fold id="paper:awaiting" label="awaiting resolution">
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
      </Fold>
    </Box>
  );
}

export function Paper() {
  const accounts = useApi((s) => api.paperAccounts(s), []);
  const positions = useApi((s) => api.paperPositions(s), []);
  return (
    <>
      <EdgeCapture />
      <Loadable state={accounts} what="paper accounts">
        {(d) =>
          d.items.length === 0 ? (
            <Box title="PAPER" help="paper">
              <EmptyState text={PAPER_EMPTY} />
            </Box>
          ) : (
            <PaperBody accounts={d.items} positions={positions} />
          )
        }
      </Loadable>
    </>
  );
}
