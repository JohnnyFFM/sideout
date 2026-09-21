// Voice scouting (prototype). One spoken utterance = one action:
//
//   "<Nummer oder Vorname> <Skill> <Note>"   e.g. "zwölf Block Punkt"
//   "Fehler Gegner" / "Punkt Gegner"          the two team buttons
//
// Recognition runs on the device (Vosk, WebAssembly, small German model,
// fetched once from static/models). The recognizer is given a *grammar*:
// only the words below plus the roster's numbers and first names can come
// out of it, everything else maps to [unk]. That is what makes a 40-word
// vocabulary work in a loud gym. The parser is strict on purpose: exactly
// one player, one skill, one grade and nothing else, or the utterance is
// dropped with a reason. A missed action costs one tap; a wrong one
// costs a statistic.
import { PAD, SKILL } from './engine.js';

// ---------------------------------------------------------------- words

const ONES = ['', 'ein', 'zwei', 'drei', 'vier', 'fünf', 'sechs', 'sieben', 'acht', 'neun'];
const TEENS = ['zehn', 'elf', 'zwölf', 'dreizehn', 'vierzehn', 'fünfzehn', 'sechzehn', 'siebzehn', 'achtzehn', 'neunzehn'];
const TENS = ['', '', 'zwanzig', 'dreißig', 'vierzig', 'fünfzig', 'sechzig', 'siebzig', 'achtzig', 'neunzig'];

/** German number word for 1–99 (as the acoustic model spells it) */
export function numberWord(n) {
  if (n === 1) return 'eins';
  if (n < 10) return ONES[n];
  if (n < 20) return TEENS[n - 10];
  if (n % 10 === 0) return TENS[n / 10];
  return ONES[n % 10] + 'und' + TENS[Math.floor(n / 10)];
}

/** spoken skill → Data-Volley skill key */
export const SKILL_WORDS = {
  aufschlag: 'S', service: 'S',
  annahme: 'R',
  zuspiel: 'E', pass: 'E',
  angriff: 'A', schlag: 'A',
  block: 'B',
  abwehr: 'D'
};

/** spoken grade → Data-Volley grade. Several ways to say each one. */
export const GRADE_WORDS = {
  punkt: '#', ass: '#', perfekt: '#', kill: '#', super: '#', top: '#',
  gut: '+', plus: '+', positiv: '+',
  okay: '!', neutral: '!', null: '!',
  schwach: '-', minus: '-', schlecht: '-', negativ: '-',
  geblockt: '/', blockiert: '/', drüber: '/', rüber: '/', halb: '/',
  fehler: '=', raus: '=', aus: '=', netz: '=', daneben: '='
};

const TEAM = { gegner: true };

// ---------------------------------------------------------------- grammar

const clean = (s) => (s || '').toLowerCase().replace(/[^a-zäöüß]/g, '');

/** per-player spoken tokens: the number word and the first name (when the
 *  first name is unambiguous in the roster and a single plain word) */
export function playerWords(players) {
  const firsts = {};
  for (const p of players) { const f = clean((p.name || '').split(' ')[0]); if (f) firsts[f] = (firsts[f] || 0) + 1; }
  const map = new Map(); // word → player id
  for (const p of players) {
    if (p.number >= 1 && p.number <= 99) map.set(numberWord(p.number), p.id);
    if (p.number === 2) map.set('zwo', p.id);
    const f = clean((p.name || '').split(' ')[0]);
    if (f && f.length >= 2 && firsts[f] === 1 && !SKILL_WORDS[f] && !GRADE_WORDS[f] && !TEAM[f]) map.set(f, p.id);
  }
  return map;
}

/** the word list handed to the recognizer (JSON array, "[unk]" catches the rest) */
export function grammar(players) {
  const words = new Set([...playerWords(players).keys(), ...Object.keys(SKILL_WORDS), ...Object.keys(GRADE_WORDS), ...Object.keys(TEAM), '[unk]']);
  return [...words];
}

// ---------------------------------------------------------------- parser

/**
 * parse(text, players, onCourt) →
 *   { ok: true, action: { skill, grade, player_id }, label }
 *   { ok: false, reason }
 * `onCourt`: ids allowed as actors (the six on the court incl. the libero);
 * omit to accept the whole roster.
 */
