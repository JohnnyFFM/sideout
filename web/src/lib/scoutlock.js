// One writer tab per match in this browser. Tabs share the session cookie
// and the localStorage queue but not component memory, so without this two
// tabs on the same live page would both flush the same queue and either
// could release the other's lease. The SSE leader lock is a different lock
// with a different job.
//
// Web Locks when available (the lock is held for the page's lifetime and
// handed to the next waiting tab when it goes away); otherwise a
// localStorage heartbeat: the writer refreshes `so_tab_<id>` every 2 s and
// a tab that sees no beat younger than 6 s takes over.
//
// The holder is shared per match within the tab and released one tick after
// its last user let go: the lineup editor navigating to the live page (or
// the other way round) keeps the writer role instead of handing it to a tab
// that was waiting for the lock.

const tabId = Math.random().toString(36).slice(2, 10);
const holders = new Map(); // matchId → { held, handoff, listeners, users, release, pending }

function createHolder(matchId) {
  const name = `so_scout_${matchId}`;
  const h = { held: false, handoff: false, listeners: new Set(), users: 0, pending: null, release: null };
  let released = false;
  let sawOther = false;
  const set = (v) => {
    if (v === h.held) return;
    h.held = v; h.handoff = v && sawOther;
    for (const l of [...h.listeners]) l(v, { handoff: h.handoff });
  };

  if (typeof navigator !== 'undefined' && navigator.locks?.request) {
    const ac = new AbortController();
    let done = null;
    navigator.locks.query?.().then((q) => { if (!h.held && q.held?.some((l) => l.name === name)) sawOther = true; }).catch(() => {});
    navigator.locks
      .request(name, { mode: 'exclusive', signal: ac.signal }, () => new Promise((resolve) => { done = resolve; if (!released) set(true); else resolve(); }))
      .catch(() => { /* aborted before acquisition */ });
    h.release = () => { released = true; set(false); if (done) done(); else ac.abort(); };
    return h;
  }

  // fallback: heartbeat in localStorage
  const key = `so_tab_${matchId}`;
  const read = () => { try { return JSON.parse(localStorage.getItem(key) || 'null'); } catch { return null; } };
  const beat = () => {
    if (released) return;
    const cur = read();
    const fresh = cur && Date.now() - cur.t < 6000;
    if (!fresh || cur.tab === tabId) {
      try { localStorage.setItem(key, JSON.stringify({ tab: tabId, t: Date.now() })); } catch { /* ignore */ }
      set(true);
    } else {
      sawOther = true;
      set(false);
    }
  };
  beat();
  const timer = setInterval(beat, 2000);
  const onStorage = (e) => { if (e.key === key) beat(); };
  window.addEventListener('storage', onStorage);
  h.release = () => {
    released = true;
    clearInterval(timer);
    window.removeEventListener('storage', onStorage);
    const cur = read();
    if (cur?.tab === tabId) { try { localStorage.removeItem(key); } catch { /* ignore */ } }
    set(false);
  };
  return h;
}

// onChange(held, { handoff }): `handoff` is true when this tab became the
// writer after another tab of this browser held the role (that tab may have
// released its lease on leaving, so the new writer acquires afresh).
export function tabWriter(matchId, onChange) {
  let h = holders.get(matchId);
  if (h?.pending) { clearTimeout(h.pending); h.pending = null; }
  if (!h) { h = createHolder(matchId); holders.set(matchId, h); }
  h.users++;
  h.listeners.add(onChange);
  if (h.held) queueMicrotask(() => { if (h.listeners.has(onChange)) onChange(true, { handoff: h.handoff }); });
  let done = false;
  return {
    release() {
      if (done) return;
      done = true;
      h.listeners.delete(onChange);
      h.users--;
      if (h.users > 0) return;
      // last user gone: let go a tick later, unless a new page of this tab takes over
      h.pending = setTimeout(() => {
        if (h.users > 0) return;
        holders.delete(matchId);
        h.release();
      }, 0);
    }
  };
}
