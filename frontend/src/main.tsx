import { createRoot } from 'react-dom/client';
import { AppRouter } from './app/AppRouter';
import { AppErrorBoundary } from './app/AppErrorBoundary';
import './style.css';
import './styles/workspace.css';
import './styles/responsive.css';
import './styles/code-preview.css';
import './styles/mentions.css';
import './styles/terminal.css';

const favicon = document.createElement('link');
favicon.rel = 'icon';
favicon.type = 'image/svg+xml';
favicon.href = '/favicon.svg';
document.head.appendChild(favicon);

document.title = 'Agent Web';

const root = document.getElementById('root');
if (!root) throw new Error('Agent Web root element #root was not found');

createRoot(root).render(
  <AppErrorBoundary>
    <AppRouter />
  </AppErrorBoundary>,
);
