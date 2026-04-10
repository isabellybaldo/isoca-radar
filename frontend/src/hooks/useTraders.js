import { useState, useEffect, useCallback } from 'react'
import client from '../api/client'

export function useTraders({ category = null, sortBy = 'total_profit_usd' } = {}) {
  const [traders, setTraders] = useState([])
  const [status, setStatus] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)

  const fetchTraders = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const params = { sort_by: sortBy }
      if (category) params.category = category

      const [tradersRes, statusRes] = await Promise.all([
        client.get('/traders', { params }),
        client.get('/traders/status'),
      ])

      setTraders(tradersRes.traders ?? [])
      setStatus(statusRes)
    } catch (err) {
      const detail = err?.response?.data?.detail
      const msg = detail
        ?? (err?.code === 'ECONNABORTED' || err?.message?.includes('timeout') ? 'Backend unavailable' : 'Failed to load traders')
      setError(msg)
    } finally {
      setLoading(false)
    }
  }, [category, sortBy])

  useEffect(() => {
    fetchTraders()
  }, [fetchTraders])

  return { traders, status, loading, error, refetch: fetchTraders }
}
