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

const tabId = Math.random().toString(36).slice(2, 10);

// onChange(held, { handoff }): `handoff` is true when this tab became the
// writer after another tab of this browser held the role (that tab may have
// released its lease on leaving, so the new writer acquires afresh).
export function tabWriter(matchId, onChange) {
  const name = `so_scout_${matchId}`;
  let held = false;
  let released = false;
  let sawOther = false;
  const set = (v) => { if (v !== held) { held = v; onChange(v, { handoff: v && sawOther }); } };

  if (typeof navigator !== 'undefined' && navigator.locks?.request) {
    const ac = new AbortController();
    let done = null;
    navigator.locks.query?.().then((q) => { if (!held && q.held?.some((l) => l.name === name)) sawOther = true; }).catch(() => {});
    navigator.locks
      .request(name, { mode: 'exclusive', signal: ac.signal }, () => new Promise((resolve) => { done = resolve; if (!released) set(true); else resolve(); }))
      .catch(() => { /* aborted before acquisition */ });
    return {
      release() {
        released = true;
        set(false);
        if (done) done(); else ac.abort();
      }
    };
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
  return {
    release() {
      released = true;
      clearInterval(timer);
      window.removeEventListener('storage', onStorage);
      const cur = read();
      if (cur?.tab === tabId) { try { localStorage.removeItem(key); } catch { /* ignore */ } }
      set(false);
    }
  };
}