export function parse(text, players, onCourt = null) {
  const words = playerWords(players);
  const byId = Object.fromEntries(players.map((p) => [p.id, p]));
  const toks = (text || '').toLowerCase().trim().split(/\s+/).filter(Boolean);
  if (!toks.length) return { ok: false, reason: 'nichts verstanden' };
  if (toks.includes('[unk]')) return { ok: false, reason: `unbekanntes Wort in „${toks.join(' ')}“` };

  // team commands: exactly two words
  if (toks.includes('gegner')) {
    const other = toks.filter((t) => t !== 'gegner');
    if (toks.length === 2 && other[0] === 'fehler') return { ok: true, action: { skill: 'opp', grade: '=' }, label: 'Fehler Gegner' };
    if (toks.length === 2 && other[0] === 'punkt') return { ok: true, action: { skill: 'opp', grade: '#' }, label: 'Punkt Gegner' };
    return { ok: false, reason: '„Fehler Gegner“ oder „Punkt Gegner“' };
  }

  const pl = toks.filter((t) => words.has(t));
  const sk = toks.filter((t) => SKILL_WORDS[t]);
  const gr = toks.filter((t) => GRADE_WORDS[t] && !words.has(t));
  if (pl.length !== 1 || sk.length !== 1 || gr.length !== 1 || toks.length !== 3) {
    const miss = [];
    if (pl.length !== 1) miss.push(pl.length ? 'eine Spielerin' : 'Spielerin');
    if (sk.length !== 1) miss.push(sk.length ? 'eine Aktion' : 'Aktion');
    if (gr.length !== 1) miss.push(gr.length ? 'eine Note' : 'Note');
    return { ok: false, reason: (miss.length ? 'fehlt: ' + miss.join(', ') : 'zu viele Wörter') + ` („${toks.join(' ')}“)` };
  }
  const player_id = words.get(pl[0]);
  const skill = SKILL_WORDS[sk[0]];
  const grade = GRADE_WORDS[gr[0]];
  const p = byId[player_id];
  if (onCourt && !onCourt.includes(player_id)) return { ok: false, reason: `${p?.number} ${firstOf(p)} steht nicht auf dem Feld` };
  if (!PAD[skill][grade]) return { ok: false, reason: `${SKILL[skill].name} kennt keine Note „${gr[0]}“` };
  return { ok: true, action: { skill, grade, player_id }, label: `${p?.number} ${firstOf(p)} · ${SKILL[skill].name} ${grade} ${PAD[skill][grade]}` };
}

const firstOf = (p) => (p?.name || '?').split(' ')[0];

// ---------------------------------------------------------------- recognizer

/**
 * Wraps vosk-browser: model in a worker, microphone through an AudioContext.
 * Audio only reaches the recognizer while `gate` is open: in hold mode the
 * caller opens it on key/pointer down and closes it on release (the release
 * flushes a final result), in continuous mode it stays open and Vosk's own
 * end-pointing splits utterances at ~0.5 s of silence.
 *
 * start({ modelUrl, grammar, onState, onPartial, onResult }) → controller
 *   controller.gate(open: boolean)   controller.stop()
 */
export async function start({ modelUrl, grammar: words, onState, onPartial, onResult }) {
  // AudioContext and microphone first, synchronously inside the tap that
  // called us: iOS/Android only grant them from a user gesture, and the
  // gesture is spent once the model download below has been awaited
  const ctx = new AudioContext();
  ctx.resume().catch(() => {});
  onState?.('Mikrofon …');
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, channelCount: 1 }, video: false });
  onState?.('lädt Modell …');
  let model;
  try {
    const { createModel } = await import('vosk-browser');
    model = await createModel(modelUrl);
  } catch (e) { stream.getTracks().forEach((t) => t.stop()); ctx.close().catch(() => {}); throw e; }
  const rec = new model.KaldiRecognizer(ctx.sampleRate, JSON.stringify(words));
  rec.on('result', (m) => { const t = (m.result?.text || '').trim(); if (t) onResult?.(t); });
  rec.on('partialresult', (m) => onPartial?.((m.result?.partial || '').trim()));
  rec.on('error', (m) => onState?.('Fehler: ' + m.error));
  let open = false;
  const node = ctx.createScriptProcessor(4096, 1, 1);
  node.onaudioprocess = (e) => { if (open) try { rec.acceptWaveform(e.inputBuffer); } catch { /* worker gone */ } };
  const src = ctx.createMediaStreamSource(stream);
  src.connect(node);
  node.connect(ctx.destination); // a ScriptProcessor only runs when it is wired to the graph; it outputs silence
  onState?.('bereit');
  return {
    gate(v) { if (open && !v) rec.retrieveFinalResult(); open = v; },
    stop() { open = false; try { node.disconnect(); src.disconnect(); } catch {} stream.getTracks().forEach((t) => t.stop()); ctx.close().catch(() => {}); try { rec.remove(); model.terminate(); } catch {} }
  };
}
