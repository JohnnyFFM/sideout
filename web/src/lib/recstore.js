// The device's recordings in IndexedDB, shared by every tab of this
// browser profile (no copy per tab). Every edit is written with its number
// in one transaction before the UI calls it saved; the outbox is simply
// "edit number > confirmed number" per recording. Nothing here is ever
// deleted by the app.
//
//   recordings  key id        { id, match_id, team_id, user_id, device_id, device_label,
//                               origin_id, origin_n, base, next_n, confirmed_n, created,
//                               error, imported, created_at, updated_at }
//   edits       key [id, n]   { recording_id, n, body, t }
//   meta        key name      { name, value }

const DB = 'sideout';
const VERSION = 1;
let dbp = null;

export function hasStorage() {
  return typeof indexedDB !== 'undefined';
}

export function openDb() {
  if (dbp) return dbp;
  dbp = new Promise((resolve, reject) => {
    if (!hasStorage()) { reject(new Error('IndexedDB nicht verfügbar')); return; }
    const req = indexedDB.open(DB, VERSION);
    req.onupgradeneeded = () => {
      const db = req.result;
      const recs = db.createObjectStore('recordings', { keyPath: 'id' });
      recs.createIndex('match', 'match_id');
      const edits = db.createObjectStore('edits', { keyPath: ['recording_id', 'n'] });
      edits.createIndex('rec', 'recording_id');
      db.createObjectStore('meta', { keyPath: 'name' });
    };
    req.onsuccess = () => {
      const db = req.result;
      db.onversionchange = () => { db.close(); dbp = null; };
      resolve(db);
    };
    req.onerror = () => { dbp = null; reject(req.error || new Error('IndexedDB öffnen fehlgeschlagen')); };
    req.onblocked = () => reject(new Error('IndexedDB blockiert'));
  });
  return dbp;
}

const wrap = (req) => new Promise((resolve, reject) => { req.onsuccess = () => resolve(req.result); req.onerror = () => reject(req.error); });
const done = (tx) => new Promise((resolve, reject) => { tx.oncomplete = () => resolve(); tx.onerror = () => reject(tx.error); tx.onabort = () => reject(tx.error || new Error('abgebrochen')); });

async function metaGet(name) {
  const db = await openDb();
  const r = await wrap(db.transaction('meta').objectStore('meta').get(name));
  return r?.value;
}
async function metaSet(name, value) {
  const db = await openDb();
  const tx = db.transaction('meta', 'readwrite');
  tx.objectStore('meta').put({ name, value });
  await done(tx);
}
export { metaGet, metaSet };

const rnd = () => { try { return crypto.randomUUID(); } catch { return Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 10); } };
export const newId = () => rnd().replace(/[^A-Za-z0-9_-]/g, '');

let devId = null;
let devIdP = null;
/** stable per browser profile; a cleared storage means a new device.
 *  Single-flight: concurrent first calls must not mint two ids. */
export function deviceId() {
  if (devId) return Promise.resolve(devId);
  if (devIdP) return devIdP;
  devIdP = (async () => {
    let v = await metaGet('device_id');
    if (!v) { v = 'd-' + newId().slice(0, 20); await metaSet('device_id', v); v = (await metaGet('device_id')) || v; }
    devId = v;
    return v;
  })().finally(() => { devIdP = null; });
  return devIdP;
}

export function deviceLabel() {
  const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  if (/iPhone/i.test(ua)) return 'iPhone';
  if (/iPad|Macintosh.*Mobile/i.test(ua)) return 'iPad';
  if (/Android/i.test(ua)) return 'Android';
  if (/Windows/i.test(ua)) return 'Windows';
  if (/Macintosh/i.test(ua)) return 'Mac';
  if (/Linux/i.test(ua)) return 'Linux';
  return 'Gerät';
}

export async function getRecording(id) {
  const db = await openDb();
  return (await wrap(db.transaction('recordings').objectStore('recordings').get(id))) || null;
}

export async function recordingsOf(matchId) {
  const db = await openDb();
  return wrap(db.transaction('recordings').objectStore('recordings').index('match').getAll(Number(matchId)));
}

/** this device's own (editable) recording of a match for the signed-in
 *  account, the newest if several. Another account's recording on the same
 *  browser is never continued. */
export async function myRecording(matchId, userId) {
  const dev = await deviceId();
  const all = await recordingsOf(matchId);
  const mine = all.filter((r) => r.device_id === dev && !r.imported && !r.deleted && r.user_id != null && r.user_id === userId).sort((a, b) => (a.created_at < b.created_at ? 1 : -1));
  return mine[0] || null;
}

export async function allRecordings() {
  const db = await openDb();
  return wrap(db.transaction('recordings').objectStore('recordings').getAll());
}

