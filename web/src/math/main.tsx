// The public /math entry (09-27 §5.1): no router, no api client, no stream, no help.ts.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../theme.css';
import './math.css';
import { loadEngine } from '../wasm/engine';
import { MathPage } from './MathPage';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <MathPage engine={loadEngine()} />
    </StrictMode>,
  );
}
