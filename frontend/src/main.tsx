import { createRoot } from 'react-dom/client';
import { AppRouter } from './app/AppRouter';
import { AppErrorBoundary } from './app/AppErrorBoundary';
import './style.css';
import './styles/workspace.css';

const root = document.getElementById('root');
if (!root) throw new Error('Agent Web root element #root was not found');

createRoot(root).render(
  <AppErrorBoundary>
    <AppRouter />
  </AppErrorBoundary>,
);