/** base + first edit in one transaction: a recording never exists without its first tap */
export async function createRecording({ match_id, team_id = null, user_id = null, base, origin_id = null, origin_n = null, firstEdit }) {
  const db = await openDb();
  const now = new Date().toISOString();
  const rec = {
    id: newId(), match_id: Number(match_id), team_id, user_id,
    device_id: await deviceId(), device_label: '',
    origin_id, origin_n, base, next_n: firstEdit ? 2 : 1, confirmed_n: 0, created: false, error: null, imported: false,
    created_at: now, updated_at: now
  };
  const tx = db.transaction(['recordings', 'edits'], 'readwrite');
  tx.objectStore('recordings').add(rec);
  if (firstEdit) tx.objectStore('edits').add({ recording_id: rec.id, n: 1, body: firstEdit, t: now });
  await done(tx);
  notify('data');
  return rec;
}

/** append one edit; the number is taken and advanced inside the transaction */
export async function appendEdit(recId, body) {
  const db = await openDb();
  const tx = db.transaction(['recordings', 'edits'], 'readwrite');
  const recs = tx.objectStore('recordings');
  const rec = await wrap(recs.get(recId));
  if (!rec) throw new Error('Aufzeichnung fehlt');
  const n = rec.next_n;
  rec.next_n = n + 1;
  rec.updated_at = new Date().toISOString();
  recs.put(rec);
  tx.objectStore('edits').add({ recording_id: recId, n, body, t: rec.updated_at });
  await done(tx);
  notify('data');
  return n;
}

export async function editsFrom(recId, fromN = 1, limit = Infinity) {
  const db = await openDb();
  const out = [];
  const range = IDBKeyRange.bound([recId, fromN], [recId, Infinity]);
  await new Promise((resolve, reject) => {
    const req = db.transaction('edits').objectStore('edits').openCursor(range);
    req.onsuccess = () => {
      const c = req.result;
      if (!c || out.length >= limit) { resolve(); return; }
      out.push(c.value); c.continue();
    };
    req.onerror = () => reject(req.error);
  });
  return out;
}

export async function markConfirmed(recId, n, { created = true } = {}) {
  const db = await openDb();
  const tx = db.transaction('recordings', 'readwrite');
  const recs = tx.objectStore('recordings');
  const rec = await wrap(recs.get(recId));
  if (rec) {
    rec.confirmed_n = Math.max(rec.confirmed_n, n);
    rec.created = rec.created || created;
    rec.error = null;
    recs.put(rec);
  }
  await done(tx);
  notify('status');
}

/** deleted on the server (by its creator or a coach): hidden here too, never continued, never uploaded again */
export async function setDeleted(recId) {
  const db = await openDb();
  const tx = db.transaction('recordings', 'readwrite');
  const recs = tx.objectStore('recordings');
  const rec = await wrap(recs.get(recId));
  if (rec) { rec.deleted = true; recs.put(rec); }
  await done(tx);
  notify('status');
}

export async function setError(recId, error) {
  const db = await openDb();
  const tx = db.transaction('recordings', 'readwrite');
  const recs = tx.objectStore('recordings');
  const rec = await wrap(recs.get(recId));
  if (rec) { rec.error = error; recs.put(rec); }
  await done(tx);
  notify('status');
}

/** edits not yet confirmed by the server (the base travels with the first batch) */
export const pendingOf = (r) => r.next_n - 1 - r.confirmed_n;
export const needsUpload = (r) => !r.deleted && (pendingOf(r) > 0 || !r.created);

/** what the signed-in account may send: its own recordings and imports (an explicit exception) */
export const ownedBy = (r, userId) => r.imported || (userId != null && r.user_id === userId);

/** recordings with something to upload under this account, oldest first */
export async function pendingRecordings(userId) {
  const all = await allRecordings();
  return all.filter((r) => needsUpload(r) && ownedBy(r, userId)).sort((a, b) => (a.created_at < b.created_at ? -1 : 1));
}

/** { total, byMatch: { id: count }, errors: [{ id, match_id, error }], foreign }
 *  for this account; `foreign` counts pending recordings of other accounts on this device */
export async function pendingSummary(userId) {
  const all = await allRecordings();
  const byMatch = {};
  let total = 0;
  let foreign = 0;
  const errors = [];
  for (const r of all) {
    if (r.deleted) continue;
    const p = pendingOf(r) || (r.created ? 0 : 1);
    if (!ownedBy(r, userId)) { if (p > 0) foreign++; continue; }
    if (p > 0) { byMatch[r.match_id] = (byMatch[r.match_id] || 0) + p; total += p; }
    if (r.error) errors.push({ id: r.id, match_id: r.match_id, error: r.error });
  }
  return { total, byMatch, errors, foreign };
}

/** JSON with sorted object keys: content identity on the client (arrays keep their order) */
export function stable(v) {
  if (Array.isArray(v)) return '[' + v.map(stable).join(',') + ']';
  if (v && typeof v === 'object') return '{' + Object.keys(v).sort().map((k) => JSON.stringify(k) + ':' + stable(v[k])).join(',') + '}';
  return JSON.stringify(v === undefined ? null : v);
}

/** a recording brought in from a file or from the old queue: stored as is
 *  (foreign device id), uploaded like any other, never edited here. An id
 *  the device already holds is merged: the base and the common prefix of
 *  edits must be identical, missing edits are appended, anything else is
 *  refused. Returns { id, existed, added }. */
