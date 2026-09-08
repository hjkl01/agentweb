import { useEffect, useState } from 'react';

export function sessionPath(id: string) {
  return `/sessions/${encodeURIComponent(id)}`;
}

export function sessionIdFromPath(pathname = window.location.pathname) {
  const match = pathname.match(/^\/sessions\/([^/]+)\/?$/);
  return match ? decodeURIComponent(match[1]) : undefined;
}

export function useAppRoute() {
  const [path, setPath] = useState(() => window.location.pathname);

  useEffect(() => {
    const onPopState = () => setPath(window.location.pathname);
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  const navigate = (next: string, replace = false) => {
    if (replace) window.history.replaceState({}, '', next);
    else window.history.pushState({}, '', next);
    setPath(next);
  };

  return { path, sessionId: sessionIdFromPath(path), navigate };
}
