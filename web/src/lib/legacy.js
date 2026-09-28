// One-time rescue of what the old client left in localStorage: unsent op
// queues (`so_ops_<match>`) and set-aside queues (`so_ops_dropped_<match>`),
// together with the match payload it had cached (`so_match_<match>`). Each
// contiguous episode becomes an imported recording (foreign device, not
// editable here) that the uploader sends like any other. The original keys
// stay untouched; a key is remembered by its length so a later, longer
// value is looked at again. Anything without a cached match stays raw and
// is only noted in the diagnostics.

import { importRecording, metaGet, metaSet, deviceId } from './recstore.js';
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

/** ops of one episode → edits, given the confirmed actions they sat on */
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

export async function importLegacy() {
  if (typeof localStorage === 'undefined') return 0;
  const keys = Object.keys(localStorage).filter((k) => /^so_ops_(dropped_)?\d+$/.test(k));
  if (!keys.length) return 0;
  const done = (await metaGet('legacy_done')) || {};
  const dev = await deviceId();
  let imported = 0;
  for (const key of keys) {
    const raw = localStorage.getItem(key) || '';
    if (done[key] === raw.length) continue;
    const dropped = key.includes('dropped');
    const matchId = Number(key.replace(/\D/g, ''));
    let ops = [];
    try { const v = JSON.parse(raw); ops = dropped ? v?.ops || [] : v || []; } catch { ops = []; }
    let cached = null;
    try { cached = JSON.parse(localStorage.getItem(`so_match_${matchId}`) || 'null'); } catch { cached = null; }
    if (!ops.length) { done[key] = raw.length; continue; }
    if (!cached) { logDiag('legacy', `${key}: ${ops.length} alte Aktionen ohne gespeicherten Spielstand, Rohdaten bleiben`); continue; }
    const eps = episodes(ops);
    for (const [k, ep] of eps.entries()) {
      const firstAdd = ep.find((o) => o.type === 'add');
      const firstSeq = firstAdd ? firstAdd.action.seq : (ep[0].seq ?? 0) + 1;
      const baseActions = (cached.actions || []).filter((a) => a.seq < firstSeq).map(baseAction);
      const base = {
        schema: 1,
        first_serve_us: cached.first_serve === 'us',
        lineups: Object.fromEntries(Object.entries(cached.lineups || {}).map(([s, l]) => [s, { pos: l.pos.slice(), libero: l.libero ?? null }])),
        roster: (cached.players || []).map(rosterEntry),
        actions: baseActions
      };
      const edits = toEdits(ep, baseActions);
      if (!edits.length) continue;
      const id = `legacy-${dev}-${matchId}-${dropped ? 'd' : 'q'}${k}`.replace(/[^A-Za-z0-9_-]/g, '').slice(0, 64);
      try {
        const r = await importRecording({ id, match_id: matchId, base, device_id: 'legacy', device_label: dropped ? 'Import (beiseitegelegt)' : 'Import (alt)' }, edits.map((b) => ({ body: b })));
        if (!r.existed) { imported++; logDiag('legacy', `${key}: ${edits.length} Aktionen als Aufzeichnung ${id} übernommen`); }
      } catch (e) {
        logDiag('legacy', `${key}: ${e?.message || e}`);
      }
    }
    done[key] = raw.length;
  }
  await metaSet('legacy_done', done);
  return imported;
}
