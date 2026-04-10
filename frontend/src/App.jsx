import { BrowserRouter, Routes, Route } from 'react-router-dom'
import Header from './components/Header'
import MarketList from './components/MarketList'
import MarketDetail from './components/MarketDetail'
import TradersDashboard from './components/TradersDashboard'

export default function App() {
  return (
    <BrowserRouter basename={import.meta.env.PROD ? '/polymarket' : '/'}>
      <Header />
      <main style={{ flex: 1, padding: '24px 16px', maxWidth: 2000, margin: '0 auto', width: '100%' }}>
        <Routes>
          <Route path="/" element={<MarketList />} />
          <Route path="/market/:slug" element={<MarketDetail />} />
          <Route path="/traders" element={<TradersDashboard />} />
        </Routes>
      </main>
    </BrowserRouter>
  )
}
