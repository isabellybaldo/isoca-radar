import { useState, useEffect, useCallback } from 'react'
import client from '../api/client'

const PAGE = 20

export default function useMarkets() {
  const [markets, setMarkets] = useState([])
  const [offset, setOffset]   = useState(0)
  const [loading, setLoading] = useState(false)
  const [hasMore, setHasMore] = useState(true)
  const [error, setError]     = useState(null)

  const load = useCallback(async (off = 0) => {
    setLoading(true)
    setError(null)
    try {
      const data = await client.get(`/markets?limit=${PAGE}&offset=${off}`)
      setMarkets((prev) => off === 0 ? data : [...prev, ...data])
      setHasMore(data.length === PAGE)
      setOffset(off + data.length)
    } catch (e) {
      setError(e.message || 'Failed to load markets')
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { load(0) }, [load])

  const loadMore = () => { if (!loading && hasMore) load(offset) }

  return { markets, loading, hasMore, error, loadMore }
}
