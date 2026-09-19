import { writable } from 'svelte/store';
import { invalidateAll } from '$app/navigation';

/** me-shape from /api/me: { user, team, members } */
export const me = writable(null);
/** module cache so the layout load does not refetch /me on every navigation */
export const meCache = { value: null };
export async function refreshMe() {
  meCache.value = null;
  await invalidateAll();
}

/** last committed mutation seen on the SSE stream (notification, not data).
 *  {entity:'resync'} is synthetic: the stream reconnected, pages refetch. */
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

/* One EventSource per tab, held only while the tab is visible. Browsers
   allow ~6 HTTP/1.1 connections per host and a stream keeps one open for
   good, so a handful of background tabs would otherwise starve every
   fetch and the app looks frozen. Hidden tabs drop the stream and
   resync when they come back. */
let source = null;
let wanted = false;
let everConnected = false;

function open() {
  if (source) return;
  source = new EventSource('/api/events');
  source.addEventListener('mutation', (ev) => {
    mutations.set(JSON.parse(ev.data));
  });
  source.onopen = () => {
    sseConnected.set(true);
    if (everConnected) mutations.set({ entity: 'resync', id: 0, version: 0, action: 'resync', actor: '' });
    everConnected = true;
  };
  source.onerror = () => sseConnected.set(false); // EventSource auto-reconnects
}
function close() {
  source?.close();
  source = null;
  sseConnected.set(false);
}

export function connectSSE() {
  wanted = true;
  if (typeof document === 'undefined' || document.visibilityState === 'visible') open();
}

export function disconnectSSE() {
  wanted = false;
  close();
}

if (typeof window !== 'undefined') {
  window.addEventListener('online', () => online.set(true));
  window.addEventListener('offline', () => online.set(false));
  document.addEventListener('visibilitychange', () => {
    if (!wanted) return;
    if (document.visibilityState === 'visible') open();
    else close();
  });
  window.addEventListener('pagehide', close);
}
