// One uploader per browser profile (Web Lock), independent of the open
// page: whatever recording has edits beyond its confirmed number goes out
// in small batches, oldest recording first. Runs on start, on login, when
// the tab returns, on the online event, after every saved edit and on a
// timer while anything is pending. Errors never delete anything: a dead
// connection or a 5xx retries with backoff, a 4xx is recorded on the
// recording and shown; the other recordings keep uploading.

import { writable } from 'svelte/store';
import { api } from './api.js';
import { pendingRecordings, editsFrom, markConfirmed, setError, pendingSummary, onChange, hasStorage } from './recstore.js';
import { logDiag } from './stores.js';

/** { total, byMatch, errors, uploading, transient, supported } */
export const sync = writable({ total: 0, byMatch: {}, errors: [], uploading: false, transient: '', supported: true, unsupportedReason: '' });

const BATCH = 200;
let started = false;
let running = false;
let again = false;
let timer = null;
let delay = 5000;

export const locksSupported = () => typeof navigator !== 'undefined' && !!navigator.locks?.request;
export const supported = () => hasStorage() && locksSupported();

export async function refreshSummary() {
  try {
    const s = await pendingSummary();
    sync.update((v) => ({ ...v, ...s }));
  } catch (e) {
    logDiag('store', e?.message || e);
  }
}

/** ask for a run soon (coalesced) */
export function kick() {
  if (!started) return;
  clearTimeout(timer);
  timer = setTimeout(run, 60);
}

function later() {
  clearTimeout(timer);
  timer = setTimeout(run, delay);
  delay = Math.min(delay * 2, 60000);
}

async function uploadOne(rec) {
  const edits = await editsFrom(rec.id, rec.confirmed_n + 1, BATCH);
  const body = {
    device_id: rec.device_id, device_label: rec.device_label,
    origin_id: rec.origin_id, origin_n: rec.origin_n,
    edits: edits.map((e) => ({ n: e.n, body: e.body }))
  };
  if (!rec.created) body.base = rec.base;
  try {
    const res = await api(`/matches/${rec.match_id}/recordings/${rec.id}`, { method: 'PUT', body, timeout: 30000 });
    const sentTo = edits.length ? edits[edits.length - 1].n : rec.confirmed_n;
    await markConfirmed(rec.id, res.confirmed);
    if (res.confirmed < sentTo && res.confirmed <= rec.confirmed_n && edits.length) {
      // the server stored none of what we sent although the numbers should continue: a hole on our side
      await setError(rec.id, `Server bestätigt nur bis ${res.confirmed}, lokal fehlt Nr. ${res.confirmed + 1}`);
      return 'error';
    }
    return 'progress';
  } catch (e) {
    if (e.offline || e.status >= 500 || e.status === 401) {
      sync.update((v) => ({ ...v, transient: e.offline ? 'keine Verbindung' : e.status === 401 ? 'Anmeldung nötig' : 'Server antwortet nicht' }));
      return 'stop';
    }
    await setError(rec.id, e.message || `HTTP ${e.status}`);
    logDiag('upload', `${rec.id}: ${e.message || e.status}`);
    return 'error';
  }
}

async function run() {
  if (!started) return;
  if (running) { again = true; return; }
  running = true;
  let stopped = false;
  try {
    await navigator.locks.request('so-uploader', { ifAvailable: true }, async (lock) => {
      if (!lock) return; // another tab is uploading the same store
      sync.update((v) => ({ ...v, uploading: true }));
      let guard = 0;
      while (guard++ < 500) {
        const recs = await pendingRecordings();
        if (!recs.length) break;
        let progressed = false;
        for (const rec of recs) {
          const r = await uploadOne(rec);
          if (r === 'progress') progressed = true;
          if (r === 'stop') { stopped = true; break; }
        }
        if (stopped || !progressed) break;
      }
      if (!stopped) { delay = 5000; sync.update((v) => ({ ...v, transient: '' })); }
    });
  } catch (e) {
    logDiag('upload', e?.message || e);
    stopped = true;
  } finally {
    running = false;
    sync.update((v) => ({ ...v, uploading: false }));
    await refreshSummary();
    if (stopped) later();
    if (again) { again = false; kick(); }
  }
}

/** called once from the layout in the browser */
export function startUploader() {
  if (started || typeof window === 'undefined') return;
  if (!supported()) {
    sync.update((v) => ({ ...v, supported: false, unsupportedReason: !hasStorage() ? 'Dieser Browser hat keinen lokalen Speicher (IndexedDB).' : 'Dieser Browser kennt keine Web Locks. Bitte Browser aktualisieren (Safari ab 15.4).' }));
    return;
  }
  started = true;
  window.addEventListener('online', kick);
  window.addEventListener('focus', kick);
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') kick(); });
  onChange(() => { refreshSummary(); kick(); });
  // the retry timer: while anything is pending, try again every half minute
  setInterval(async () => { const s = await pendingSummary().catch(() => null); if (s?.total) kick(); }, 30000);
  refreshSummary();
  kick();
}
