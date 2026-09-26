import { NavLink, Route, Routes } from 'react-router';
import { useStream } from './api/StreamContext';
import { EmptyState } from './components/EmptyState';
import { FakeToggle } from './components/FakeToggle';
import { LiveDot } from './components/LiveDot';
import { EpisodeDetail } from './pages/EpisodeDetail';
import { Episodes } from './pages/Episodes';
import { Health } from './pages/Health';
import { Paper } from './pages/Paper';
import { Radar } from './pages/Radar';

const NAV = [
  { to: '/', label: 'RADAR', end: true },
  { to: '/episodes', label: 'EPISODES', end: false },
  { to: '/paper', label: 'PAPER', end: false },
  { to: '/health', label: 'HEALTH', end: false },
];

export function Header() {
  const { state } = useStream();
  return (
    <header className="top">
      <span className="logo">♥ cute-trading</span>
      <nav className="nav" aria-label="main">
        {NAV.map((n) => (
          <NavLink key={n.to} to={n.to} end={n.end} className={({ isActive }) => (isActive ? 'on' : undefined)}>
            {n.label}
          </NavLink>
        ))}
      </nav>
      <LiveDot state={state} />
      <FakeToggle />
    </header>
  );
}

export function NotFound() {
  return <EmptyState text="This page does not exist. Neither does free money." />;
}

export function App() {
  return (
    <div className="app">
      <Header />
      <main className="body">
        <Routes>
          <Route path="/" element={<Radar />} />
          <Route path="/episodes" element={<Episodes />} />
          <Route path="/episodes/:id" element={<EpisodeDetail />} />
          <Route path="/paper" element={<Paper />} />
          <Route path="/health" element={<Health />} />
          <Route path="*" element={<NotFound />} />
        </Routes>
      </main>
    </div>
  );
}
