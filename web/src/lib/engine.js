// The match engine, client twin of server/src/engine.rs.
// Everything on screen derives from the action log: score, rotation, serve,
// set boundaries and every statistic are replayed from it, so the live
// screen renders optimistically and offline from the same code the server
// runs. `scripts/fixture.mjs` pins both implementations together.

export const SKILLS = [
  { key: 'S', name: 'Aufschlag', short: 'Auf' },
  { key: 'R', name: 'Annahme', short: 'Ann' },
  { key: 'E', name: 'Zuspiel', short: 'Zus' },
  { key: 'A', name: 'Angriff', short: 'Ang' },
  { key: 'B', name: 'Block', short: 'Blo' },
  { key: 'D', name: 'Abwehr', short: 'Abw' }
];
export const SKILL = Object.fromEntries(SKILLS.map((s) => [s.key, s]));
export const GRADES = ['#', '+', '!', '-', '/', '='];
export const GRADE_CLASS = { '#': 'g-win', '+': 'g-pos', '!': 'g-mid', '-': 'g-neg', '/': 'g-half', '=': 'g-err' };
export const GRADE_NAME = { '#': 'Punkt/Perfekt', '+': 'Positiv', '!': 'Neutral', '-': 'Negativ', '/': 'Halb', '=': 'Fehler' };
export const PAD = {
  S: { '#': 'Ass', '+': 'Gut', '!': 'OK', '-': 'Schwach', '/': null, '=': 'Fehler' },
  R: { '#': 'Perfekt', '+': 'Gut', '!': 'OK', '-': 'Schwach', '/': 'Overpass', '=': 'Fehler' },
  E: { '#': 'Perfekt', '+': 'Gut', '!': 'OK', '-': 'Schwach', '/': null, '=': 'Fehler' },
  A: { '#': 'Punkt', '+': 'Gut', '!': 'Rally', '-': 'Schwach', '/': 'Geblockt', '=': 'Fehler' },
  B: { '#': 'Punkt', '+': 'Touch+', '!': 'Touch', '-': 'Touch−', '/': null, '=': 'Fehler' },
  D: { '#': 'Perfekt', '+': 'Gut', '!': 'OK', '-': 'Schwach', '/': 'Overpass', '=': 'Fehler' }
};
export const POS_NAME = { Z: 'Zuspiel', A: 'Außen', M: 'Mitte', D: 'Diagonal', L: 'Libero' };
export const ROMAN = ['I', 'II', 'III', 'IV', 'V', 'VI'];

/** rally outcome of one action: 'us' | 'them' | null (continues) */
export function outcome(skill, grade) {
  if (skill === 'opp') return grade === '=' ? 'us' : 'them'; // opponent error → our point
  if (skill === 'adj') return grade === '#' ? 'us' : 'them'; // catch-up: # us, = them
  if (skill === 'sub' || skill === 'lib' || skill === 'rot' || skill === 'srv') return null;
  if (grade === '=') return 'them';
  if (grade === '/' && skill === 'A') return 'them';
  if (grade === '#' && (skill === 'A' || skill === 'S' || skill === 'B')) return 'us';
  return null;
}

export const setTarget = (s) => (s === 5 ? 15 : 25);
const rotate = (l) => [l[1], l[2], l[3], l[4], l[5], l[0]];

/** lineups: { "1": {pos:[6 ids], libero}, ... } — a set inherits the closest lower one */
export function lineupFor(cfg, set) {
  const keys = Object.keys(cfg.lineups || {}).map(Number).sort((a, b) => a - b);
  let best = null;
  for (const k of keys) if (k <= set) best = k;
  if (best == null && keys.length) best = keys[0];
  const l = best == null ? null : cfg.lineups[best];
  return l ? { pos: l.pos.slice(), libero: l.libero ?? null } : { pos: [0, 0, 0, 0, 0, 0], libero: null };
}

/**
 * replay(cfg, actions) → state
 * cfg: { first_serve_us: bool, lineups }
 * actions: [{ id, seq, skill, grade, player_id, sub_out, sub_in }] in seq order
 */
