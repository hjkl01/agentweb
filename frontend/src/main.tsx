import { createRoot } from 'react-dom/client';
import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { App } from './App';
import { AppErrorBoundary } from './app/AppErrorBoundary';
import './style.css';
import './styles/workspace.css';

const root = document.getElementById('root');
if (!root) throw new Error('Agent Web root element #root was not found');

createRoot(root).render(
  <AppErrorBoundary>
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<App />} />
        <Route path="/sessions/:sessionId" element={<App />} />
        <Route path="*" element={<App />} />
      </Routes>
    </BrowserRouter>
  </AppErrorBoundary>,
);
