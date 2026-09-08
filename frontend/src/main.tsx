import { createRoot } from 'react-dom/client';
import { App } from './App';
import { AppErrorBoundary } from './app/AppErrorBoundary';
import './style.css';
import './styles/workspace.css';

const root = document.getElementById('root');
if (!root) throw new Error('Agent Web root element #root was not found');

createRoot(root).render(
  <AppErrorBoundary>
    <App />
  </AppErrorBoundary>,
);
