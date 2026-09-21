// voice scouting: a fake microphone plays synthesized German commands ("drei Angriff Punkt", …),
// the panel turns them into actions on /live/1. Needs the model in static/models (npm run voice-model)
// and a WAV (48 kHz, 16-bit mono): node voice.mjs <outdir> <wav>
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const [outDir, wav] = process.argv.slice(2);
const origin = 'http://127.0.0.1:8081';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0; const check = (name, cond, detail) => { console.log((cond ? 'ok  ' : 'FAIL') + ' ' + name + (cond ? '' : '  ' + JSON.stringify(detail))); if (!cond) fails++; };
const PORT = 9403;
const edge = spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${PORT}`, `--user-data-dir=${outDir}/edge-profile-voice-${Date.now().toString(36)}`, '--no-first-run', '--window-size=1280,900',
  '--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', `--use-file-for-fake-audio-capture=${wav}%noloop`, '--autoplay-policy=no-user-gesture-required', 'about:blank'], { stdio: 'ignore' });
let list; for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json(); break; } catch { await sleep(200); } }
const ws = new WebSocket(list.find((t) => t.type === 'page').webSocketDebuggerUrl); await new Promise((r) => (ws.onopen = r));
let id = 0; const pending = new Map(); const errors = []; const heard = [];
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === 'Runtime.exceptionThrown') errors.push((d.params.exceptionDetails?.exception?.description || '').split('\n')[0]);
  if (d.method === 'Runtime.consoleAPICalled' && d.params.args?.[0]?.value === '[voice]') heard.push(d.params.args.slice(1).map((a) => a.value).join(' → ')); };
const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) return 'EXC:' + (r.result.exceptionDetails.exception?.description || '').split('\n')[0]; return r.result?.result?.value; };
const shot = async (name) => { const r = await send('Page.captureScreenshot', { format: 'png' }); writeFileSync(`${outDir}/${name}.png`, Buffer.from(r.result.data, 'base64')); };
await send('Runtime.enable');
await send('Page.navigate', { url: `${origin}/login` }); await sleep(1200);
await ev(`fetch('/api/auth/login',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({username:'jonas',password:'geheim123'})}).then(r=>r.status)`);
await ev(`localStorage.setItem('so_voice', JSON.stringify({ mode: 'cont', key: 'Space' }))`);
await send('Page.navigate', { url: `${origin}/live/1` }); await sleep(2500);
check('voice panel present and off', (await ev(`document.querySelector('.voice .status')?.textContent`)) === 'aus');
const t0 = Date.now();
await ev(`[...document.querySelectorAll('.voice .btn')].find(b => b.textContent.trim() === 'Einschalten').click()`);
let status = ''; for (let i = 0; i < 120; i++) { status = await ev(`document.querySelector('.voice .status')?.textContent`); if (status === 'hört zu' || status.startsWith('Start fehl') || status.startsWith('Fehler')) break; await sleep(500); }
check('model loads and the panel listens (continuous mode)', status === 'hört zu', { status, ms: Date.now() - t0 });
console.log('model + mic ready after', Date.now() - t0, 'ms');
// the WAV: 2 s lead, five utterances 2.5 s apart (~17 s)
for (let i = 0; i < 70 && heard.length < 5; i++) await sleep(500);
await sleep(1500);
console.log('heard:', heard);
const m = await ev(`fetch('/api/matches/1',{headers:{'X-Requested-By':'x'}}).then(r=>r.json()).then(m=>JSON.stringify({ acts: m.actions.map(a=>[a.skill,a.grade,m.players.find(p=>p.id===a.player_id)?.number ?? null]) }))`);
const acts = JSON.parse(m).acts;
check('four actions recorded: 3 A#, 4 B#, 1 E+, Fehler Gegner', JSON.stringify(acts) === JSON.stringify([['A', '#', 3], ['B', '#', 4], ['E', '+', 1], ['opp', '=', null]]), acts);
check('the coaching phrase was rejected, not recorded', heard.some((h) => /fehlt|unbekannt|zu viele/.test(h)), heard);
check('last-action box shows the voice action', /Fehler Gegner/.test(await ev(`document.querySelector('#lastBox .txt')?.textContent`)));
check('echo lines rendered', (await ev(`document.querySelectorAll('.voice .line').length`)) >= 1);
await shot('voice-desktop');
await ev(`[...document.querySelectorAll('.voice .btn')].find(b => b.textContent.trim() === 'Aus').click()`); await sleep(300);
check('switches off', (await ev(`document.querySelector('.voice .status')?.textContent`)) === 'aus');
console.log('js errors:', errors.length ? errors : 'none');
console.log(fails ? `${fails} FAILED` : 'all voice checks passed');
edge.kill(); process.exit(fails ? 1 : 0);
