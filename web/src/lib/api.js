// Thin fetch wrapper: JSON in/out, CSRF header on every call, typed errors.
// A 409 carries `err.current` and `err.code` (the server's `error` string:
// "conflict" for a version clash, "recording_mismatch" / "edit_mismatch" /
// "selection_moved" for the recording contract). A network failure or a
// timeout surfaces as `err.offline`.

import { base } from '$app/paths';

export class ApiError extends Error {
  constructor(status, message, current, code) {
    super(message);
    this.status = status;
    this.current = current;
    this.code = code ?? (status === 0 ? 'offline' : null);
    this.offline = status === 0;
  }
}

export async function api(path, opts = {}) {
  const { method = 'GET', body, keepalive, timeout = 30000 } = opts;
  const ac = typeof AbortController !== 'undefined' ? new AbortController() : null;
  const t = ac && timeout ? setTimeout(() => ac.abort(), timeout) : null;
  let res;
  try {
    res = await fetch(`${base}/api${path}`, {
      method,
      headers: {
        'X-Requested-By': 'sideout',
        ...(body !== undefined ? { 'Content-Type': 'application/json' } : {})
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
      keepalive: !!keepalive,
      signal: ac?.signal
    });
  } catch {
    throw new ApiError(0, 'offline');
  } finally {
    if (t) clearTimeout(t);
  }
  let data = null;
  try {
    data = await res.json();
  } catch {
    /* empty body */
  }
  if (!res.ok) {
    if (res.status === 401 && !path.startsWith('/auth') && location.pathname !== `${base}/login`) {
      location.href = `${base}/login`;
    }
    throw new ApiError(res.status, data?.error || `HTTP ${res.status}`, data?.current, typeof data?.error === 'string' ? data.error : null);
  }
  return data;
}
