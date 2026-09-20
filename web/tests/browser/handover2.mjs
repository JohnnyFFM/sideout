// handover, part 2 (8081): same-session tabs (Web Locks and the heartbeat fallback),
// a takeover landing while an append is on the wire, and a scout event with queued work
import { spawn } from 'node:child_process';
const [outDir] = process.argv.slice(2);
const origin = 'http://127.0.0.1:8081'; const M = 1;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0; const check = (name, cond, detail) => { console.log((cond ? 'ok  ' : 'FAIL') + ' ' + name + (cond ? '' : '  ' + JSON.stringify(detail))); if (!cond) fails++; };
const procs = []; const RUN = Date.now().toString(36);

async function browser(port, profile, user, noLocks = false) {
  const edge = spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${port}`, `--user-data-dir=${outDir}/${profile}-${RUN}`, '--no-first-run', '--window-size=390,844', 'about:blank'], { stdio: 'ignore' });
  procs.push(edge);
  let list; for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); break; } catch { await sleep(200); } }
  const b = { port, tabs: [] };
  b.tab = async (targetId) => {
    const url = targetId ? (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.id === targetId).webSocketDebuggerUrl : list.find((t) => t.type === 'page').webSocketDebuggerUrl;
    const ws = new WebSocket(url); await new Promise((r) => (ws.onopen = r));
    let id = 0; const pending = new Map(); const errors = []; let mode = 'pass';
    ws.onmessage = (m) => {
      const d = JSON.parse(m.data);
      if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
      if (d.method === 'Runtime.exceptionThrown') errors.push((d.params.exceptionDetails?.exception?.description || '').split('\n')[0]);
      if (d.method === 'Fetch.requestPaused') {
        const { requestId, request, responseStatusCode } = d.params;
        const isAct = request.url.includes(`/api/matches/${M}/actions`) && request.method === 'POST';
        if (responseStatusCode === undefined) { send('Fetch.continueRequest', { requestId }); return; }
        if (isAct && mode === 'delay-post') { mode = 'pass'; setTimeout(() => send('Fetch.continueRequest', { requestId }), 3000); return; }
        send('Fetch.continueRequest', { requestId });
      }
    };
    const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
    const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) return 'EXC:' + (r.result.exceptionDetails.exception?.description || '').split('\n')[0]; return r.result?.result?.value; };
    await send('Runtime.enable'); await send('Network.enable'); await send('Page.enable');
    await send('Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Response' }] });
    if (noLocks) await send('Page.addScriptToEvaluateOnNewDocument', { source: "Object.defineProperty(navigator, 'locks', { value: undefined, configurable: true });" });
    await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
    const go = async (path, ms = 2500) => { await send('Page.navigate', { url: origin + path }); await sleep(ms); };
    const state = () => ev(`JSON.stringify({ us: +document.querySelector('.score .side.us .pts')?.textContent, queue: (JSON.parse(localStorage.getItem('so_ops_${M}')||'[]')).length, lease: !!(JSON.parse(localStorage.getItem('so_lease_${M}')||'null')?.lease), banner: document.querySelector('.scoutbar')?.textContent.replace(/\\s+/g,' ').trim() || '', padOn: document.querySelectorAll('.cell:not(:disabled)').length - 3, oppOn: !document.querySelector('.btn.big.us')?.disabled, dropped: (JSON.parse(localStorage.getItem('so_ops_dropped_${M}')||'null')?.ops||[]).length, locks: !!navigator.locks })`).then((s) => JSON.parse(s));
    const tap = () => ev(`(()=>{const b=document.querySelector('.btn.big.us'); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
    const login = async (user) => { await go('/login', 1200); await ev(`fetch('/api/auth/login',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({username:'${user}',password:'geheim123'})}).then(r=>r.status)`); };
    const t = { send, ev, go, state, tap, login, errors, setMode: (m) => { mode = m; }, close: () => send('Target.closeTarget', { targetId }) };
    b.tabs.push(t);
    return t;
  };
  b.newTab = async () => { const r = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json(); return b.tab(r.id); };
  b.first = await b.tab(null);
  if (user) await b.first.login(user);
  return b;
}
const lr = await fetch(`${origin}/api/auth/login`, { method: 'POST', headers: { 'X-Requested-By': 'x', 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'jonas', password: 'geheim123' }) });
const cookie = lr.headers.get('set-cookie').split(';')[0];
const server = async () => { const m = await (await fetch(`${origin}/api/matches/${M}`, { headers: { cookie } })).json(); return { seq: m.state.last_seq, us: m.state.us, status: m.status, held: m.scout.held, actor: m.scout.actor, rev: m.scout.revision }; };

