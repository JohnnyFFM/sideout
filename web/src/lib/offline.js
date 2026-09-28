// Read caches in localStorage so the overview pages and the live screen
// open without a connection: the last fetched match payload per match and
// the small list payloads. Recordings live in IndexedDB (recstore.js);
// these caches are never a source of truth.

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
