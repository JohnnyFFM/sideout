// SIDEOUT mock data + engine.
// Everything on screen derives from ONE append-only action log:
//   { set, skill, grade, player }      skill ∈ S R E A B D  (Data-Volley codes)
//   { set, skill:'opp', grade:'#'|'=' } opponent point / opponent error
//   { set, skill:'sub', out, in }       substitution
// Score, rotation, serve, side-out and every stat are replayed from the log,
// so undo = pop. The real app keeps the same model in SQLite (see CONCEPT.md).
window.SO = (function () {
  'use strict';

  // ---------- vocabulary ----------
  const SKILLS = [
    { key: 'S', name: 'Aufschlag', short: 'Auf' },
    { key: 'R', name: 'Annahme',   short: 'Ann' },
    { key: 'E', name: 'Zuspiel',   short: 'Zus' },
    { key: 'A', name: 'Angriff',   short: 'Ang' },
    { key: 'B', name: 'Block',     short: 'Blo' },
    { key: 'D', name: 'Abwehr',    short: 'Abw' },
  ];
  const SKILL = Object.fromEntries(SKILLS.map(s => [s.key, s]));
  const GRADES = ['#', '+', '!', '-', '/', '='];
  const GRADE_CLASS = { '#': 'g-win', '+': 'g-pos', '!': 'g-mid', '-': 'g-neg', '/': 'g-half', '=': 'g-err' };
  const GRADE_NAME  = { '#': 'Punkt/Perfekt', '+': 'Positiv', '!': 'Neutral', '-': 'Negativ', '/': 'Halb', '=': 'Fehler' };
  // pad labels per skill (null = code not used for that skill)
  const PAD = {
    S: { '#': 'Ass',     '+': 'Gut',    '!': 'OK',    '-': 'Schwach', '/': null,       '=': 'Fehler' },
    R: { '#': 'Perfekt', '+': 'Gut',    '!': 'OK',    '-': 'Schwach', '/': 'Overpass', '=': 'Fehler' },
    E: { '#': 'Perfekt', '+': 'Gut',    '!': 'OK',    '-': 'Schwach', '/': null,       '=': 'Fehler' },
    A: { '#': 'Punkt',   '+': 'Gut',    '!': 'Rally', '-': 'Schwach', '/': 'Geblockt', '=': 'Fehler' },
    B: { '#': 'Punkt',   '+': 'Touch+', '!': 'Touch', '-': 'Touch−',  '/': null,       '=': 'Fehler' },
    D: { '#': 'Perfekt', '+': 'Gut',    '!': 'OK',    '-': 'Schwach', '/': null,       '=': 'Fehler' },
  };
  const POS_NAME = { Z: 'Zuspiel', A: 'Außen', M: 'Mitte', D: 'Diagonal', L: 'Libero' };

  // rally outcome of a single action: 'us' | 'them' | null (rally continues)
  function outcome(skill, grade) {
    if (skill === 'opp') return grade === '=' ? 'us' : 'them';
    if (skill === 'sub') return null;
    if (grade === '=') return 'them';
    if (grade === '/' && skill === 'A') return 'them';          // blocked
    if (grade === '#' && (skill === 'A' || skill === 'S' || skill === 'B')) return 'us';
    return null;
  }

  // ---------- team ----------
  const TEAM = { name: 'TSV Eintracht', short: 'TSV', league: 'Landesliga Süd', season: '2026/27' };
  const ROSTER = [
    { no: 1,  name: 'Lena Vogt',      pos: 'Z' },
    { no: 3,  name: 'Mia Brandt',     pos: 'A' },
    { no: 4,  name: 'Sara Kunz',      pos: 'M' },
    { no: 5,  name: 'Jule Hartmann',  pos: 'D' },
    { no: 7,  name: 'Nele Fischer',   pos: 'A' },
    { no: 8,  name: 'Emma Roth',      pos: 'M' },
    { no: 9,  name: 'Kim Sauer',      pos: 'A' },
    { no: 10, name: 'Anna Weiß',      pos: 'Z' },
    { no: 11, name: 'Tina Berger',    pos: 'M' },
    { no: 12, name: 'Jo Lindner',     pos: 'L' },
    { no: 14, name: 'Paula Schmid',   pos: 'D' },
    { no: 15, name: 'Hanna Keller',   pos: 'A' },
  ];
  const BY_NO = Object.fromEntries(ROSTER.map(p => [p.no, p]));
  const player = no => BY_NO[no] || { no, name: '?', pos: '?' };
  const firstName = no => player(no).name.split(' ')[0];

  // lineup = jersey numbers in positions I..VI (index 0 = position I = server)
  const MATCH = {
    id: 3,
    opponent: 'VfL Bad Vilbel',
    date: '2026-09-19',
    home: true,
    hall: 'Sporthalle Nord',
    firstServe: 'us',
    lineup: [5, 3, 4, 1, 7, 8],   // I Jule(D) · II Mia(A) · III Sara(M) · IV Lena(Z) · V Nele(A) · VI Emma(M)
    libero: 12,
  };
  // earlier matches of the season: fully simulated from a seed, same engine, same lineup
  const PAST = [
    { id: 1, opponent: 'TG Rüsselsheim', date: '2026-09-05', home: false, hall: 'Walter-Köbel-Halle', firstServe: 'them', lineup: MATCH.lineup, libero: 12, seed: 1101 },
    { id: 2, opponent: 'SG Langen',      date: '2026-09-12', home: true,  hall: 'Sporthalle Nord',    firstServe: 'us',   lineup: MATCH.lineup, libero: 12, seed: 2202 },
  ];
  const UPCOMING = [
    { opponent: 'TV Hofheim', date: '2026-09-26', home: false, hall: 'Sporthalle Hofheim' },
    { opponent: 'VC Wiesbaden III', date: '2026-10-03', home: true, hall: 'Sporthalle Nord' },
  ];

  // ---------- replay ----------
  function rotate(lu) { return [lu[1], lu[2], lu[3], lu[4], lu[5], lu[0]]; }
  const setTarget = s => (s === 5 ? 15 : 25);

  // replay the log → state + annotated rows
  function replay(actions, match) {
    match = match || MATCH;
    const st = {
      set: 1, us: 0, them: 0, sets: [], lineup: match.lineup.slice(),
      serving: match.firstServe === 'us', rally: 1, rows: [], finished: false,
      rallyLog: [],           // one entry per finished rally
    };
    let setServeStart = match.firstServe === 'us';
    const startSet = () => {
      st.lineup = match.lineup.slice();
      st.serving = setServeStart;
      st.us = 0; st.them = 0;
    };
    for (let i = 0; i < actions.length; i++) {
      const a = actions[i];
      if (st.finished) break;
      if (a.skill === 'sub') {
        const k = st.lineup.indexOf(a.out);
        if (k >= 0) st.lineup[k] = a.in;
        st.rows.push(Object.assign({}, a, { i, set: st.set, rally: st.rally, us: st.us, them: st.them, outcome: null }));
        continue;
      }
      const out = outcome(a.skill, a.grade);
      const row = Object.assign({}, a, {
        i, set: st.set, rally: st.rally, us: st.us, them: st.them,
        serving: st.serving, rot: st.lineup[0], outcome: out,
      });
      st.rows.push(row);
      if (!out) continue;
      // rally ends
      const wonOnReceive = out === 'us' && !st.serving;
      st.rallyLog.push({ set: st.set, rally: st.rally, serving: st.serving, won: out === 'us', rot: st.lineup[0], us: st.us, them: st.them, cause: a.skill, grade: a.grade, player: a.player });
      if (out === 'us') st.us++; else st.them++;
      if (wonOnReceive) st.lineup = rotate(st.lineup);
      st.serving = out === 'us';
      st.rally++;
      const tgt = setTarget(st.set);
      if ((st.us >= tgt || st.them >= tgt) && Math.abs(st.us - st.them) >= 2) {
        st.sets.push({ us: st.us, them: st.them });
        const w = st.sets.filter(s => s.us > s.them).length, l = st.sets.length - w;
        if (w === 3 || l === 3) { st.finished = true; break; }
        st.set++; st.rally = 1;
        setServeStart = !setServeStart;
        startSet();
      }
    }
    st.setsWon = st.sets.filter(s => s.us > s.them).length;
    st.setsLost = st.sets.length - st.setsWon;
    return st;
  }

  // who is on court right now, with libero shown for the back-row middle (V/VI, not I: she serves)
  function courtView(lineup, libero) {
    return lineup.map((no, idx) => {
      const pos = idx + 1;
      const p = player(no);
      const lib = libero && p.pos === 'M' && (pos === 5 || pos === 6);
      return { pos, no: lib ? libero : no, replaced: lib ? no : null, libero: !!lib };
    });
  }

  // ---------- stats ----------
  const RECEPTION_SCORE = { '#': 3, '+': 2, '!': 1, '-': 0, '/': 0, '=': 0 };
  function emptyPlayer(no) {
    return {
      no, name: player(no).name, pos: player(no).pos, pts: 0, n: 0,
      A: { n: 0, k: 0, e: 0, blk: 0 }, S: { n: 0, ace: 0, err: 0, pos: 0 },
      R: { n: 0, perf: 0, pos: 0, err: 0, sum: 0 }, B: { pts: 0, touch: 0, err: 0 },
      D: { n: 0, good: 0, err: 0 }, E: { n: 0, ast: 0, err: 0 },
      grades: { S: {}, R: {}, E: {}, A: {}, B: {}, D: {} },
    };
  }
  function stats(actions, opts) {
    opts = opts || {};
    const rp = replay(actions, opts.match);
    const rows = rp.rows.filter(r => !opts.set || r.set === opts.set);
    const rallies = rp.rallyLog.filter(r => !opts.set || r.set === opts.set);
    const P = {};
    const team = {
      ptsBy: { A: 0, S: 0, B: 0, opp: 0 }, errBy: { S: 0, R: 0, E: 0, A: 0, B: 0, D: 0 },
      lostBy: { oppKill: 0, err: 0 },
      sideout: { won: 0, n: 0 }, brk: { won: 0, n: 0 },
      byRot: {}, grades: { S: {}, R: {}, E: {}, A: {}, B: {}, D: {} },
      us: 0, them: 0,
    };
    const get = no => (P[no] = P[no] || emptyPlayer(no));
    let lastSet = null; // for assists
    rows.forEach(r => {
      if (r.skill === 'sub' || r.skill === 'opp') {
        if (r.skill === 'opp') { if (r.grade === '=') team.ptsBy.opp++; else team.lostBy.oppKill++; }
        lastSet = null; return;
      }
      const p = get(r.player); p.n++;
      p.grades[r.skill][r.grade] = (p.grades[r.skill][r.grade] || 0) + 1;
      team.grades[r.skill][r.grade] = (team.grades[r.skill][r.grade] || 0) + 1;
      if (r.grade === '=') { team.errBy[r.skill]++; team.lostBy.err++; }
      switch (r.skill) {
        case 'A': p.A.n++; if (r.grade === '#') { p.A.k++; p.pts++; team.ptsBy.A++; if (lastSet) lastSet.E.ast++; }
                  else if (r.grade === '=') p.A.e++; else if (r.grade === '/') { p.A.blk++; team.lostBy.err++; team.errBy.A += 0; } break;
        case 'S': p.S.n++; if (r.grade === '#') { p.S.ace++; p.pts++; team.ptsBy.S++; } else if (r.grade === '=') p.S.err++; else if (r.grade === '+') p.S.pos++; break;
        case 'R': p.R.n++; p.R.sum += RECEPTION_SCORE[r.grade]; if (r.grade === '#') { p.R.perf++; p.R.pos++; } else if (r.grade === '+') p.R.pos++; else if (r.grade === '=') p.R.err++; break;
        case 'B': if (r.grade === '#') { p.B.pts++; p.pts++; team.ptsBy.B++; } else if (r.grade === '=') p.B.err++; else p.B.touch++; break;
        case 'D': p.D.n++; if (r.grade === '=') p.D.err++; else if (r.grade !== '-') p.D.good++; break;
        case 'E': p.E.n++; if (r.grade === '=') p.E.err++; break;
      }
      lastSet = r.skill === 'E' ? p : (r.skill === 'A' ? null : lastSet);
    });
    rallies.forEach(r => {
      const k = r.serving ? 'brk' : 'sideout';
      team[k].n++; if (r.won) team[k].won++;
      if (r.won) team.us++; else team.them++;
      const rot = team.byRot[r.rot] = team.byRot[r.rot] || { rot: r.rot, so: { won: 0, n: 0 }, brk: { won: 0, n: 0 } };
      const rk = k === 'brk' ? 'brk' : 'so'; rot[rk].n++; if (r.won) rot[rk].won++;
    });
    const players = Object.values(P).map(p => {
      p.A.pct = p.A.n ? (p.A.k - p.A.e - p.A.blk) / p.A.n : null;   // Hitting efficiency (K-E)/TA
      p.A.kpct = p.A.n ? p.A.k / p.A.n : null;
      p.R.avg = p.R.n ? p.R.sum / p.R.n : null;                      // 0–3 scale
      p.R.pospct = p.R.n ? p.R.pos / p.R.n : null;
      p.R.perfpct = p.R.n ? p.R.perf / p.R.n : null;
      return p;
    });
    // roster order: starters first by lineup position, then the rest by jersey
    const order = (opts.match || MATCH).lineup.concat([(opts.match || MATCH).libero]);
    players.sort((a, b) => {
      const ia = order.indexOf(a.no), ib = order.indexOf(b.no);
      if (ia >= 0 && ib >= 0) return ia - ib; if (ia >= 0) return -1; if (ib >= 0) return 1; return a.no - b.no;
    });
    return { players, team, replay: rp, rows, rallies };
  }

  // ---------- seeded simulation (deterministic) ----------
  function mulberry32(a) { return function () { a |= 0; a = a + 0x6D2B79F5 | 0; let t = Math.imul(a ^ a >>> 15, 1 | a); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
  function pick(rnd, table) { // table: [[value, weight], ...]
    const tot = table.reduce((s, t) => s + t[1], 0); let x = rnd() * tot;
    for (const [v, w] of table) { x -= w; if (x <= 0) return v; } return table[table.length - 1][0];
  }
  function simulate(seed, stopAt, match) {
    const rnd = mulberry32(seed);
    const log = [];
    const m = match || MATCH;
    let st = replay(log, m);
    const push = a => { log.push(a); st = replay(log, m); };
    const cur = () => courtView(st.lineup, m.libero);
    const front = () => cur().filter(c => c.pos >= 2 && c.pos <= 4);
    const back  = () => cur().filter(c => c.pos === 5 || c.pos === 6 || c.pos === 1);
    const setter = () => { const z = cur().find(c => player(c.no).pos === 'Z'); return z ? z.no : cur()[0].no; };

    function ourAttack(q) { // q: quality of the previous contact 0..3
      const s = setter();
      const sg = pick(rnd, [['#', 10 + q * 8], ['+', 30 + q * 5], ['!', 30], ['-', 15 - q * 3], ['=', 2]]);
      push({ set: st.set, skill: 'E', grade: sg, player: s });
      if (outcome('E', sg)) return;
      const hitters = cur().filter(c => c.no !== s && !c.libero);
      const w = hitters.map(c => { const pp = player(c.no).pos; const fr = c.pos >= 2 && c.pos <= 4;
        return [c.no, pp === 'A' ? (fr ? 30 : 12) : pp === 'D' ? (fr ? 26 : 10) : pp === 'M' ? (fr ? (q >= 2 ? 22 : 8) : 0) : 3]; })
        .filter(t => t[1] > 0);
      const h = pick(rnd, w);
      const ag = pick(rnd, [['#', 28 + q * 6], ['+', 22], ['!', 20], ['-', 10], ['/', 8], ['=', 10 - q]]);
      push({ set: st.set, skill: 'A', grade: ag, player: h });
      if (outcome('A', ag)) return;
      oppAttack();
    }
    function oppAttack() {
      const r = rnd();
      if (r < 0.22) { push({ set: st.set, skill: 'opp', grade: '=' }); return; }   // opponent error
      if (r < 0.50) { push({ set: st.set, skill: 'opp', grade: '#' }); return; }   // opponent kill
      // we touch it: maybe block, then dig
      if (rnd() < 0.42) {
        const fr = front(); const bw = fr.map(c => [c.no, player(c.no).pos === 'M' ? 5 : 2]);
        const b = pick(rnd, bw);
        const bg = pick(rnd, [['#', 22], ['+', 25], ['!', 30], ['-', 15], ['=', 8]]);
        push({ set: st.set, skill: 'B', grade: bg, player: b });
        if (outcome('B', bg)) return;
      }
      const bk = back(); const dw = bk.map(c => [c.no, c.libero ? 6 : player(c.no).pos === 'A' ? 4 : 2]);
      const d = pick(rnd, dw);
      const dg = pick(rnd, [['#', 15], ['+', 30], ['!', 25], ['-', 15], ['=', 15]]);
      push({ set: st.set, skill: 'D', grade: dg, player: d });
      if (outcome('D', dg)) return;
      ourAttack(RECEPTION_SCORE[dg]);
    }
    function rally() {
      if (st.serving) {
        const srv = st.lineup[0];
        const g = pick(rnd, [['#', 6], ['+', 26], ['!', 36], ['-', 20], ['=', 12]]);
        push({ set: st.set, skill: 'S', grade: g, player: srv });
        if (outcome('S', g)) return;
        oppAttack();
      } else {
        const bk = back(); const rw = bk.map(c => [c.no, c.libero ? 7 : player(c.no).pos === 'A' ? 5 : 1]);
        const rec = pick(rnd, rw);
        const g = pick(rnd, [['#', 28], ['+', 30], ['!', 22], ['-', 10], ['/', 3], ['=', 7]]);
        push({ set: st.set, skill: 'R', grade: g, player: rec });
        if (outcome('R', g)) return;
        ourAttack(RECEPTION_SCORE[g]);
      }
    }
    let guard = 0;
    while (!st.finished && guard++ < 2000) {
      if (stopAt && st.set === stopAt.set && (st.us + st.them) >= stopAt.points) break;
      rally();
    }
    return log;
  }

  // ---------- persistence ----------
  const KEY = 'so_actions_v1';
  function load() {
    try { const s = localStorage.getItem(KEY); if (s) return JSON.parse(s); } catch (e) {}
    return seed();
  }
  function seed() { return simulate(20260919, { set: 3, points: 18 }); }
  const pastCache = {};
  function pastLog(m) { return pastCache[m.id] || (pastCache[m.id] = simulate(m.seed, null, m)); }
  function save(actions) { try { localStorage.setItem(KEY, JSON.stringify(actions)); } catch (e) {} }
  function reset() { try { localStorage.removeItem(KEY); } catch (e) {} return seed(); }

  const pct = (x, d) => x == null ? '–' : (x * 100).toFixed(d == null ? 0 : d) + ' %';
  const fix = (x, d) => x == null ? '–' : x.toFixed(d == null ? 2 : d);
  const eff = x => x == null ? '–' : (x < 0 ? '−' : '') + '.' + String(Math.round(Math.abs(x) * 1000)).padStart(3, '0');

  return { SKILLS, SKILL, GRADES, GRADE_CLASS, GRADE_NAME, PAD, POS_NAME, TEAM, ROSTER, MATCH, PAST, UPCOMING, pastLog,
    player, firstName, outcome, replay, courtView, stats, simulate, load, save, reset, seed, pct, fix, eff, setTarget };
})();