// ---- 1. two tabs, same session, Web Locks ----
const J = await browser(9395, 'edge-profile70', 'jonas');
const t1 = J.first;
await t1.go(`/live/${M}`);
let s1 = await t1.state(); let sv = await server();
check('tab 1 holds (locks available)', s1.lease && s1.padOn > 0 && s1.locks && sv.actor === 'Jonas Steitz', { s1, sv });
const t2 = await J.newTab();
await t2.go(`/live/${M}`);
let s2 = await t2.state();
check('tab 2 of the same session: watches, "anderer Tab" notice, no scoring', s2.banner.includes('anderen Tab') && s2.padOn === 0 && !s2.oppOn, { s2 });
await t1.tap(); await sleep(1200); sv = await server();
const seq0 = sv.seq;
check('tab 1 writes; tab 2 does not flush the shared queue', sv.seq >= 1 && (await t2.state()).queue === 0, { sv });
// tab 2 leaving must not release tab 1's lease
await t2.go('/spiele', 1500);
sv = await server();
check('a watching tab leaving does not release the writer tab\'s lease', sv.held === true && sv.actor === 'Jonas Steitz', { sv });
// tab 1 leaves while tab 3 watches → tab 3 becomes the writer and can scout
const t3 = await J.newTab();
await t3.go(`/live/${M}`);
check('tab 3 watches first', (await t3.state()).banner.includes('anderen Tab'));
await t1.go('/spiele', 2000);
let took = false; for (let i = 0; i < 20 && !took; i++) { await sleep(300); const s = await t3.state(); took = !s.banner && s.lease && s.padOn > 0; }
check('tab 3 takes the writer role when tab 1 leaves and holds a lease', took, { s3: await t3.state() });
await t3.tap(); await sleep(1200); sv = await server();
check('tab 3 writes', sv.seq === seq0 + 1, { sv });
await t3.go('/spiele', 1500);

// ---- 2. the same without Web Locks (heartbeat fallback) ----
const K = await browser(9396, 'edge-profile71', 'jonas', true);
const k1 = K.first;
await k1.go(`/live/${M}`);
let sk1 = await k1.state();
check('fallback: no locks API, tab 1 still becomes the writer', !sk1.locks && sk1.lease && sk1.padOn > 0, { sk1 });
const k2 = await K.newTab();
await k2.go(`/live/${M}`, 3000);
let sk2 = await k2.state();
check('fallback: tab 2 watches', sk2.banner.includes('anderen Tab') && sk2.padOn === 0, { sk2 });
await k1.go('/spiele', 1500);
let took2 = false; for (let i = 0; i < 40 && !took2; i++) { await sleep(300); const s = await k2.state(); took2 = !s.banner && s.lease && s.padOn > 0; }
check('fallback: tab 2 becomes the writer within the heartbeat window', took2, { sk2: await k2.state() });
await k2.go('/spiele', 1500);

// ---- 3. takeover while an append is on the wire ----
const A = (await browser(9397, 'edge-profile72', 'jonas')).first;
const B = (await browser(9398, 'edge-profile73', 'petra.k')).first;
await A.go(`/live/${M}`);
check('A holds', (await A.state()).lease);
sv = await server(); const before = sv.seq;
A.setMode('delay-post');
await A.tap(); await sleep(400);                       // A's POST is held 3 s at the server's answer
await B.go(`/live/${M}`, 2000);
await B.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(1500);   // B takes over meanwhile
await sleep(3500);                                      // A's answer arrives after the loss
let sA = await A.state(); sv = await server();
check('A ends read-only after the in-flight answer, no queue resurrection', !sA.lease && sA.queue === 0 && sA.banner.includes('Petra Kuhn') && sA.padOn === 0, { sA });
check('the append committed before the takeover (server has it, revision moved on)', sv.seq === before + 1 && sv.actor === 'Petra Kuhn' && sA.us === sv.us, { sv, sA });

// ---- 4. scout event with queued work: verification before the next send ----
await A.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(1500);   // A takes back
await A.send('Network.emulateNetworkConditions', { offline: true, latency: 0, downloadThroughput: -1, uploadThroughput: -1 }); await sleep(200);
await A.tap(); await sleep(300);
check('A offline with a queued tap', (await A.state()).queue === 1);
await B.go(`/live/${M}`, 2000);
await B.ev(`document.querySelector('.scoutbar .btn').click()`); await sleep(1500);
await A.send('Network.emulateNetworkConditions', { offline: false, latency: 0, downloadThroughput: -1, uploadThroughput: -1 }); await sleep(3500);
sA = await A.state(); sv = await server();
check('after the scout event the queued tap is verified, refused and set aside, not sent', sA.queue === 0 && sA.dropped >= 1 && !sA.lease && sv.seq === before + 1, { sA, sv });

console.log('js errors:', [t1, t2, t3, k1, k2, A, B].flatMap((t) => t.errors).length ? [t1, t2, t3, k1, k2, A, B].flatMap((t) => t.errors) : 'none');
console.log(fails ? `${fails} FAILED` : 'all part-2 checks passed');
for (const p of procs) p.kill();
process.exit(fails ? 1 : 0);
