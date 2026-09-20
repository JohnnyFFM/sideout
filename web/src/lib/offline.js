// Offline-first queue for the live screen.
// The page renders from server-confirmed actions + a local op queue
// ({type:'add', action} | {type:'undo', cid, seq}). Ops are flushed in
// order. Every queued action carries a client id (`cid`) that the server
// stores, so after a lost response or a reload the queue can be reconciled
// against the confirmed log instead of guessing: an add whose cid the server
// already holds is done, an undo whose target is gone is done.

const key = (matchId) => `so_ops_${matchId}`;

export function loadOps(matchId) {
  try {
    const raw = localStorage.getItem(key(matchId));
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

export function saveOps(matchId, ops) {
  try {
    if (ops.length) localStorage.setItem(key(matchId), JSON.stringify(ops));
    else localStorage.removeItem(key(matchId));
  } catch {
    /* quota / private mode: the queue lives in memory only */
  }
}

export function newCid() {
  try {
    return crypto.randomUUID();
  } catch {
    return Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 10);
  }
}

const sameCid = (a, op) => (a?.cid && op?.cid ? a.cid === op.cid : a?.seq === op?.seq);

/** confirmed actions + queued ops → the list the engine replays.
 *  An add the server already holds is not appended twice; an undo only
 *  removes the action it was aimed at. */
export function applyOps(actions, ops) {
  const out = actions.slice();
  for (const op of ops) {
    if (op.type === 'add') {
      if (!out.some((a) => a.cid && a.cid === op.action.cid)) out.push(op.action);
    } else if (op.type === 'undo') {
      const top = out[out.length - 1];
      if (top && (op.cid == null && op.seq == null ? true : sameCid(top, op))) out.pop();
    }
  }
  return out;
}

/**
 * reconcileOps(actions, ops, knownTop) — after a (re)load: drop the ops the
 * server already reflects, renumber the rest onto the confirmed log.
 *   actions   the confirmed log from the server
 *   ops       the local queue
 *   knownTop  the last confirmed seq this device knew before the load
 * Returns { ops, conflict, dropped }. `conflict` means another device wrote
 * actions we did not know about while we still had unsent ops: those ops
 * cannot be applied safely and are returned in `dropped`.
 */
export function reconcileOps(actions, ops, knownTop = 0) {
  const confirmed = new Set(actions.map((a) => a.cid).filter(Boolean));
  const ours = new Set(ops.filter((o) => o.type === 'add').map((o) => o.action.cid));
  const foreign = actions.some((a) => a.seq > knownTop && !(a.cid && ours.has(a.cid)));
  const sim = actions.slice();
  const out = [];
  for (const op of ops) {
    if (op.type === 'add') {
      if (op.action.cid && confirmed.has(op.action.cid)) continue; // saved, answer got lost
      const seq = (sim[sim.length - 1]?.seq || 0) + 1;
      const action = op.action.seq === seq ? op.action : { ...op.action, seq, id: -seq };
      out.push({ type: 'add', action, sent: false }); // not on the server: it goes out fresh
      sim.push(action);
    } else if (op.type === 'undo') {
      const top = sim[sim.length - 1];
      if (!top) continue;
      const stillThere = op.cid ? sim.some((a) => a.cid === op.cid) : op.seq == null || top.seq >= op.seq;
      if (!stillThere) continue; // the target is gone: undone already, or its add was dropped
      if (!sameCid(top, op)) return { ops: [], conflict: true, dropped: ops }; // something sits on top of it
      // undo of our own add that is not on the server (we just fetched the
      // log, so absence is certain): the pair cancels out
      const k = out.findIndex((o) => o.type === 'add' && o.action.cid && o.action.cid === op.cid);
      if (k >= 0) out.splice(k, 1);
      else out.push({ type: 'undo', cid: top.cid ?? null, seq: top.seq });
      sim.pop();
    }
  }
  if (out.length && foreign) return { ops: [], conflict: true, dropped: ops };
  return { ops: out, conflict: false, dropped: [] };
}

const dkey = (matchId) => `so_ops_dropped_${matchId}`;
/** ops that could not be sent (another device scouted meanwhile) are kept for the record */
export function keepDropped(matchId, ops) {
  try {
    localStorage.setItem(dkey(matchId), JSON.stringify({ at: new Date().toISOString(), ops }));
  } catch {
    /* ignore */
  }
}
export function droppedOps(matchId) {
  try {
    const raw = localStorage.getItem(dkey(matchId));
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

const mkey = (matchId) => `so_match_${matchId}`;
/** last fetched match payload, so the live page opens offline */
export function cacheMatch(matchId, match) {
  try {
    localStorage.setItem(mkey(matchId), JSON.stringify(match));
  } catch {
    /* ignore */
  }
}
export function cachedMatch(matchId) {
  try {
    const raw = localStorage.getItem(mkey(matchId));
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

/** a match is "offline ready" when its cached payload is as new as the list says */
export function offlineReady(m) {
  const c = cachedMatch(m.id);
  return !!c && c.version === m.version && (c.actions?.length || 0) >= (m.state?.last_seq ?? 0);
}

const lkey = (name) => `so_list_${name}`;
/** small list payloads (matches, roster) so the overview pages open offline */
export function cacheList(name, data) {
  try {
    localStorage.setItem(lkey(name), JSON.stringify(data));
  } catch {
    /* ignore */
  }
}
export function cachedList(name) {
  try {
    const raw = localStorage.getItem(lkey(name));
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}
