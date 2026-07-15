import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from '#app-entry';
import './features/orbit/orbit-widget.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
