export async function api<T = any>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch('/api' + path, {
    headers: { 'Content-Type': 'application/json', ...(options?.headers || {}) },
    ...options,
  });

  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(data.error || response.statusText);
  }
  return data as T;
}

export function fileUrl(sessionId: string, path: string): string {
  const encodedPath = path.split('/').map(encodeURIComponent).join('/');
  return `/sessions/${sessionId}/file/${encodedPath}`;
}