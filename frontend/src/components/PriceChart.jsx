import {
  ResponsiveContainer, LineChart, Line,
  XAxis, YAxis, Tooltip, CartesianGrid,
} from 'recharts'

function fmt(v) {
  return `${Math.round(v * 100)}%`
}

function fmtDate(t) {
  if (!t) return ''
  return new Date(t * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

export default function PriceChart({ history }) {
  if (!history || history.length === 0) return (
    <p style={{ fontFamily: 'var(--font-pixel)', fontSize: 9, color: 'var(--rose)', padding: '16px 0' }}>
      no price history available
    </p>
  )

  return (
    <ResponsiveContainer width="100%" height={220}>
      <LineChart data={history} margin={{ top: 4, right: 8, bottom: 0, left: -16 }}>
        <CartesianGrid stroke="#aa446533" strokeDasharray="4 4" />
        <XAxis
          dataKey="t"
          tickFormatter={fmtDate}
          tick={{ fontFamily: 'VT323', fontSize: 14, fill: '#861657' }}
          tickLine={false}
          axisLine={{ stroke: '#861657' }}
          interval="preserveStartEnd"
        />
        <YAxis
          domain={[0, 1]}
          tickFormatter={fmt}
          tick={{ fontFamily: 'VT323', fontSize: 14, fill: '#861657' }}
          tickLine={false}
          axisLine={{ stroke: '#861657' }}
          width={44}
        />
        <Tooltip
          formatter={(v) => [fmt(v), 'YES']}
          labelFormatter={fmtDate}
          contentStyle={{
            background: '#861657',
            border: '3px solid #aa4465',
            fontFamily: 'VT323',
            fontSize: 16,
            color: '#ddfff7',
            borderRadius: 0,
          }}
        />
        <Line
          type="stepAfter"
          dataKey="p"
          stroke="#93e1d8"
          strokeWidth={3}
          dot={false}
          activeDot={{ r: 5, fill: '#ffa69e', stroke: '#861657', strokeWidth: 2 }}
        />
      </LineChart>
    </ResponsiveContainer>
  )
}
