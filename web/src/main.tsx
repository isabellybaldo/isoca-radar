import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router';
import { App } from './App';
import { StreamProvider } from './api/StreamContext';
import './theme.css';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <BrowserRouter>
        <StreamProvider>
          <App />
        </StreamProvider>
      </BrowserRouter>
    </StrictMode>,
  );
}
