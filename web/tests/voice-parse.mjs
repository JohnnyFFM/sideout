// parser + grammar checks for voice scouting: node tests/voice-parse.mjs
import assert from 'node:assert/strict';
import { grammar, parse, numberWord } from '../src/lib/voice.js';
const players = [
  { id: 1, number: 1, name: 'Lena Vogt', active: true }, { id: 3, number: 3, name: 'Mia Brandt', active: true }, { id: 4, number: 4, name: 'Sara Kunz', active: true },
  { id: 5, number: 5, name: 'Jule Hartmann', active: true }, { id: 7, number: 7, name: 'Nele Fischer', active: true }, { id: 8, number: 8, name: 'Emma Roth', active: true },
  { id: 12, number: 12, name: 'Jo Lindner', active: true }, { id: 9, number: 9, name: 'Kim Sauer', active: true }, { id: 22, number: 22, name: 'Lena Berg', active: true }
];
const court = [1, 3, 4, 5, 7, 12];
assert.equal(numberWord(1), 'eins'); assert.equal(numberWord(12), 'zwölf'); assert.equal(numberWord(21), 'einundzwanzig'); assert.equal(numberWord(30), 'dreißig');
const g = grammar(players);
assert.ok(g.includes('zwölf') && g.includes('mia') && g.includes('[unk]') && g.includes('gegner') && g.includes('block'));
assert.ok(!g.includes('lena'), 'ambiguous first name is left out'); assert.ok(g.includes('zweiundzwanzig'));
let r = parse('drei angriff punkt', players, court); assert.deepEqual(r.action, { skill: 'A', grade: '#', player_id: 3 }); assert.match(r.label, /3 Mia · Angriff # Punkt/);
r = parse('punkt drei angriff', players, court); assert.equal(r.ok, true, 'order does not matter');
r = parse('mia annahme gut', players, court); assert.deepEqual(r.action, { skill: 'R', grade: '+', player_id: 3 });
r = parse('zwölf block punkt', players, court); assert.equal(r.action.player_id, 12);
r = parse('fehler gegner', players, court); assert.deepEqual(r.action, { skill: 'opp', grade: '=' });
r = parse('gegner punkt', players, court); assert.deepEqual(r.action, { skill: 'opp', grade: '#' });
r = parse('neun block punkt', players, court); assert.equal(r.ok, false); assert.match(r.reason, /nicht auf dem Feld/);
r = parse('drei aufschlag geblockt', players, court); assert.equal(r.ok, false); assert.match(r.reason, /keine Note/);
r = parse('drei angriff', players, court); assert.equal(r.ok, false); assert.match(r.reason, /fehlt: Note/);
r = parse('drei angriff punkt super', players, court); assert.equal(r.ok, false, 'two grades');
r = parse('drei [unk] punkt', players, court); assert.equal(r.ok, false); assert.match(r.reason, /unbekannt/);
r = parse('', players, court); assert.equal(r.ok, false);
r = parse('drei vier angriff punkt', players, court); assert.equal(r.ok, false, 'two players');
r = parse('lena angriff punkt', players, court); assert.equal(r.ok, false, 'ambiguous name is not a player word');
r = parse('drei angriff punkt', players, null); assert.equal(r.ok, true, 'no court restriction');
console.log('voice parser: all checks passed');
