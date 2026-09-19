import { writable } from 'svelte/store';

/** me-shape from /api/me: { user, team, members } */
export const me = writable(null);

/** last committed mutation seen on the SSE stream (notification, not data) */
export const mutations = writable(null);
export const sseConnected = writable(false);
export const online = writable(typeof navigator === 'undefined' ? true : navigator.onLine);

export const toast = writable(null);
let toastTimer;
export function showToast(text, isErr = false) {
  toast.set({ text, isErr });
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.set(null), 2200);
}

let source = null;
export function connectSSE() {
  if (source) return;
  source = new EventSource('/api/events');
  source.addEventListener('mutation', (ev) => {
    mutations.set(JSON.parse(ev.data));
  });
  source.onopen = () => sseConnected.set(true);
  source.onerror = () => sseConnected.set(false);
}

export function disconnectSSE() {
  source?.close();
  source = null;
  sseConnected.set(false);
}

if (typeof window !== 'undefined') {
  window.addEventListener('online', () => online.set(true));
  window.addEventListener('offline', () => online.set(false));
}
