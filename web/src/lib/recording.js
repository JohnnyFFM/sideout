// Recordings on the client, twin of server/src/recording.rs.
// A recording is an immutable initial state (base) plus numbered edits;
// `fold` turns them into what the engine replays (cfg, roster, actions).

export const SCHEMA = 1;

/** confirmed state of a match (a server payload) → the base of a copy, or of a fresh recording from the planning */
export function baseFromMatch(m, { planning = false } = {}) {
  const active = (m.players || []).filter((p) => p.active);
  if (planning) {
    const lineups = m.planning?.lineups || {};
    const referenced = new Set(Object.values(lineups).flatMap((l) => l.pos.concat(l.libero ? [l.libero] : [])));
    const roster = (m.players || []).filter((p) => p.active || referenced.has(p.id));
    return {
      schema: SCHEMA,
      first_serve_us: (m.planning?.first_serve || 'us') === 'us',
      lineups: cloneLineups(lineups),
      roster: roster.map(rosterEntry),
      actions: []
    };
  }
  // a copy of the shown state: the recording's own roster, plus everyone active today
  const known = new Map((m.roster || []).map((p) => [p.id, rosterEntry(p)]));
  for (const p of active) if (!known.has(p.id)) known.set(p.id, rosterEntry(p));
  return {
    schema: SCHEMA,
    first_serve_us: m.first_serve === 'us',
    lineups: cloneLineups(m.lineups || {}),
    roster: [...known.values()],
    actions: (m.actions || []).map(baseAction)
  };
}

const rosterEntry = (p) => ({ id: p.id, number: p.number, name: p.name, position: p.position });
const baseAction = (a) => ({ seq: a.seq, skill: a.skill, grade: a.grade ?? null, player_id: a.player_id ?? null, sub_out: a.sub_out ?? null, sub_in: a.sub_in ?? null });
function cloneLineups(l) {
  const out = {};
  for (const [k, v] of Object.entries(l)) out[k] = { pos: v.pos.slice(), libero: v.libero ?? null };
  return out;
}

/** a new action for an `add` edit; seq continues the effective log */
export function newAction(actions, a) {
  const seq = (actions[actions.length - 1]?.seq || 0) + 1;
  return { seq, skill: a.skill, grade: a.grade ?? null, player_id: a.player_id ?? null, sub_out: a.sub_out ?? null, sub_in: a.sub_in ?? null };
}

/**
 * fold(base, edits) → { cfg, roster, actions }
 * Lenient like the server: an undo whose target is not on top is a no-op,
 * an add always appends. Action ids are the edit numbers (base actions
 * carry negative ids), so the engine's rows stay unique.
 */
export function fold(base, edits) {
  const cfg = { first_serve_us: !!base.first_serve_us, lineups: cloneLineups(base.lineups || {}) };
  const roster = (base.roster || []).map((p) => ({ ...p }));
  const actions = (base.actions || []).map((a) => ({ ...a, id: -a.seq }));
  edits.forEach((e, i) => {
    const n = i + 1;
    switch (e.op) {
      case 'add':
        actions.push({ ...e.action, id: n });
        break;
      case 'undo': {
        const top = actions[actions.length - 1];
        if (top && top.seq === e.seq) actions.pop();
        break;
      }
      case 'lineup':
        cfg.lineups[e.set] = { pos: e.lineup.pos.slice(), libero: e.lineup.libero ?? null };
        break;
      case 'first_serve':
        cfg.first_serve_us = !!e.us;
        break;
      case 'roster': {
        const k = roster.findIndex((r) => r.id === e.player.id);
        if (k >= 0) roster[k] = { ...e.player }; else roster.push({ ...e.player });
        break;
      }
    }
  });
  return { cfg, roster, actions };
}

/** the roster as a lookup, the team's current names first, the snapshot's for anyone the team no longer has */
export function playersOf(teamPlayers, snapshot) {
  const out = (teamPlayers || []).map((p) => ({ ...p }));
  for (const p of snapshot?.roster || []) if (!out.some((t) => t.id === p.id)) out.push({ ...p, active: false, snapshot: true });
  for (const a of snapshot?.actions || []) {
    for (const id of [a.player_id, a.sub_out, a.sub_in]) {
      if (id != null && !out.some((t) => t.id === id)) out.push({ id, number: 0, name: `Unbekannt (ID ${id})`, position: '?', active: false, snapshot: true });
    }
  }
  return out;
}

/** a readable name for a recording in lists and panels: who, on what, and
 *  for imports where they came from (server rows carry `device`, local rows `device_label`) */
export function recordingLabel(r, myDevice) {
  if (r.device_id && r.device_id === myDevice && !r.imported) return 'Dieses Gerät';
  const who = r.user || 'Unbekannt';
  const dev = r.device ?? r.device_label ?? '';
  if (r.device_id === 'legacy' && dev === 'Import') return `${who} · alter Spielstand vom Server (vor der Umstellung)`;
  if (r.device_id === 'legacy') return `${who} · aus der alten App-Version eines Geräts${/beiseite/i.test(dev) ? ' (damals beiseitegelegt)' : ''}`;
  if (r.imported) return `${who} · aus Datei importiert${dev ? ` (${dev})` : ''}`;
  return `${who}${dev ? ' · ' + dev : ''}`;
}