export function replay(cfg, actions) {
  const first = lineupFor(cfg, 1);
  const st = {
    set: 1, us: 0, them: 0, sets: [], sets_won: 0, sets_lost: 0,
    lineup: first.pos, libero: first.libero, libero_for: null, libero_off: false,
    serving: !!cfg.first_serve_us, rally: 1, rotn: 0, finished: false, last_seq: 0,
    rows: [], rally_log: []
  };
  // rotn: rotations since the set started (0-5). Rotation statistics group by
  // it, not by the player on I, so substitutes do not split a rotation.
  let setServeStart = !!cfg.first_serve_us;
  for (const a of actions) {
    if (st.finished) break;
    st.last_seq = a.seq;
    if (a.skill === 'rot' || a.skill === 'srv') {
      if (a.skill === 'rot') { st.lineup = rotate(st.lineup); st.rotn = (st.rotn + 1) % 6; } else st.serving = a.grade === '#';
      st.rows.push({ ...a, set: st.set, rally: st.rally, us: st.us, them: st.them, serving: st.serving, rot: st.lineup[0], outcome: null });
      continue;
    }
    if (a.skill === 'sub' || a.skill === 'lib') {
      if (a.skill === 'lib') {
        if (a.sub_in == null) { st.libero_off = true; st.libero_for = null; }
        else { st.libero_off = false; st.libero_for = a.sub_out ?? null; }
      } else {
        const k = st.lineup.indexOf(a.sub_out);
        if (k >= 0 && a.sub_in) st.lineup[k] = a.sub_in;
        if (st.libero_for === a.sub_out) st.libero_for = null;
      }
      st.rows.push({ ...a, set: st.set, rally: st.rally, us: st.us, them: st.them, serving: st.serving, rot: st.lineup[0], outcome: null });
      continue;
    }
    const out = outcome(a.skill, a.grade);
    st.rows.push({ ...a, set: st.set, rally: st.rally, us: st.us, them: st.them, serving: st.serving, rot: st.lineup[0], outcome: out });
    if (!out) continue;
    const won = out === 'us';
    if (a.skill === 'adj') {
      // catch-up point: score only, no rally, rotation or serve change
      if (won) st.us++; else st.them++;
    } else {
      st.rally_log.push({ set: st.set, rally: st.rally, serving: st.serving, won, rot: st.lineup[0], rotn: st.rotn, us: st.us, them: st.them });
      const wonOnReceive = won && !st.serving;
      if (won) st.us++; else st.them++;
      if (wonOnReceive) { st.lineup = rotate(st.lineup); st.rotn = (st.rotn + 1) % 6; }
      st.serving = won;
      st.rally++;
    }
    const tgt = setTarget(st.set);
    if ((st.us >= tgt || st.them >= tgt) && Math.abs(st.us - st.them) >= 2) {
      st.sets.push({ us: st.us, them: st.them });
      const w = st.sets.filter((s) => s.us > s.them).length;
      const l = st.sets.length - w;
      if (w === 3 || l === 3) { st.finished = true; break; }
      st.set++;
      st.rally = 1;
      st.rotn = 0;
      setServeStart = !setServeStart;
      const nl = lineupFor(cfg, st.set);
      st.lineup = nl.pos;
      st.libero = nl.libero;
      st.serving = setServeStart;
      st.us = 0;
      st.them = 0;
    }
  }
  st.sets_won = st.sets.filter((s) => s.us > s.them).length;
  st.sets_lost = st.sets.length - st.sets_won;
  return st;
}

/** court slots I..VI; the libero stands in for the back-row middle, or for
 *  `liberoFor` when the coach set one explicitly. Back row is V, VI and,
 *  while the opponent serves, I: the libero never serves, so on I she
 *  steps out the moment we win the serve. */
export function courtView(lineup, libero, byId, liberoFor = null, liberoOff = false, serving = false) {
  const isBack = (pos) => pos === 5 || pos === 6 || (pos === 1 && !serving);
  // exactly one card gets the libero: the explicit player, or in automatic
  // mode one back-row middle (VI before V before I, in case a lineup has
  // both middles in the back row at once)
  let target = null;
  if (libero && !liberoOff) {
    if (liberoFor) { const k = lineup.indexOf(liberoFor); if (k >= 0 && isBack(k + 1)) target = liberoFor; }
    else for (const pos of [6, 5, 1]) { const id = lineup[pos - 1]; if (isBack(pos) && byId[id]?.position === 'M') { target = id; break; } }
  }
  return lineup.map((id, idx) => {
    const pos = idx + 1;
    const lib = target != null && id === target;
    return { pos, id: lib ? libero : id, replaced: lib ? id : null, libero: lib };
  });
}

