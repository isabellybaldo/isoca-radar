// Frozen-site plan, task 4: the portfolio landing's entry (portfolio/index.html). No router, no api client.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../theme.css';
import './portfolio.css';
import { Landing } from './Landing';
import { Shell } from './Shell';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Shell>
        <Landing />
      </Shell>
    </StrictMode>,
  );
}
