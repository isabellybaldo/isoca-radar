import { useParams, Link } from 'react-router-dom'
import useMarketDetail from '../hooks/useMarketDetail'
import PriceBar from './PriceBar'
import PriceChart from './PriceChart'
import LoadingPixel from './LoadingPixel'
import styles from './MarketDetail.module.css'

function fmt(n) {
  if (!n) return '—'
  const v = parseFloat(n)
  if (v >= 1_000_000) return `$${(v / 1_000_000).toFixed(2)}M`
  if (v >= 1_000)     return `$${(v / 1_000).toFixed(1)}K`
  return `$${v.toFixed(2)}`
}

export default function MarketDetail() {
  const { slug } = useParams()
  const { market, history, loading, error } = useMarketDetail(slug)

  if (loading) return <LoadingPixel text="loading market..." />
  if (error)   return (
    <div className={styles.error + ' pixel-box'}>
      <p>!! {error}</p>
      <Link to="/" className="pixel-btn" style={{ marginTop: 16, display: 'inline-block' }}>← back</Link>
    </div>
  )
  if (!market) return null

  const prices = market.outcomePrices || []
  const yes = parseFloat(prices[0] ?? 0.5)
  const no  = parseFloat(prices[1] ?? 0.5)

  return (
    <div className={styles.wrap}>
      <Link to="/" className={styles.back}>← back to markets</Link>

      <div className={styles.card + ' pixel-box'}>
        {market.image && (
          <img src={market.image} alt="" className={styles.hero} />
        )}
        <div className={styles.body}>
          <h1 className={styles.question}>{market.question}</h1>

          <div className={styles.priceSection}>
            <p className={styles.sectionLabel}>// current odds</p>
            <PriceBar yes={yes} no={no} />
          </div>

          <div className={styles.stats}>
            <div className={styles.stat}>
              <span className={styles.statLabel}>volume</span>
              <span className={styles.statVal}>{fmt(market.volume)}</span>
            </div>
            <div className={styles.stat}>
              <span className={styles.statLabel}>liquidity</span>
              <span className={styles.statVal}>{fmt(market.liquidity)}</span>
            </div>
            {market.endDate && (
              <div className={styles.stat}>
                <span className={styles.statLabel}>closes</span>
                <span className={styles.statVal}>{new Date(market.endDate).toLocaleDateString()}</span>
              </div>
            )}
          </div>

          {market.description && (
            <div className={styles.desc}>
              <p className={styles.sectionLabel}>// description</p>
              <p className={styles.descText}>{market.description}</p>
            </div>
          )}

          <div className={styles.chartSection}>
            <p className={styles.sectionLabel}>// yes price history (1w)</p>
            <PriceChart history={history} />
          </div>
        </div>
      </div>
    </div>
  )
}