/** the skill(s) the rally phase makes likely next */
export function expectedSkills(st) {
  const cur = st.rows.filter((r) => r.set === st.set && r.rally === st.rally && !['sub', 'lib', 'rot', 'srv', 'adj'].includes(r.skill));
  if (!cur.length) return st.serving ? ['S'] : ['R'];
  const last = cur[cur.length - 1];
  // an overpass ("/") on reception or dig hands the ball to the opponent
  if (last.grade === '/' && (last.skill === 'R' || last.skill === 'D')) return ['B', 'D'];
  return { S: ['B', 'D'], R: ['E'], E: ['A'], A: ['B', 'D'], B: ['D'], D: ['E'] }[last.skill] || [];
}

// ------------------------------------------------------------------ stats

const RECEPTION_SCORE = { '#': 3, '+': 2, '!': 1, '-': 0, '/': 0, '=': 0 };
function emptyPlayer(p) {
  return {
    id: p.id, number: p.number, name: p.name, pos: p.position, pts: 0, n: 0,
    A: { n: 0, k: 0, e: 0, blk: 0 }, S: { n: 0, ace: 0, err: 0, pos: 0 },
    R: { n: 0, perf: 0, pos: 0, err: 0, sum: 0 }, B: { pts: 0, touch: 0, err: 0 },
    D: { n: 0, good: 0, err: 0 }, E: { n: 0, ast: 0, err: 0 },
    grades: { S: {}, R: {}, E: {}, A: {}, B: {}, D: {} }
  };
}

/** rotation bucket key for a rally: the setter's court position when the set's
 *  starting lineup has exactly one setter ("Z1".."Z6"), else the starter who
 *  is on I in that rotation ("P<id>"). Comparable across sets with different
 *  lineups, immune to substitutions. */
export function rotKey(cfg, byId, set, rotn) {
  const l = lineupFor(cfg, set);
  const setters = l.pos.map((id, i) => (byId[id]?.position === 'Z' ? i : -1)).filter((i) => i >= 0);
  if (setters.length === 1) return 'Z' + ((((setters[0] - rotn) % 6) + 6) % 6 + 1);
  return 'P' + (l.pos[rotn] ?? 0);
}

