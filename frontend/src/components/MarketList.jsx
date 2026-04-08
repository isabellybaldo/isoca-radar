import useMarkets from '../hooks/useMarkets'
import MarketCard from './MarketCard'
import LoadingPixel from './LoadingPixel'
import styles from './MarketList.module.css'

export default function MarketList() {
  const { markets, loading, hasMore, error, loadMore } = useMarkets()

  if (loading && markets.length === 0) return <LoadingPixel text="fetching markets..." />

  if (error) return (
    <div className={styles.error + ' pixel-box'}>
      <p>!! error: {error}</p>
    </div>
  )

  return (
    <div className={styles.wrap}>
      <h2 className={styles.title}>// prediction markets</h2>
      <div className={styles.grid}>
        {markets.map((m) => <MarketCard key={m.id ?? m.slug} market={m} />)}
      </div>
      {hasMore && (
        <div className={styles.more}>
          <button
            className="pixel-btn"
            onClick={loadMore}
            disabled={loading}
          >
            {loading ? '...' : 'load more ▼'}
          </button>
        </div>
      )}
    </div>
  )
}
