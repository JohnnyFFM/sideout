// One writer tab per match in this browser (Web Locks). Tabs share the
// session cookie and the IndexedDB recordings but not component memory, so
// without this two tabs on the same live page could hand out the same edit
// number twice. The lock is held for the page's lifetime and passes to the
// next waiting tab when this one goes away; the browser releases it when a
// tab dies. No fallback: without Web Locks the app does not scout and says
// so (Safari 15.4+, Chrome 69+, Firefox 96+).
//
// The holder is shared per match within the tab and released one tick after
// its last user let go: the lineup editor navigating to the live page (or
// the other way round) keeps the writer role instead of handing it to a tab
// that was waiting for the lock.

const holders = new Map(); // matchId → { held, handoff, listeners, users, release, pending }

export const locksSupported = () => typeof navigator !== 'undefined' && !!navigator.locks?.request;

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
  if (!locksSupported()) {
    h.release = () => {};
    queueMicrotask(() => { for (const l of [...h.listeners]) l(false, { unsupported: true }); });
    return h;
  }
  const ac = new AbortController();
  let done = null;
  navigator.locks.query?.().then((q) => { if (!h.held && q.held?.some((l) => l.name === name)) sawOther = true; }).catch(() => {});
  navigator.locks
    .request(name, { mode: 'exclusive', signal: ac.signal }, () => new Promise((resolve) => { done = resolve; if (!released) set(true); else resolve(); }))
    .catch(() => { /* aborted before acquisition */ });
  h.release = () => { released = true; set(false); if (done) done(); else ac.abort(); };
  return h;
}

// onChange(held, { handoff, unsupported }): `handoff` is true when this tab
// became the writer after another tab of this browser held the role.
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
