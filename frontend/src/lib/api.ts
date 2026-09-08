export class ApiError extends Error {
  status?: number;
  path: string;

  constructor(message: string, path: string, status?: number) {
    super(message);
    this.name = 'ApiError';
    this.path = path;
    this.status = status;
  }
}

export async function api<T = any>(path: string, options?: RequestInit): Promise<T> {
  const url = '/api' + path;
  let response: Response;

  try {
    response = await fetch(url, {
      headers: { 'Content-Type': 'application/json', ...(options?.headers || {}) },
      ...options,
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new ApiError(`无法连接后端：${message}`, path);
  }

  const text = await response.text();
  let data: any = {};
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = { error: text.slice(0, 500) };
    }
  }

  if (!response.ok) {
    throw new ApiError(data.error || data.message || response.statusText, path, response.status);
  }
  return data as T;
}

export function fileUrl(sessionId: string, path: string): string {
  const encodedPath = path.split('/').map(encodeURIComponent).join('/');
  return `/sessions/${sessionId}/file/${encodedPath}`;
}
