import { BrowserRouter, Routes, Route } from 'react-router-dom'
import Header from './components/Header'
import MarketList from './components/MarketList'
import MarketDetail from './components/MarketDetail'

export default function App() {
  return (
    <BrowserRouter>
      <Header />
      <main style={{ flex: 1, padding: '24px 16px', maxWidth: 1100, margin: '0 auto', width: '100%' }}>
        <Routes>
          <Route path="/" element={<MarketList />} />
          <Route path="/market/:slug" element={<MarketDetail />} />
        </Routes>
      </main>
    </BrowserRouter>
  )
}
