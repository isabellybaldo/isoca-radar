import { useState, useEffect } from 'react'
import client from '../api/client'

export default function useMarketDetail(slug) {
  const [market, setMarket]   = useState(null)
  const [history, setHistory] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError]     = useState(null)

  useEffect(() => {
    if (!slug) return
    setLoading(true)
    setError(null)

    async function fetch() {
      try {
        const m = await client.get(`/markets/${slug}`)
        setMarket(m)

        const tokenId = m.clobTokenIds?.[0]
        if (tokenId) {
          try {
            const h = await client.get(`/prices/history?token_id=${tokenId}&interval=1w`)
            setHistory(Array.isArray(h) ? h : [])
          } catch {
            setHistory([])
          }
        }
      } catch (e) {
        setError(e.message || 'Market not found')
      } finally {
        setLoading(false)
      }
    }

    fetch()
  }, [slug])

  return { market, history, loading, error }
}