export async function importRecording(rec, edits) {
  const db = await openDb();
  const tx = db.transaction(['recordings', 'edits'], 'readwrite');
  const recs = tx.objectStore('recordings');
  const existing = await wrap(recs.get(rec.id));
  const now = new Date().toISOString();
  if (existing) {
    if (stable(existing.base) !== stable(rec.base)) { tx.abort(); throw new Error('Aufzeichnung mit gleicher ID, aber anderem Anfangsstand'); }
    const have = await wrap(tx.objectStore('edits').index('rec').getAll(rec.id));
    have.sort((a, b) => a.n - b.n);
    const bodies = edits.map((e) => e.body ?? e);
    for (let i = 0; i < Math.min(have.length, bodies.length); i++) {
      if (stable(have[i].body) !== stable(bodies[i])) { tx.abort(); throw new Error(`Aufzeichnung mit gleicher ID, aber anderer Änderung Nr. ${i + 1}`); }
    }
    let added = 0;
    const es = tx.objectStore('edits');
    for (let i = have.length; i < bodies.length; i++) {
      es.add({ recording_id: rec.id, n: i + 1, body: bodies[i], t: edits[i]?.t || now });
      added++;
    }
    if (added) {
      existing.next_n = Math.max(existing.next_n, bodies.length + 1);
      existing.updated_at = now;
      existing.error = null;
      recs.put(existing);
    }
    await done(tx);
    if (added) notify('data');
    return { id: rec.id, existed: true, added };
  }
  const n = edits.length;
  recs.add({
    id: rec.id, match_id: Number(rec.match_id), team_id: rec.team_id ?? null, user_id: rec.user_id ?? null,
    device_id: rec.device_id || 'import', device_label: rec.device_label || 'Import',
    origin_id: rec.origin_id ?? null, origin_n: rec.origin_n ?? null, base: rec.base,
    next_n: n + 1, confirmed_n: 0, created: false, error: null, imported: true,
    created_at: rec.created_at || now, updated_at: now
  });
  const es = tx.objectStore('edits');
  edits.forEach((e, i) => es.add({ recording_id: rec.id, n: i + 1, body: e.body ?? e, t: e.t || now }));
  await done(tx);
  notify('data');
  return { id: rec.id, existed: false, added: n };
}

/** a recording this device takes over as its own (the match's migrated
 *  recording from the server, or a fresh one): stored with the given
 *  confirmed state, new edits appended behind it, continued from now on */
export async function adoptRecording({ id, match_id, base, edits = [], confirmed = 0, userId, newEdits = [], origin_id = null, origin_n = null, created = true }) {
  const db = await openDb();
  const dev = await deviceId(); // before the transaction: an await inside it would let it auto-commit
  const now = new Date().toISOString();
  const tx = db.transaction(['recordings', 'edits'], 'readwrite');
  const recs = tx.objectStore('recordings');
  if (await wrap(recs.get(id))) { tx.abort(); throw new Error('Aufzeichnung schon vorhanden'); }
  const all = [...edits, ...newEdits];
  recs.add({
    id, match_id: Number(match_id), team_id: null, user_id: userId,
    device_id: dev, device_label: '',
    origin_id, origin_n, base, next_n: all.length + 1, confirmed_n: Math.min(confirmed, edits.length), created,
    error: null, imported: false, created_at: now, updated_at: now
  });
  const es = tx.objectStore('edits');
  all.forEach((body, i) => es.add({ recording_id: id, n: i + 1, body, t: now }));
  await done(tx);
  notify('data');
  return id;
}

/** everything of one recording, for export */
export async function exportRecording(id) {
  const rec = await getRecording(id);
  if (!rec) return null;
  const edits = await editsFrom(id, 1);
  return {
    format: 'sideout-recording', schema: SCHEMA_FILE, exported_at: new Date().toISOString(),
    recording: {
      id: rec.id, match_id: rec.match_id, team_id: rec.team_id, user_id: rec.user_id,
      device_id: rec.device_id, device_label: rec.device_label, origin_id: rec.origin_id, origin_n: rec.origin_n,
      base: rec.base, created_at: rec.created_at, confirmed_n: rec.confirmed_n
    },
    edits: edits.map((e) => ({ n: e.n, body: e.body, t: e.t }))
  };
}
export const SCHEMA_FILE = 1;

// change notifications for stores and pages (same tab: listeners; other
// tabs: BroadcastChannel). kind 'data' = new recording or edit (the
// uploader has work), 'status' = confirmed/error changed (only counts)
const listeners = new Set();
const bc = typeof BroadcastChannel !== 'undefined' ? new BroadcastChannel('so-recstore') : null;
bc?.addEventListener('message', (ev) => { for (const l of [...listeners]) l(ev.data?.kind || 'data'); });
function notify(kind) {
  for (const l of [...listeners]) l(kind);
  bc?.postMessage({ kind });
}
export function onChange(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