/** stats(cfg, players, actions, set) — set 0/null = whole match. Same shape as the server. */
export function stats(cfg, players, actions, set) {
  const rp = replay(cfg, actions);
  const rows = rp.rows.filter((r) => !set || r.set === set);
  const rallies = rp.rally_log.filter((r) => !set || r.set === set);
  const byId = Object.fromEntries(players.map((p) => [p.id, p]));
  const P = {};
  const team = {
    ptsBy: { A: 0, S: 0, B: 0, opp: 0 }, errBy: { S: 0, R: 0, E: 0, A: 0, B: 0, D: 0 },
    lostBy: { oppKill: 0, err: 0 }, sideout: { won: 0, n: 0 }, brk: { won: 0, n: 0 },
    byRot: {}, grades: { S: {}, R: {}, E: {}, A: {}, B: {}, D: {} }, us: 0, them: 0
  };
  const get = (id) => (P[id] = P[id] || emptyPlayer(byId[id] || { id, number: 0, name: '?', position: '?' }));
  let lastSet = null;
  for (const r of rows) {
    if (['sub', 'lib', 'opp', 'adj', 'rot', 'srv'].includes(r.skill)) {
      if (r.skill === 'opp') { if (r.grade === '=') team.ptsBy.opp++; else team.lostBy.oppKill++; }
      lastSet = null;
      continue;
    }
    if (r.player_id == null) continue;
    const p = get(r.player_id);
    p.n++;
    p.grades[r.skill][r.grade] = (p.grades[r.skill][r.grade] || 0) + 1;
    team.grades[r.skill][r.grade] = (team.grades[r.skill][r.grade] || 0) + 1;
    if (r.grade === '=') { team.errBy[r.skill]++; team.lostBy.err++; }
    switch (r.skill) {
      case 'A':
        p.A.n++;
        if (r.grade === '#') { p.A.k++; p.pts++; team.ptsBy.A++; if (lastSet) lastSet.E.ast++; }
        else if (r.grade === '=') p.A.e++;
        else if (r.grade === '/') { p.A.blk++; team.lostBy.err++; }
        break;
      case 'S':
        p.S.n++;
        if (r.grade === '#') { p.S.ace++; p.pts++; team.ptsBy.S++; } else if (r.grade === '=') p.S.err++; else if (r.grade === '+') p.S.pos++;
        break;
      case 'R':
        p.R.n++; p.R.sum += RECEPTION_SCORE[r.grade] || 0;
        if (r.grade === '#') { p.R.perf++; p.R.pos++; } else if (r.grade === '+') p.R.pos++; else if (r.grade === '=') p.R.err++;
        break;
      case 'B':
        if (r.grade === '#') { p.B.pts++; p.pts++; team.ptsBy.B++; } else if (r.grade === '=') p.B.err++; else p.B.touch++;
        break;
      case 'D':
        p.D.n++;
        if (r.grade === '=') p.D.err++; else if (r.grade !== '-' && r.grade !== '/') p.D.good++;
        break;
      case 'E':
        p.E.n++;
        if (r.grade === '=') p.E.err++;
        break;
    }
    // the remembered setter dies with the rally: an assist needs the set and
    // the kill in the same rally
    lastSet = r.outcome ? null : r.skill === 'E' ? p : r.skill === 'A' ? null : lastSet;
  }
  for (const r of rallies) {
    const k = r.serving ? 'brk' : 'sideout';
    team[k].n++; if (r.won) team[k].won++;
    if (r.won) team.us++; else team.them++;
    const key = rotKey(cfg, byId, r.set, r.rotn);
    const rot = (team.byRot[key] = team.byRot[key] || { rot: key, so: { won: 0, n: 0 }, brk: { won: 0, n: 0 } });
    const rk = r.serving ? 'brk' : 'so';
    rot[rk].n++; if (r.won) rot[rk].won++;
  }
  const list = Object.values(P).map((p) => {
    p.A.pct = p.A.n ? (p.A.k - p.A.e - p.A.blk) / p.A.n : null;
    p.A.kpct = p.A.n ? p.A.k / p.A.n : null;
    p.R.avg = p.R.n ? p.R.sum / p.R.n : null;
    p.R.pospct = p.R.n ? p.R.pos / p.R.n : null;
    p.R.perfpct = p.R.n ? p.R.perf / p.R.n : null;
    return p;
  });
  const l = lineupFor(cfg, set || rp.set);
  const order = l.pos.concat(l.libero ? [l.libero] : []);
  list.sort((a, b) => {
    const ia = order.indexOf(a.id), ib = order.indexOf(b.id);
    const ra = ia < 0 ? 99 : ia, rb = ib < 0 ? 99 : ib;
    return ra - rb || a.number - b.number;
  });
  return { set: set || null, players: list, team, state: rp, rallies };
}

// -------------------------------------------------------------- formatting

export const pct = (x, d = 0) => (x == null ? '–' : (x * 100).toFixed(d) + ' %');
export const fix = (x, d = 2) => (x == null ? '–' : x.toFixed(d));
export const eff = (x) => (x == null ? '–' : (x < 0 ? '−' : '') + '.' + String(Math.round(Math.abs(x) * 1000)).padStart(3, '0'));
export const firstName = (p) => (p?.name || '?').split(' ')[0];
export function fmtDate(iso) {
  if (!iso) return '';
  const [y, m, d] = iso.split('-');
  return `${Number(d)}.${Number(m)}.${y}`;
}
export function fmtDM(iso) {
  if (!iso) return '';
  const [, m, d] = iso.split('-');
  return `${Number(d)}.${Number(m)}.`;
}
export function todayIso() {
  const n = new Date();
  return `${n.getFullYear()}-${String(n.getMonth() + 1).padStart(2, '0')}-${String(n.getDate()).padStart(2, '0')}`;
}
