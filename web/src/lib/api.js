// Thin fetch wrapper: JSON in/out, CSRF header on every call, typed errors.
// A 409 carries `err.current`. A network failure surfaces as `err.offline`.

export class ApiError extends Error {
  constructor(status, message, current) {
    super(message);
    this.status = status;
    this.current = current;
    this.offline = status === 0;
  }
}

export async function api(path, opts = {}) {
  const { method = 'GET', body } = opts;
  let res;
  try {
    res = await fetch(`/api${path}`, {
      method,
      headers: {
        'X-Requested-By': 'sideout',
        ...(body !== undefined ? { 'Content-Type': 'application/json' } : {})
      },
      body: body !== undefined ? JSON.stringify(body) : undefined
    });
  } catch {
    throw new ApiError(0, 'offline');
  }
  let data = null;
  try {
    data = await res.json();
  } catch {
    /* empty body */
  }
  if (!res.ok) {
    if (res.status === 401 && !path.startsWith('/auth') && location.pathname !== '/login') {
      location.href = '/login';
    }
    throw new ApiError(res.status, data?.error || `HTTP ${res.status}`, data?.current);
  }
  return data;
}
