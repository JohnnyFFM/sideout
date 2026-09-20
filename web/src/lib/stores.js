import { writable } from 'svelte/store';
import { invalidateAll } from '$app/navigation';
import { base } from '$app/paths';

/** me-shape from /api/me: { user, team, teams, members } */
export const me = writable(null);
/** module cache so the layout load does not refetch /me on every navigation */
export const meCache = { value: null };
export async function refreshMe() {
  meCache.value = null;
  await invalidateAll();
}

/** last committed mutation seen on the SSE stream (notification, not data).
 *  {entity:'resync'} is synthetic: the stream (re)connected, pages refetch. */
export const mutations = writable(null);
export const sseConnected = writable(false);
export const sseRole = writable('–'); // 'leader' | 'follower' | '–'
export const online = writable(typeof navigator === 'undefined' ? true : navigator.onLine);

export const toast = writable(null);
let toastTimer;
export function showToast(text, isErr = false) {
  toast.set({ text, isErr });
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.set(null), isErr ? 4000 : 2200);
}

/* ---------------------------------------------------------------- live stream
   Exactly ONE tab per browser holds the EventSource (Web Locks decide who)
   and relays every event to the other tabs over a BroadcastChannel.
   Browsers allow ~6 HTTP/1.1 connections per host and a stream keeps one
   open for good, so one stream per tab starved every fetch once a handful
   of tabs were open and the app looked frozen. Now the count is one. */
let source = null;
let wanted = false;
let everConnected = false;
let leader = false;
let lockRequested = false;
const bc = typeof BroadcastChannel !== 'undefined' ? new BroadcastChannel('so-sse') : null;

// the stream is scoped to the active team at connect time: whenever the
// identity refresh lands on another team (a switch, a removal, leaving),
// the stream reconnects for that team
let streamTeam = null;
me.subscribe((m) => {
  const t = m?.team?.id ?? null;
  if (t == null) return;
  if (streamTeam != null && t !== streamTeam && wanted) reconnectSSE();
  streamTeam = t;
});

function publish(d) {
  mutations.set(d);
  // a reconnect, or a change to someone's role or membership: my own role
  // and team list may have changed, so the cached identity is refetched
  if (d.entity === 'resync' || (d.entity === 'team' && /^member_/.test(d.action))) refreshMe().catch(() => {});
}

function open() {
  if (source) return;
  source = new EventSource(`${base}/api/events`);
  source.addEventListener('mutation', (ev) => {
    const d = JSON.parse(ev.data);
    publish(d);
    bc?.postMessage({ type: 'mutation', data: d });
  });
  source.onopen = () => {
    sseConnected.set(true);
    bc?.postMessage({ type: 'state', connected: true });
    if (everConnected) {
      const r = { entity: 'resync', id: 0, version: 0, action: 'resync', actor: '' };
      publish(r);
      bc?.postMessage({ type: 'mutation', data: r });
    }
    everConnected = true;
  };
  source.onerror = () => {
    sseConnected.set(false);
    bc?.postMessage({ type: 'state', connected: false });
  };
}
function close() {
  source?.close();
  source = null;
  sseConnected.set(false);
  if (leader) bc?.postMessage({ type: 'state', connected: false });
}

function becomeLeader() {
  leader = true;
  sseRole.set('leader');
  if (wanted) open();
}
function requestLeadership() {
  if (lockRequested) return;
  lockRequested = true;
  if (typeof navigator !== 'undefined' && navigator.locks) {
    sseRole.set('follower');
    bc?.postMessage({ type: 'hello' });
    // granted when no other tab holds it; held until this tab goes away
    navigator.locks.request('so-sse-leader', () => new Promise(() => becomeLeader())).catch(() => becomeLeader());
  } else {
    becomeLeader();
  }
}

bc?.addEventListener('message', (ev) => {
  const m = ev.data;
  if (m.type === 'mutation') publish(m.data);
  else if (m.type === 'state') { if (!leader) sseConnected.set(m.connected); }
  else if (m.type === 'hello') { if (leader) bc.postMessage({ type: 'state', connected: !!source && source.readyState === 1 }); }
  else if (m.type === 'reconnect') { if (leader) { close(); if (wanted) open(); } }
});

export function connectSSE() {
  wanted = true;
  requestLeadership();
  if (leader) open();
}

export function disconnectSSE() {
  wanted = false;
  close();
}

/** after switching teams: the stream is scoped to the team at connect time */
export function reconnectSSE() {
  if (leader) { close(); if (wanted) open(); }
  else bc?.postMessage({ type: 'reconnect' });
}

/** switch the active team, refresh the session and the live stream */
export async function switchTeam(teamId) {
  const { api } = await import('./api.js');
  await api('/teams/switch', { method: 'POST', body: { team_id: teamId } });
  await refreshMe(); // the team change reconnects the stream (see me.subscribe above)
}

/* ---------------------------------------------------------------- diagnostics
   Any uncaught error is shown as a toast and kept (last 10) for the
   Diagnose panel in Einstellungen. */
export const DIAG_KEY = 'so_errors';
export function readDiag() {
  try { return JSON.parse(localStorage.getItem(DIAG_KEY) || '[]'); } catch { return []; }
}
function logError(kind, msg) {
  const e = { t: new Date().toISOString(), kind, msg: String(msg || '').slice(0, 300), path: location.pathname };
  try { localStorage.setItem(DIAG_KEY, JSON.stringify(readDiag().concat([e]).slice(-10))); } catch { /* ignore */ }
  showToast('Fehler: ' + e.msg, true);
}

if (typeof window !== 'undefined') {
  window.addEventListener('online', () => { online.set(true); refreshMe().catch(() => {}); }); // revalidate the offline identity
  window.addEventListener('offline', () => online.set(false));
  window.addEventListener('pagehide', close);
  // safety net independent of the stream: a tab that comes back into view
  // refetches what it shows (a phone that slept, a laptop lid, a stale tab)
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible' && wanted) publish({ entity: 'resync', id: 0, version: 0, action: 'visible', actor: '' });
  });
  window.addEventListener('error', (ev) => logError('error', ev.message));
  window.addEventListener('unhandledrejection', (ev) => logError('promise', ev.reason?.message || ev.reason));
}
