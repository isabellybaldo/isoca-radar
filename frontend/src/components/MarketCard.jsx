import { Link } from 'react-router-dom'
import PriceBar from './PriceBar'
import styles from './MarketCard.module.css'

function fmt(n) {
  if (!n) return '—'
  const v = parseFloat(n)
  if (v >= 1_000_000) return `$${(v / 1_000_000).toFixed(1)}M`
  if (v >= 1_000)     return `$${(v / 1_000).toFixed(0)}K`
  return `$${v.toFixed(0)}`
}

export default function MarketCard({ market }) {
  const prices = market.outcomePrices || []
  const yes = parseFloat(prices[0] ?? 0.5)
  const no  = parseFloat(prices[1] ?? 0.5)

  return (
    <Link to={`/market/${market.slug}`} className={styles.card + ' pixel-box'}>
      {market.image && (
        <img src={market.image} alt="" className={styles.thumb} />
      )}
      <div className={styles.body}>
        <p className={styles.question}>{market.question}</p>
        <PriceBar yes={yes} no={no} />
        <div className={styles.meta}>
          <span className={styles.vol}>vol {fmt(market.volume)}</span>
          {market.endDate && (
            <span className={styles.end}>
              ends {new Date(market.endDate).toLocaleDateString()}
            </span>
          )}
        </div>
      </div>
    </Link>
  )
}
