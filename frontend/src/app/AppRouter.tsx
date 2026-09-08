import { useEffect } from 'react';
import { App } from '../App';
import { useAppRoute } from './router';

export function AppRouter() {
  const { sessionId, navigate } = useAppRoute();

  useEffect(() => {
    if (window.location.pathname !== '/' && !sessionId) navigate('/', true);
  }, [navigate, sessionId]);

  return <App sessionId={sessionId} navigate={navigate} />;
}
