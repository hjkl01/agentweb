import { createRoot } from 'react-dom/client';
import { App } from './App';
import { AppErrorBoundary } from './app/AppErrorBoundary';
import './style.css';

createRoot(document.getElementById('root')!).render(
  <AppErrorBoundary>
    <App />
  </AppErrorBoundary>,
);
