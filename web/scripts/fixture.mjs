// Generates server/tests/fixtures/match.json from the JS engine so the Rust
// engine test (`parity_with_js_fixture`) proves both implementations agree.
// Run: npm run fixture
import { replay, stats } from '../src/lib/engine.js';
import { writeFileSync, mkdirSync } from 'node:fs';

function mulberry32(a) {
  return function () {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rnd = mulberry32(4242);
const pick = (t) => { const tot = t.reduce((s, x) => s + x[1], 0); let x = rnd() * tot; for (const [v, w] of t) { x -= w; if (x <= 0) return v; } return t[t.length - 1][0]; };

const players = [
  { id: 11, number: 1, name: 'Lena', position: 'Z' }, { id: 12, number: 3, name: 'Mia', position: 'A' },
  { id: 13, number: 4, name: 'Sara', position: 'M' }, { id: 14, number: 5, name: 'Jule', position: 'D' },
  { id: 15, number: 7, name: 'Nele', position: 'A' }, { id: 16, number: 8, name: 'Emma', position: 'M' },
  { id: 17, number: 12, name: 'Jo', position: 'L' }, { id: 18, number: 9, name: 'Kim', position: 'A' }
];
const cfg = { first_serve_us: true, lineups: { 1: { pos: [14, 12, 13, 11, 15, 16], libero: 17 }, 3: { pos: [12, 13, 11, 15, 16, 14], libero: 17 } } };
const actions = [];
let seq = 0;
const push = (a) => actions.push({ id: ++seq, seq, grade: null, player_id: null, sub_out: null, sub_in: null, ...a });

let st = replay(cfg, actions);
for (let guard = 0; guard < 3000 && !st.finished; guard++) {
  const onCourt = st.lineup;
  const who = () => onCourt[Math.floor(rnd() * 6)];
  const r = rnd();
  if (r < 0.03 && st.rally > 3) push({ skill: 'sub', sub_out: onCourt[Math.floor(rnd() * 6)], sub_in: 18 });
  else if (r < 0.15) push({ skill: 'opp', grade: pick([['#', 55], ['=', 45]]) });
  else {
    const skill = st.serving && st.rows.filter((x) => x.set === st.set && x.rally === st.rally).length === 0 ? 'S' : pick([['R', 20], ['E', 25], ['A', 30], ['B', 10], ['D', 15]]);
    const grade = pick([['#', 22], ['+', 25], ['!', 22], ['-', 14], ['/', 5], ['=', 12]]);
    const valid = { S: '#+!-=', R: '#+!-/=', E: '#+!-=', A: '#+!-/=', B: '#+!-=', D: '#+!-=' }[skill];
    push({ skill, grade: valid.includes(grade) ? grade : '!', player_id: skill === 'S' ? onCourt[0] : rnd() < 0.2 ? 17 : who() });
  }
  st = replay(cfg, actions);
}
const s = stats(cfg, players, actions, 0);
const fixture = {
  config: cfg,
  players,
  actions,
  expected: {
    set: st.set, us: st.us, them: st.them, lineup: st.lineup, serving: st.serving, sets: st.sets,
    sideout: s.team.sideout, brk: s.team.brk, ptsBy: s.team.ptsBy,
    players: s.players.map((p) => ({ id: p.id, pts: p.pts, k: p.A.k, rsum: p.R.sum, ast: p.E.ast }))
  }
};
mkdirSync(new URL('../../server/tests/fixtures/', import.meta.url), { recursive: true });
writeFileSync(new URL('../../server/tests/fixtures/match.json', import.meta.url), JSON.stringify(fixture, null, 1));
console.log(`fixture: ${actions.length} actions, ${st.sets.map((x) => x.us + ':' + x.them).join(' ')}${st.finished ? '' : ` (set ${st.set} ${st.us}:${st.them})`}`);
