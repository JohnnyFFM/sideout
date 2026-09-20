// handover, part 3 (8081): lineup editor under ownership, live → editor → live,
// pending offline work blocks a lineup change, "scoutet:" hints on Spiele and the home card
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const [outDir] = process.argv.slice(2);
const origin = 'http://127.0.0.1:8081'; const M = 1;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0; const check = (name, cond, detail) => { console.log((cond ? 'ok  ' : 'FAIL') + ' ' + name + (cond ? '' : '  ' + JSON.stringify(detail))); if (!cond) fails++; };
const procs = []; const RUN = Date.now().toString(36);

async function browser(port, profile, user, width = 390, height = 844) {
  const edge = spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${port}`, `--user-data-dir=${outDir}/${profile}-${RUN}`, '--no-first-run', `--window-size=${width},${height}`, 'about:blank'], { stdio: 'ignore' });
  procs.push(edge);
  let list; for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); break; } catch { await sleep(200); } }
  const ws = new WebSocket(list.find((t) => t.type === 'page').webSocketDebuggerUrl); await new Promise((r) => (ws.onopen = r));
  let id = 0; const pending = new Map(); const errors = [];
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } if (d.method === 'Runtime.exceptionThrown') errors.push((d.params.exceptionDetails?.exception?.description || '').split('\n')[0]); };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) return 'EXC:' + (r.result.exceptionDetails.exception?.description || '').split('\n')[0]; return r.result?.result?.value; };
  await send('Runtime.enable'); await send('Network.enable');
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 2, mobile: width < 700 });
  await send('Page.navigate', { url: `${origin}/login` }); await sleep(1200);
  await ev(`fetch('/api/auth/login',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({username:'${user}',password:'geheim123'})}).then(r=>r.status)`);
  const go = async (path, ms = 2500) => { await send('Page.navigate', { url: origin + path }); await sleep(ms); };
  const txt = (sel) => ev(`document.querySelector('${sel}')?.textContent.replace(/\\s+/g,' ').trim() || ''`);
  const offline = (on) => send('Network.emulateNetworkConditions', { offline: on, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  const shot = async (name) => { const r = await send('Page.captureScreenshot', { format: 'png' }); writeFileSync(`${outDir}/${name}.png`, Buffer.from(r.result.data, 'base64')); };
  const live = () => ev(`JSON.stringify({ lease: !!(JSON.parse(localStorage.getItem('so_lease_${M}')||'null')?.lease), queue: (JSON.parse(localStorage.getItem('so_ops_${M}')||'[]')).length, banner: document.querySelector('.scoutbar')?.textContent.replace(/\\s+/g,' ').trim() || '', padOn: document.querySelectorAll('.cell:not(:disabled)').length - 3 })`).then((s) => JSON.parse(s));
  const editor = () => ev(`JSON.stringify({ notice: document.querySelector('form .scoutbar')?.textContent.replace(/\\s+/g,' ').trim() || '', takeover: !!document.querySelector('form .scoutbar .btn'), saveOn: !document.querySelector('button[type=submit]')?.disabled, err: document.querySelector('.err')?.textContent.trim() || '', hscroll: document.documentElement.scrollWidth <= innerWidth + 1 })`).then((s) => JSON.parse(s));
  return { send, ev, go, txt, offline, shot, live, editor, errors };
}
const lr = await fetch(`${origin}/api/auth/login`, { method: 'POST', headers: { 'X-Requested-By': 'x', 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'jonas', password: 'geheim123' }) });
const cookie = lr.headers.get('set-cookie').split(';')[0];
const server = async () => { const m = await (await fetch(`${origin}/api/matches/${M}`, { headers: { cookie } })).json(); return { seq: m.state.last_seq, held: m.scout.held, actor: m.scout.actor, rev: m.scout.revision, lineups: Object.keys(m.lineups), first_serve: m.first_serve, status: m.status }; };

const A = await browser(9400, 'edge-profile80', 'jonas');
const B = await browser(9401, 'edge-profile81', 'petra.k');

// A scouts (entry claim) and records one point so the match is live
await A.go(`/live/${M}`);
await A.ev(`document.querySelector('.btn.big.us').click()`); await sleep(1500);
let sv = await server();
check('A holds and the match is live', sv.actor === 'Jonas Steitz' && sv.status === 'live', { sv });

// 1. B opens the lineup editor: notice with actor, save disabled, explicit takeover offered
await B.go(`/spiele/${M}?set=2`);
let e = await B.editor();
check('editor (other device): notice names the holder, save disabled, takeover offered', e.notice.includes('Jonas Steitz') && !e.saveOn && e.takeover && e.hscroll, { e });
await B.shot('editor-notice-phone');
// 2. explicit takeover in the editor, then save set 2 → live page holds
await B.ev(`document.querySelector('form .scoutbar .btn').click()`); await sleep(2000);
e = await B.editor();
check('after takeover the notice is gone and save is enabled', !e.notice && e.saveOn, { e });
await B.ev(`document.querySelector('button[type=submit]').click()`); await sleep(3000);
sv = await server();
check('lineup for set 2 saved under B\'s lease, B lands on the live page as holder', sv.lineups.includes('2') && sv.actor === 'Petra Kuhn' && (await B.ev('location.pathname')) === `/live/${M}` && (await B.live()).lease, { sv, path: await B.ev('location.pathname') });

// 3. A (displaced) opens the editor: refused to save; the live page shows the banner
await A.go(`/spiele/${M}?set=2`);
e = await A.editor();
check('A in the editor: holder notice, save disabled', e.notice.includes('Petra Kuhn') && !e.saveOn, { e });
await A.go(`/live/${M}`);
check('A live: read-only with banner', (await A.live()).banner.includes('Petra Kuhn') && (await A.live()).padOn === 0);

// 4. live → editor → live for the holder itself (B): no takeover needed, first_serve change accepted
await B.go(`/live/${M}`);
await B.ev(`(()=>{const a=document.createElement('a'); a.href='/spiele/${M}?set=3'; document.body.appendChild(a); a.click();})()`); await sleep(2500);
e = await B.editor();
check('holder in the editor: no notice, save enabled', !e.notice && e.saveOn, { e });
await B.ev(`[...document.querySelectorAll('.seg button')].find(b=>b.textContent==='Gegner').click()`); await sleep(200);
await B.ev(`document.querySelector('button[type=submit]').click()`); await sleep(3000);
sv = await server();
check('first serve + lineup 3 saved by the holder, back on live', sv.first_serve === 'them' && sv.lineups.includes('3') && (await B.ev('location.pathname')) === `/live/${M}`, { sv });

// 5. unsent work of this device blocks a lineup change until the live page has sent it
//    (an op left in the queue as the live page leaves it when the connection is gone)
await B.ev(`localStorage.setItem('so_ops_${M}', JSON.stringify([{ type: 'add', sent: false, action: { id: -2, seq: 2, cid: 'pending-x', skill: 'opp', grade: '=', player_id: null, sub_out: null, sub_in: null } }]))`);
await B.go(`/spiele/${M}?set=4`);
e = await B.editor();
check('editor with unsent actions: save blocked with the hint', !e.saveOn && (e.err.includes('nicht gesendete') || (await B.txt('form')).includes('nicht gesendete')), { e });
await B.go(`/live/${M}`, 3500);
check('the live page sends the queued action', (await B.live()).queue === 0 && (await server()).seq === 2, { l: await B.live(), sv: await server() });

// 6. Spiele list and home show who scouts (from A's point of view: B holds)
await A.go('/spiele');
const row = await A.txt('.matches li');
check('Spiele row: "scoutet: Petra Kuhn"', row.includes('scoutet: Petra Kuhn'), { row });
await A.go('/team');
const card = await A.txt('.livecard');
check('home live card: "scoutet: Petra Kuhn"', card.includes('scoutet: Petra Kuhn'), { card });
await A.shot('home-scoutet-phone');
// after B releases (leaves), the hint disappears on a scout event
await B.go('/spiele', 1500);
let gone = false; for (let i = 0; i < 20 && !gone; i++) { await sleep(300); gone = !(await A.txt('.livecard')).includes('scoutet:'); }
check('home card drops the hint after the release event', gone);
// B (holder's own device) never sees itself as "elsewhere"
await B.go(`/live/${M}`, 2000); await B.go('/spiele', 2000);
check('own device: no "scoutet:" for its own lease', !(await B.txt('.matches li')).includes('scoutet:') || !(await server()).held);

console.log('js errors:', [...A.errors, ...B.errors].length ? [...A.errors, ...B.errors] : 'none');
console.log(fails ? `${fails} FAILED` : 'all part-3 checks passed');
for (const p of procs) p.kill();
process.exit(fails ? 1 : 0);
