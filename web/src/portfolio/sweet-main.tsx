// Frozen-site plan, task 4: the frozen Sweet spots page's entry (portfolio/sweet.html). No router, no stream.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../theme.css';
import './portfolio.css';
import { FrozenSweet } from './FrozenSweet';
import { Shell } from './Shell';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Shell>
        <FrozenSweet />
      </Shell>
    </StrictMode>,
  );
}
