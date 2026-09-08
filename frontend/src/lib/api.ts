export class ApiError extends Error {
  status?: number;
  path: string;
  constructor(message: string, path: string, status?: number) { super(message); this.name = 'ApiError'; this.path = path; this.status = status; }
}

let authHeader = sessionStorage.getItem('agentweb_auth');

function requestOptions(options?: RequestInit): RequestInit {
  return {
    credentials: 'include',
    headers: { 'Content-Type': 'application/json', ...(authHeader ? { Authorization: `Basic ${authHeader}` } : {}), ...(options?.headers || {}) },
    ...options,
  };
}

async function login(): Promise<boolean> {
  const username = window.prompt('Agent Web 用户名', 'admin');
  if (username === null) return false;
  const password = window.prompt('Agent Web 密码');
  if (password === null) return false;
  const encoded = btoa(`${username}:${password}`);
  const response = await fetch('/api/auth/login', requestOptions({ method: 'POST', body: JSON.stringify({ username, password }) }));
  if (!response.ok) return false;
  authHeader = encoded;
  sessionStorage.setItem('agentweb_auth', encoded);
  return true;
}

export async function api<T = any>(path: string, options?: RequestInit): Promise<T> {
  const url = '/api' + path;
  let response: Response;
  try { response = await fetch(url, requestOptions(options)); }
  catch (error) { const message = error instanceof Error ? error.message : String(error); throw new ApiError(`无法连接后端：${message}`, path); }
  if (response.status === 401 && path !== '/auth/login') {
    if (await login()) {
      try { response = await fetch(url, requestOptions(options)); }
      catch (error) { const message = error instanceof Error ? error.message : String(error); throw new ApiError(`无法连接后端：${message}`, path); }
    }
  }
  const text = await response.text();
  let data: any = {};
  if (text) { try { data = JSON.parse(text); } catch { data = { error: text.slice(0, 500) }; } }
  if (!response.ok) throw new ApiError(data.error || data.message || response.statusText, path, response.status);
  return data as T;
}

export function fileUrl(sessionId: string, path: string): string { return `/sessions/${sessionId}/file/${path.split('/').map(encodeURIComponent).join('/')}`; }
