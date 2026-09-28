// One uploader per browser profile (Web Lock), independent of the open
// page: whatever recording of the signed-in account (or an imported one)
// has edits beyond its confirmed number goes out in small batches, oldest
// recording first. Runs on start, on login, when the tab returns, on the
// online event, after every saved edit and on a timer while anything is
// pending. Errors never delete anything and never block the other
// recordings: a dead connection or a missing login ends the pass (it
// affects everything), a 5xx puts that one recording on a growing pause,
// a 4xx is recorded on the recording, shown, and retried much later or on
// an explicit "Jetzt hochladen".

import { get, writable } from 'svelte/store';
import { api } from './api.js';
import { pendingRecordings, editsFrom, markConfirmed, setError, pendingSummary, onChange, hasStorage } from './recstore.js';
import { logDiag, me, refreshMe } from './stores.js';

/** { total, byMatch, errors, foreign, uploading, transient, supported, unsupportedReason } */
export const sync = writable({ total: 0, byMatch: {}, errors: [], foreign: 0, uploading: false, transient: '', supported: true, unsupportedReason: '' });

const BATCH = 200;
const PERMANENT_PAUSE = 10 * 60 * 1000;
let started = false;
let running = false;
let again = false;
let timer = null;
let delay = 5000;
/** per-recording pause after a failure: id → { until, delay } */
const paused = new Map();

export const locksSupported = () => typeof navigator !== 'undefined' && !!navigator.locks?.request;
export const supported = () => hasStorage() && locksSupported();
const userId = () => get(me)?.user?.id ?? null;

export async function refreshSummary() {
  try {
    const s = await pendingSummary(userId());
    sync.update((v) => ({ ...v, ...s }));
  } catch (e) {
    logDiag('store', e?.message || e);
  }
}

/** ask for a run soon (coalesced); `force` lifts every per-recording pause */
export function kick(opts = {}) {
  if (!started) return;
  if (opts === true || opts?.force) paused.clear();
  clearTimeout(timer);
  timer = setTimeout(run, 60);
}

function later(ms) {
  clearTimeout(timer);
  timer = setTimeout(run, ms);
}

function pause(id, ms) {
  paused.set(id, { until: Date.now() + ms, delay: ms });
}

async function uploadOne(rec, uid) {
  const edits = await editsFrom(rec.id, rec.confirmed_n + 1, BATCH);
  const body = {
    uploader: uid,
    device_id: rec.device_id, device_label: rec.device_label,
    origin_id: rec.origin_id, origin_n: rec.origin_n,
    edits: edits.map((e) => ({ n: e.n, body: e.body }))
  };
  if (!rec.created) body.base = rec.base;
  try {
    const res = await api(`/matches/${rec.match_id}/recordings/${rec.id}`, { method: 'PUT', body, timeout: 30000 });
    const sentTo = edits.length ? edits[edits.length - 1].n : rec.confirmed_n;
    // confirmed only what this upload actually put in front of the server:
    // a later import may add edits beyond that, and they must be compared too
    await markConfirmed(rec.id, Math.min(res.confirmed, sentTo));
    if (res.confirmed < sentTo && res.confirmed <= rec.confirmed_n && edits.length) {
      // the server stored none of what we sent although the numbers should continue: a hole on our side
      await setError(rec.id, `Server bestätigt nur bis ${res.confirmed}, lokal fehlt Nr. ${res.confirmed + 1}`);
      pause(rec.id, PERMANENT_PAUSE);
      return 'defer';
    }
    paused.delete(rec.id);
    return 'progress';
  } catch (e) {
    if (e.offline || e.status === 401) {
      sync.update((v) => ({ ...v, transient: e.offline ? 'keine Verbindung' : 'Anmeldung nötig' }));
      return 'stop';
    }
    if (e.code === 'account_mismatch') {
      // the session cookie belongs to someone else now (another tab switched
      // accounts): nothing more goes out under this identity, the app
      // re-reads who it is and the next pass works for that account
      sync.update((v) => ({ ...v, transient: 'Konto gewechselt' }));
      refreshMe().catch(() => {});
      return 'stop';
    }
    if (e.status >= 500) {
      const prev = paused.get(rec.id)?.delay || 0;
      pause(rec.id, Math.min(Math.max(prev * 2, 5000), 60000));
      sync.update((v) => ({ ...v, transient: 'Server antwortet nicht' }));
      return 'defer';
    }
    await setError(rec.id, e.message || `HTTP ${e.status}`);
    pause(rec.id, PERMANENT_PAUSE);
    logDiag('upload', `${rec.id}: ${e.message || e.status}`);
    return 'defer';
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
      const uid = userId();
      if (uid == null) return; // nobody signed in: nothing goes out under a guessed identity
      sync.update((v) => ({ ...v, uploading: true }));
      let guard = 0;
      while (guard++ < 500) {
        const now = Date.now();
        const recs = (await pendingRecordings(uid)).filter((r) => !(paused.get(r.id)?.until > now));
        if (!recs.length) break;
        let progressed = false;
        for (const rec of recs) {
          // the identity this pass started with must still be the app's identity
          if (userId() !== uid) { stopped = true; break; }
          const r = await uploadOne(rec, uid);
          if (r === 'progress') progressed = true;
          if (r === 'stop') { stopped = true; break; }
        }
        if (stopped || !progressed) break;
      }
      if (!stopped) { delay = 5000; if (![...paused.values()].some((p) => p.delay < PERMANENT_PAUSE)) sync.update((v) => ({ ...v, transient: '' })); }
    });
  } catch (e) {
    logDiag('upload', e?.message || e);
    stopped = true;
  } finally {
    running = false;
    sync.update((v) => ({ ...v, uploading: false }));
    await refreshSummary();
    if (stopped) { later(delay); delay = Math.min(delay * 2, 60000); }
    else {
      // wake up when the earliest pause ends
      const next = Math.min(...[...paused.values()].map((p) => p.until));
      if (Number.isFinite(next)) later(Math.max(500, next - Date.now()));
    }
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
  window.addEventListener('online', () => kick({ force: true }));
  window.addEventListener('focus', () => kick());
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') kick(); });
  // new data nudges the uploader; a status change (confirmed, error) only refreshes the counts
  onChange((kind) => { refreshSummary(); if (kind === 'data') kick(); });
  me.subscribe(() => { refreshSummary(); kick(); });
  // the retry timer: while anything is pending, try again every half minute
  setInterval(async () => { const s = await pendingSummary(userId()).catch(() => null); if (s?.total) kick(); }, 30000);
  refreshSummary();
  kick();
}
