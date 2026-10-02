// One-time move of what the old client left in localStorage: unsent op
// queues (`so_ops_<match>`) and set-aside queues (`so_ops_dropped_<match>`),
// together with the match payload it had cached (`so_match_<match>`).
//
// In the old model those ops were the continuation of the match's one
// shared log, so they become a continuation here too, never extra rows:
//   1. the server's migrated recording of the match (`legacy-<match>`)
//      exists → it is adopted by this device (base and edits fetched, the
//      old ops appended as new edits) and continued from now on;
//   2. else this device already records the match → the ops are appended
//      to that recording;
//   3. else a new own recording is started from the cached match with the
//      ops as its first edits.
// Needs the server for case 1; a key that could not be handled stays and is
// tried again on the next start. The original keys are never deleted.

import { api } from './api.js';
import { metaGet, metaSet, deviceId, myRecording, appendEdit, getRecording, adoptRecording } from './recstore.js';
import { logDiag } from './stores.js';

const rosterEntry = (p) => ({ id: p.id, number: p.number, name: p.name, position: p.position });
const baseAction = (a) => ({ seq: a.seq, skill: a.skill, grade: a.grade ?? null, player_id: a.player_id ?? null, sub_out: a.sub_out ?? null, sub_in: a.sub_in ?? null });

/** split an op list where the sequence numbers stop continuing */
export function episodes(ops) {
  const out = [];
  let cur = [];
  let expect = null; // the seq the next add should carry
  for (const op of ops) {
    if (op.type === 'add') {
      const s = op.action?.seq;
      if (cur.length && expect != null && s !== expect) { out.push(cur); cur = []; }
      cur.push(op);
      expect = (s ?? 0) + 1;
    } else if (op.type === 'undo') {
      cur.push(op);
      if (op.seq != null) expect = op.seq;
      else if (expect != null) expect -= 1;
    }
  }
  if (cur.length) out.push(cur);
  return out;
}

/** ops → edits, given the actions they sat on (seq is a label, the fold appends regardless) */
export function toEdits(ops, baseActions) {
  const sim = baseActions.map((a) => a.seq);
  const edits = [];
  for (const op of ops) {
    if (op.type === 'add') {
      edits.push({ op: 'add', action: baseAction(op.action) });
      sim.push(op.action.seq);
    } else if (op.type === 'undo') {
      const seq = op.seq ?? sim[sim.length - 1];
      if (seq == null) continue;
      edits.push({ op: 'undo', seq });
      if (sim[sim.length - 1] === seq) sim.pop();
    }
  }
  return edits;
}

function readOps(key) {
  try {
    const v = JSON.parse(localStorage.getItem(key) || 'null');
    return key.includes('dropped') ? v?.ops || [] : v || [];
  } catch { return []; }
}

export async function importLegacy(userId) {
  if (typeof localStorage === 'undefined' || userId == null) return 0;
  const keys = Object.keys(localStorage).filter((k) => /^so_ops_(dropped_)?\d+$/.test(k));
  if (!keys.length) return 0;
  const done = (await metaGet('legacy_done')) || {};
  // per match: the set-aside episodes first (older), then the live queue
  const byMatch = new Map();
  for (const key of keys) {
    const raw = localStorage.getItem(key) || '';
    if (done[key] === raw.length) continue;
    const mid = Number(key.replace(/\D/g, ''));
    const m = byMatch.get(mid) || { keys: [], ops: [] };
    m.keys.push([key, raw.length]);
    const ops = readOps(key);
    if (key.includes('dropped')) m.ops = episodes(ops).flat().concat(m.ops); else m.ops = m.ops.concat(ops);
    byMatch.set(mid, m);
  }
  let moved = 0;
  for (const [mid, { keys: ks, ops }] of byMatch) {
    const mark = () => { for (const [k, len] of ks) done[k] = len; };
    if (!ops.length) { mark(); continue; }
    let cached = null;
    try { cached = JSON.parse(localStorage.getItem(`so_match_${mid}`) || 'null'); } catch { cached = null; }
    try {
      // 1. the match's migrated recording on the server: adopt and continue it
      const legacyId = `legacy-${mid}`;
      let legacy = null;
      try { legacy = await api(`/matches/${mid}/recordings/${legacyId}`); } catch (e) { if (e.offline) throw e; legacy = null; }
      if (legacy) {
        const have = await getRecording(legacyId);
        const edits = toEdits(ops, legacy.snapshot.actions);
        if (have) { for (const e of edits) await appendEdit(legacyId, e); }
        else await adoptRecording({ id: legacyId, match_id: mid, base: legacy.base, edits: legacy.edits.map((e) => e.body), confirmed: legacy.n, userId, newEdits: edits, origin_id: legacy.origin_id, origin_n: legacy.origin_n });
        logDiag('legacy', `Spiel ${mid}: ${edits.length} alte Aktionen in die Aufzeichnung übernommen`);
        moved += edits.length; mark(); continue;
      }
      // 2. this device already records the match
      const mine = await myRecording(mid, userId);
      if (mine) {
        const edits = toEdits(ops, cached?.actions || []);
        for (const e of edits) await appendEdit(mine.id, e);
        logDiag('legacy', `Spiel ${mid}: ${edits.length} alte Aktionen an die eigene Aufzeichnung angehängt`);
        moved += edits.length; mark(); continue;
      }
      // 3. a new own recording from the cached match
      if (!cached) { logDiag('legacy', `Spiel ${mid}: ${ops.length} alte Aktionen ohne gespeicherten Spielstand, bleiben liegen`); continue; }
      const firstAdd = ops.find((o) => o.type === 'add');
      const firstSeq = firstAdd ? firstAdd.action.seq : (ops[0].seq ?? 0) + 1;
      const baseActions = (cached.actions || []).filter((a) => a.seq < firstSeq).map(baseAction);
      const base = {
        schema: 1,
        first_serve_us: cached.first_serve === 'us',
        lineups: Object.fromEntries(Object.entries(cached.lineups || {}).map(([s, l]) => [s, { pos: l.pos.slice(), libero: l.libero ?? null }])),
        roster: (cached.players || []).map(rosterEntry),
        actions: baseActions
      };
      const edits = toEdits(ops, baseActions);
      await adoptRecording({ id: `legacy-${(await deviceId()).slice(2, 12)}-${mid}`, match_id: mid, base, edits: [], confirmed: 0, userId, newEdits: edits, created: false });
      logDiag('legacy', `Spiel ${mid}: ${edits.length} alte Aktionen als eigene Aufzeichnung übernommen`);
      moved += edits.length; mark();
    } catch (e) {
      logDiag('legacy', `Spiel ${mid}: ${e?.message || e} (wird beim nächsten Start erneut versucht)`);
    }
  }
  await metaSet('legacy_done', done);
  return moved;
}
