import { useState } from 'react'
import LoadingPixel from './LoadingPixel'
import { useTraders } from '../hooks/useTraders'
import styles from './TradersDashboard.module.css'

const SORT_OPTIONS = [
  { value: 'total_profit_usd', label: 'Profit' },
  { value: 'roi', label: 'ROI %' },
  { value: 'win_rate', label: 'Win Rate' },
  { value: 'num_bets', label: '# Bets' },
  { value: 'total_volume_usd', label: 'Volume' },
  { value: 'consistency_score', label: 'Consistency' },
]

const COLUMNS = [
  { key: null,               label: '#',           tip: null },
  { key: null,               label: 'Trader',      tip: null },
  { key: 'total_profit_usd', label: 'Profit',      tip: 'Total USD profit across all resolved markets (from Polymarket leaderboard)' },
  { key: 'roi',              label: 'ROI',         tip: 'Return on investment: profit ÷ total volume traded' },
  { key: 'win_rate',         label: 'Win Rate',    tip: 'Winning bets ÷ total bets placed (REDEEM txns ÷ BUY txns — excludes still-open positions)' },
  { key: 'num_bets',         label: '# Bets',      tip: 'Number of buy-side trades placed' },
  { key: 'total_volume_usd', label: 'Volume',      tip: 'Total USDC traded across all markets' },
  { key: null,               label: 'Avg Pos',     tip: 'Average position size: total volume ÷ number of bets' },
  { key: 'consistency_score',label: 'Consistency', tip: 'Proxy for sustained profitability — currently mirrors win rate; will refine to week-by-week windows' },
  { key: null,               label: 'Politics',    tip: 'Number of bets placed in politics markets' },
  { key: null,               label: 'Finance',     tip: 'Number of bets placed in finance/crypto markets' },
]

const FILTERS = [
  { value: null, label: 'All' },
  { value: 'politics', label: 'Politics' },
  { value: 'finance', label: 'Finance' },
]

function fmt$(n) {
  if (n == null) return '—'
  if (Math.abs(n) >= 1_000_000) return `$${(n / 1_000_000).toFixed(1)}M`
  if (Math.abs(n) >= 1_000) return `$${(n / 1_000).toFixed(1)}k`
  return `$${Number(n).toFixed(0)}`
}

function fmtPct(n) {
  if (n == null) return '—'
  return `${Number(n).toFixed(1)}%`
}

function truncAddr(addr) {
  if (!addr) return '—'
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`
}

function formatLastUpdate(isoStr) {
  if (!isoStr) return null
  const d = new Date(isoStr)
  const diff = Math.floor((Date.now() - d.getTime()) / 60_000)
  if (diff < 1) return 'just now'
  if (diff < 60) return `${diff}m ago`
  if (diff < 1440) return `${Math.floor(diff / 60)}h ago`
  return `${Math.floor(diff / 1440)}d ago`
}

export default function TradersDashboard() {
  const [category, setCategory] = useState(null)
  const [sortBy, setSortBy] = useState('total_profit_usd')
  const [refreshing, setRefreshing] = useState(false)

  const { traders, status, loading, error, refetch } = useTraders({ category, sortBy })

  async function handleRefresh() {
    setRefreshing(true)
    await refetch()
    setRefreshing(false)
  }

  const statusClass = {
    running: styles.statusRunning,
    done: styles.statusDone,
    error: styles.statusError,
    never_run: styles.statusNever,
  }[status?.status] ?? styles.statusNever

  const lastUpdate = status?.finished_at
    ? formatLastUpdate(status.finished_at)
    : null

  return (
    <div className={styles.wrapper}>
      {/* Top bar */}
      <div className={styles.topBar}>
        <h2 className={styles.title}>smart money tracker</h2>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          {status && (
            <span className={`${styles.statusBadge} ${statusClass}`}>
              {status.status === 'running' ? 'syncing...' : lastUpdate ? `updated ${lastUpdate}` : status.status}
            </span>
          )}
          <button
            className="pixel-btn"
            onClick={handleRefresh}
            disabled={loading || refreshing}
          >
            {refreshing ? 'loading...' : 'refresh'}
          </button>
        </div>
      </div>

      {/* Controls */}
      <div className={styles.controls}>
        <div className={styles.filterGroup}>
          {FILTERS.map(f => (
            <button
              key={String(f.value)}
              className={`${styles.filterBtn} ${category === f.value ? styles.filterBtnActive : ''}`}
              onClick={() => setCategory(f.value)}
            >
              {f.label}
            </button>
          ))}
        </div>

        <select
          className={styles.sortSelect}
          value={sortBy}
          onChange={e => setSortBy(e.target.value)}
        >
          {SORT_OPTIONS.map(o => (
            <option key={o.value} value={o.value}>sort: {o.label}</option>
          ))}
        </select>
      </div>

      {/* Content */}
      {error && <div className={styles.errorMsg}>{error}</div>}

      {loading && !error && <LoadingPixel />}

      {!loading && !error && traders.length === 0 && (
        <div className={styles.empty}>
          no traders found yet<br /><br />
          discovery runs every {'{interval}'} hours
        </div>
      )}

      {!loading && !error && traders.length > 0 && (
        <div className={styles.tableWrap}>
          <table className={styles.table}>
            <thead>
              <tr>
                {COLUMNS.map(col => (
                  <th
                    key={col.label}
                    className={col.key && sortBy === col.key ? styles.active : ''}
                    onClick={col.key ? () => setSortBy(col.key) : undefined}
                    style={!col.key ? { cursor: 'default' } : undefined}
                  >
                    {col.key && sortBy === col.key ? `${col.label} ▾` : col.label}
                    {col.tip && (
                      <span className={styles.tooltip}>{col.tip}</span>
                    )}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {traders.map((t, i) => (
                <tr key={t.address}>
                  <td className={styles.rank}>{i + 1}</td>
                  <td>
                    <span className={styles.address}>{truncAddr(t.address)}</span>
                    {t.username && <span className={styles.username}>{t.username}</span>}
                  </td>
                  <td className={t.total_profit_usd >= 0 ? styles.positive : styles.negative}>
                    {fmt$(t.total_profit_usd)}
                  </td>
                  <td className={t.roi >= 0 ? styles.positive : styles.negative}>
                    {fmtPct(t.roi)}
                  </td>
                  <td>{fmtPct(t.win_rate)}</td>
                  <td>{t.num_bets}</td>
                  <td>{fmt$(t.total_volume_usd)}</td>
                  <td>{fmt$(t.avg_position_usd)}</td>
                  <td>{fmtPct(t.consistency_score)}</td>
                  <td>
                    {t.politics_bets > 0 && (
                      <span className={`${styles.pill} ${styles.pillPolitics}`}>
                        {t.politics_bets}
                      </span>
                    )}
                  </td>
                  <td>
                    {t.finance_bets > 0 && (
                      <span className={`${styles.pill} ${styles.pillFinance}`}>
                        {t.finance_bets}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
