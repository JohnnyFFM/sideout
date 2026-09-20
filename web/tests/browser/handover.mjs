// scouting handover in the browser, two devices (separate profiles) against 8081:
// entry claim, banner + takeover, loss on the old device, offline queue after
// takeover (committed retries not counted), release after the final point,
// undo after completion, viewer read-only, error envelope through the wrapper
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const [outDir] = process.argv.slice(2);
const origin = 'http://127.0.0.1:8081'; const M = 1;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0; const check = (name, cond, detail) => { console.log((cond ? 'ok  ' : 'FAIL') + ' ' + name + (cond ? '' : '  ' + JSON.stringify(detail))); if (!cond) fails++; };
const procs = [];
const RUN = Date.now().toString(36);

async function browser(port, profile, user, width = 390, height = 844) {
  const edge = spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${port}`, `--user-data-dir=${outDir}/${profile}-${RUN}`, '--no-first-run', `--window-size=${width},${height}`, 'about:blank'], { stdio: 'ignore' });
  procs.push(edge);
  let list; for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); break; } catch { await sleep(200); } }
  const page = list.find((t) => t.type === 'page');
  const ws = new WebSocket(page.webSocketDebuggerUrl); await new Promise((r) => (ws.onopen = r));
  let id = 0; const pending = new Map(); const errors = []; let mode = 'pass'; const intercepted = [];
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
    if (d.method === 'Runtime.exceptionThrown') errors.push((d.params.exceptionDetails?.exception?.description || '').split('\n')[0]);
    if (d.method === 'Fetch.requestPaused') {
      const { requestId, request, responseStatusCode } = d.params;
      const isAct = request.url.includes(`/api/matches/${M}/actions`) && request.method === 'POST';
      if (responseStatusCode === undefined) { send('Fetch.continueRequest', { requestId }); return; }
      if (isAct && mode === 'fail-post') { mode = 'pass'; intercepted.push('POST answer dropped'); send('Fetch.failRequest', { requestId, errorReason: 'Failed' }); return; }
      if (isAct && mode === 'delay-post') { mode = 'pass'; intercepted.push('POST answer held'); setTimeout(() => send('Fetch.continueRequest', { requestId }), 2500); return; }
      send('Fetch.continueRequest', { requestId });
    }
  };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) return 'EXC:' + (r.result.exceptionDetails.exception?.description || '').split('\n')[0]; return r.result?.result?.value; };
  await send('Runtime.enable'); await send('Network.enable');
  await send('Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Response' }] });
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 2, mobile: width < 700 });
  await send('Page.navigate', { url: `${origin}/login` }); await sleep(1200);
  await ev(`fetch('/api/auth/login',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({username:'${user}',password:'geheim123'})}).then(r=>r.status)`);
  const go = async (path, ms = 2500) => { await send('Page.navigate', { url: origin + path }); await sleep(ms); };
  const txt = (sel) => ev(`document.querySelector('${sel}')?.textContent.replace(/\\s+/g,' ').trim() || ''`);
  const offline = (on) => send('Network.emulateNetworkConditions', { offline: on, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  const shot = async (name) => { const r = await send('Page.captureScreenshot', { format: 'png' }); writeFileSync(`${outDir}/${name}.png`, Buffer.from(r.result.data, 'base64')); };
  const state = () => ev(`JSON.stringify({ us: +document.querySelector('.score .side.us .pts')?.textContent, queue: (JSON.parse(localStorage.getItem('so_ops_${M}')||'[]')).length, lease: !!(JSON.parse(localStorage.getItem('so_lease_${M}')||'null')?.lease), banner: document.querySelector('.scoutbar')?.textContent.replace(/\\s+/g,' ').trim() || '', padOn: document.querySelectorAll('.cell:not(:disabled)').length - 3, oppOn: !document.querySelector('.btn.big.us')?.disabled, undoOn: !document.querySelector('#lastBox button')?.disabled, dropped: (JSON.parse(localStorage.getItem('so_ops_dropped_${M}')||'null')?.ops||[]).length })`).then((s) => JSON.parse(s));
  const tap = () => ev(`(()=>{const b=document.querySelector('.btn.big.us'); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
  const undo = () => ev(`(()=>{const b=document.querySelector('#lastBox button'); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
  return { edge, send, ev, go, txt, offline, shot, state, tap, undo, errors, intercepted, setMode: (m) => { mode = m; } };
}
// server truth via node (own session)
const lr = await fetch(`${origin}/api/auth/login`, { method: 'POST', headers: { 'X-Requested-By': 'x', 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'jonas', password: 'geheim123' }) });
const cookie = lr.headers.get('set-cookie').split(';')[0];
const server = async () => { const m = await (await fetch(`${origin}/api/matches/${M}`, { headers: { cookie } })).json(); return { seq: m.state.last_seq, us: m.state.us, status: m.status, held: m.scout.held, actor: m.scout.actor, rev: m.scout.revision, stale: m.scout.stale }; };

const A = await browser(9390, 'edge-profile60', 'jonas');        // device A: coach
const B = await browser(9391, 'edge-profile61', 'petra.k');      // device B: scout (assistant)
const V = await browser(9392, 'edge-profile62', 'maxv', 1100, 800); // viewer, desktop

// 1. entry claim on A: writes work, no banner
await A.go(`/live/${M}`);
let sA = await A.state(); let sv = await server();
check('A claims on entry: lease held, pad enabled, server shows holder', sA.lease && sA.padOn > 0 && sA.oppOn && sv.held && sv.actor === 'Jonas Steitz' && !sA.banner, { sA, sv });
await A.tap(); await sleep(1200); sv = await server();
check('A writes with its lease', sv.seq === 1 && sv.us === 1, { sv });

// 2. B sees the banner, is read-only, error envelope through the wrapper
await B.go(`/live/${M}`);
let sB = await B.state();
check('B: banner with actor and time, no lease, scoring locked', /Jonas Steitz scoutet auf einem anderen Gerät.*seit \d\d:\d\d/.test(sB.banner) && !sB.lease && sB.padOn === 0 && !sB.oppOn && !sB.undoOn, { sB });
check('B: takeover button present', (await B.ev(`!!document.querySelector('.scoutbar .btn')`)) === true);
await B.shot('handover-banner-phone');
const env = await B.ev(`fetch('/api/matches/${M}/actions',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({seq:2,skill:'opp',grade:'=',cid:'probe-b'})}).then(async r=>{const d=await r.json(); return JSON.stringify({status:r.status, error:d.error, held:d.current?.scout?.held, mine:d.current?.scout?.mine})})`);
check('B: protected write refused with the scout envelope', env === JSON.stringify({ status: 409, error: 'scouted_elsewhere', held: true, mine: false }), { env });
check('B: tap and Ctrl+Z do nothing', (await B.tap()) === 'blocked' && (await B.undo()) === 'blocked' && ((await B.ev(`(()=>{document.dispatchEvent(new KeyboardEvent('keydown',{key:'z',ctrlKey:true,bubbles:true})); return 1})()`)) === 1) && (await server()).seq === 1);
// viewer: watches, no button
await V.go(`/live/${M}`);
const vb = await V.txt('.scoutbar');
check('viewer sees who scouts but no takeover button', vb.includes('Jonas Steitz') && !(await V.ev(`!!document.querySelector('.scoutbar .btn')`)) && (await V.state()).padOn === 0, { vb });

// 3. B takes over; A loses the lease live (scout event), its controls lock
await B.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(2000);
sB = await B.state(); sv = await server();
check('B holds after takeover', sB.lease && sB.padOn > 0 && !sB.banner && sv.actor === 'Petra Kuhn', { sB, sv });
let lost = false; for (let i = 0; i < 20 && !lost; i++) { await sleep(300); const s = await A.state(); lost = !s.lease && s.banner.includes('Petra Kuhn'); }
sA = await A.state();
check('A: lease gone, banner shows Petra, scoring locked, nothing dropped', lost && sA.padOn === 0 && sA.dropped === 0, { sA });
check('A: a tap after the loss does not reach the server', (await A.tap()) === 'blocked' && (await server()).seq === 1);
await B.tap(); await sleep(1200);
check('B writes', (await server()).seq === 2);

// 4. A takes back, goes offline, queues two taps; B takes over meanwhile and writes;
//    A returns: its queued work is set aside, not replayed, and reported
await A.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(2000);
sA = await A.state();
check('A took over again', sA.lease && sA.padOn > 0, { sA });
await A.offline(true); await sleep(300);
await A.tap(); await sleep(150); await A.tap(); await sleep(400);
sA = await A.state();
check('A offline: two taps queued under its lease', sA.queue === 2 && sA.us === 4, { sA });
await B.go(`/live/${M}`);
await B.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(2000);
await B.tap(); await sleep(1200);
sv = await server();
check('B took over while A is offline and wrote', sv.actor === 'Petra Kuhn' && sv.seq === 3, { sv });
await A.offline(false); await sleep(3500);
sA = await A.state(); sv = await server();
check('A back: queue set aside (2 dropped), nothing replayed, read-only with banner', sA.queue === 0 && sA.dropped === 2 && !sA.lease && sA.banner.includes('Petra Kuhn') && sv.seq === 3 && sA.us === 3, { sA, sv });

// 5. committed-but-unanswered retry is not counted as lost
await A.ev(`localStorage.removeItem('so_ops_dropped_${M}')`);
await A.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(2000);
A.setMode('fail-post');
await A.tap(); await sleep(1200);
sA = await A.state(); sv = await server();
check('A: answer lost, server holds seq 4, op still queued', sv.seq === 4 && sA.queue === 1, { sA, sv });
await B.go(`/live/${M}`);
await B.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(2500);
let settled = false; for (let i = 0; i < 20 && !settled; i++) { await sleep(300); const s = await A.state(); settled = s.queue === 0 && !s.lease; }
sA = await A.state();
check('A: after takeover the committed retry is settled by cid, dropped count 0', settled && sA.dropped === 0 && sA.us === 4, { sA });

// 6. new holder releases (leaves the page): A's old work still not replayed; A may claim afresh
await A.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(2000);
await A.offline(true); await sleep(300); await A.tap(); await sleep(400);
check('A offline again with one queued tap', (await A.state()).queue === 1);
await B.go(`/live/${M}`); await B.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(2000);
await B.go('/spiele'); await sleep(1500);   // B leaves → releases
sv = await server();
check('B leaving released the match', sv.held === false, { sv });
await A.offline(false); await sleep(4000);
sA = await A.state(); sv = await server();
check('A back to a free match: old op set aside (dropped 1), then a fresh claim, nothing replayed', sA.queue === 0 && sA.dropped === 1 && sv.seq === 4 && sA.lease && sv.actor === 'Jonas Steitz', { sA, sv });

// 7. final point pending vs confirmed, undo after completion
await A.ev(`localStorage.removeItem('so_ops_dropped_${M}')`);
// drive to one point before the end with catch-up points from node (as A's session would) — use the API with A's lease is not available here,
// so finish via the UI: 3 sets need many taps; use the server-side helper route instead: post adj points as a second coach session
{
  const lr2 = await fetch(`${origin}/api/auth/login`, { method: 'POST', headers: { 'X-Requested-By': 'x', 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'petra.k', password: 'geheim123' }) });
  const c2 = lr2.headers.get('set-cookie').split(';')[0];
  const m = await (await fetch(`${origin}/api/matches/${M}`, { headers: { cookie: c2 } })).json();
  const acq = await (await fetch(`${origin}/api/matches/${M}/scout`, { method: 'POST', headers: { cookie: c2, 'X-Requested-By': 'x', 'Content-Type': 'application/json' }, body: JSON.stringify({ mode: 'takeover', revision: m.scout.revision, request_id: 'node-finish' }) })).json();
  let seq = m.state.last_seq; const lease = acq.scout.lease;
  // set 1: us 4 already (4 opp errors) → 21 more; sets 2,3: 25 each; stop one point short
  for (let i = 0; i < 21 + 25 + 24; i++) { seq++; const r = await fetch(`${origin}/api/matches/${M}/actions`, { method: 'POST', headers: { cookie: c2, 'X-Requested-By': 'x', 'Content-Type': 'application/json', 'X-Scout-Lease': lease }, body: JSON.stringify({ seq, skill: 'adj', grade: '#', cid: 'n' + seq }) }); if (r.status !== 200) { console.log('finish setup failed', r.status, await r.text()); break; } }
  await fetch(`${origin}/api/matches/${M}/scout?lease=${lease}`, { method: 'DELETE', headers: { cookie: c2, 'X-Requested-By': 'x' } });
}
await A.go(`/live/${M}`);
sA = await A.state(); sv = await server();
check('A reclaims the free match one point before the end', sA.lease && sv.status === 'live', { sA, sv });
await A.offline(true); await sleep(300);
await A.tap(); await sleep(600);
sA = await A.state(); sv = await server();
check('final point pending offline: no release yet', sA.queue === 1 && sA.lease && (await server()).held === true, { sA });
await A.offline(false); await sleep(3500);
sA = await A.state(); sv = await server();
check('final point confirmed: match done and the lease released', sv.status === 'done' && sv.held === false && !sA.lease && sA.queue === 0, { sA, sv });
await A.shot('handover-finished');
await A.undo(); await sleep(2500);
sv = await server(); sA = await A.state();
check('undo after completion: fresh claim, final point removed, match live again', sv.status === 'live' && sA.lease && sv.actor === 'Jonas Steitz', { sv, sA });

// 8. banner geometry on the phone: scoring controls stay on screen
await B.go(`/live/${M}`);
const geo = await B.ev(`(()=>{const b=document.querySelector('.scoutbar')?.getBoundingClientRect(); const p=document.querySelector('.pad-foot')?.getBoundingClientRect(); return JSON.stringify({bannerH: b&&Math.round(b.height), padFootBottom: p&&Math.round(p.bottom), innerH: innerHeight, hscroll: document.documentElement.scrollWidth <= innerWidth + 1})})()`);
const g = JSON.parse(geo);
check('phone: banner compact, opponent buttons within the screen, no horizontal scroll', g.bannerH && g.bannerH <= 110 && g.padFootBottom <= g.innerH && g.hscroll, { g });
await B.send('Emulation.setDeviceMetricsOverride', { width: 844, height: 390, deviceScaleFactor: 2, mobile: true }); await sleep(800);
const geo2 = JSON.parse(await B.ev(`(()=>{const b=document.querySelector('.scoutbar')?.getBoundingClientRect(); const p=document.querySelector('.pad-foot')?.getBoundingClientRect(); return JSON.stringify({bannerH: b&&Math.round(b.height), padFootBottom: p&&Math.round(p.bottom), innerH: innerHeight, hscroll: document.documentElement.scrollWidth <= innerWidth + 1})})()`));
check('landscape: banner and buttons still fit', geo2.bannerH && geo2.padFootBottom <= geo2.innerH + 1 && geo2.hscroll, { geo2 });
await B.shot('handover-banner-landscape');

console.log('intercepted:', [...A.intercepted, ...B.intercepted]);
console.log('js errors:', [...A.errors, ...B.errors, ...V.errors].length ? [...A.errors, ...B.errors, ...V.errors] : 'none');
console.log(fails ? `${fails} FAILED` : 'all handover checks passed');
for (const p of procs) p.kill();
process.exit(fails ? 1 : 0);
