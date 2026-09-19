// Offline-first queue for the live screen.
// The page renders from server-confirmed actions + a local op queue
// ({type:'add', action} | {type:'undo'}). Ops are flushed in order; a
// retried POST with the same seq is idempotent on the server, a 409 means
// another device appended first — we reload and drop the queue.

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

/** confirmed actions + queued ops → the list the engine replays */
export function applyOps(actions, ops) {
  const out = actions.slice();
  for (const op of ops) {
    if (op.type === 'add') out.push(op.action);
    else if (op.type === 'undo') out.pop();
  }
  return out;
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
