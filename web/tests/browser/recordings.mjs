// recordings in the browser against 8081 (fresh db + seedho.mjs):
// own recording on first tap, offline taps across two matches uploaded from
// another page, a second device forks with the copy/fresh choice while the
// selection stays, the coach selects, a viewer never records, undo is an
// edit, two tabs of one browser share the store with one writer, the
// legacy queue of the old client is imported, the export file round-trips.
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const [outDir] = process.argv.slice(2);
const origin = 'http://127.0.0.1:8081';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0; const check = (name, cond, detail) => { console.log((cond ? 'ok  ' : 'FAIL') + ' ' + name + (cond ? '' : '  ' + JSON.stringify(detail))); if (!cond) fails++; };
const procs = [];
const RUN = Date.now().toString(36);

async function attach(port, pick) {
  let page;
  for (let i = 0; i < 50; i++) { try { const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); page = pick(list.filter((t) => t.type === 'page')); if (page) break; } catch { /* not up yet */ } await sleep(200); }
  const ws = new WebSocket(page.webSocketDebuggerUrl); await new Promise((r) => (ws.onopen = r));
  let id = 0; const pending = new Map(); const errors = []; let intercept = null; const seen = [];
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
    if (d.method === 'Runtime.exceptionThrown') errors.push((d.params.exceptionDetails?.exception?.description || '').split('\n')[0]);
    if (d.method === 'Fetch.requestPaused') {
      const { requestId, request } = d.params;
      seen.push(request.method + ' ' + request.url.replace(origin, ''));
      const r = intercept ? intercept(request.method, request.url) : null;
      if (r?.delay) setTimeout(() => send('Fetch.continueRequest', { requestId }), r.delay);
      else if (r) send('Fetch.fulfillRequest', { requestId, responseCode: r.status, responseHeaders: [{ name: 'Content-Type', value: 'application/json' }], body: Buffer.from(JSON.stringify(r.body || { error: 'intercepted' })).toString('base64') });
      else send('Fetch.continueRequest', { requestId });
    }
  };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) return 'EXC:' + (r.result.exceptionDetails.exception?.description || '').split('\n')[0]; return r.result?.result?.value; };
  await send('Runtime.enable'); await send('Network.enable');
  await send('Fetch.enable', { patterns: [{ urlPattern: '*/api/*', requestStage: 'Request' }] });
  const setIntercept = (fn) => { intercept = fn; };
  return { page, send, ev, errors, setIntercept, seen };
}

async function browser(port, profile, user, width = 390, height = 844) {
  // a leftover Edge from a crashed run would be attached to instead of a fresh one
  try { await fetch(`http://127.0.0.1:${port}/json`); console.log(`port ${port} is already in use (stale headless Edge?), aborting`); process.exit(2); } catch { /* free */ }
  const edge = spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${port}`, `--user-data-dir=${outDir}/${profile}-${RUN}`, '--no-first-run', `--window-size=${width},${height}`, 'about:blank'], { stdio: 'ignore' });
  procs.push(edge);
  const t = await attach(port, (pages) => pages[0]);
  await t.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 2, mobile: width < 700 });
  await t.send('Page.navigate', { url: `${origin}/login` }); await sleep(1200);
  await t.ev(`fetch('/api/auth/login',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({username:'${user}',password:'geheim123'})}).then(r=>r.status)`);
  return wrap(t, port);
}
function wrap(t, port) {
  const go = async (path, ms = 2500) => { await t.send('Page.navigate', { url: origin + path }); await sleep(ms); };
  const txt = (sel) => t.ev(`document.querySelector('${sel}')?.textContent.replace(/\\s+/g,' ').trim() || ''`);
  const offline = (on) => t.send('Network.emulateNetworkConditions', { offline: on, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  const shot = async (name) => { const r = await t.send('Page.captureScreenshot', { format: 'png' }); writeFileSync(`${outDir}/${name}.png`, Buffer.from(r.result.data, 'base64')); };
  const state = () => t.ev(`JSON.stringify({ us: +document.querySelector('.score .side.us .pts')?.textContent, them: +document.querySelector('.score .side.them .pts')?.textContent, bar: [...document.querySelectorAll('.scoutbar')].map(e=>e.textContent.replace(/\\s+/g,' ').trim()).join(' | '), padOn: document.querySelectorAll('.cell:not(:disabled)').length - 3, oppOn: !document.querySelector('.btn.big.us')?.disabled, undoOn: !document.querySelector('#lastBox button')?.disabled, last: document.querySelector('#lastBox .txt')?.textContent.replace(/\\s+/g,' ').trim() || '', recs: document.querySelector('.recs summary')?.textContent.trim() || '', dialog: !!document.querySelector('.scoutbar[role=dialog]') })`).then((s) => JSON.parse(s));
  const tap = () => t.ev(`(()=>{const b=document.querySelector('.btn.big.us'); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
  const tapThem = () => t.ev(`(()=>{const b=document.querySelector('.btn.big.them'); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
  const undo = () => t.ev(`(()=>{const b=document.querySelector('#lastBox button'); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
  const local = () => t.ev(`(async()=>{const db=await new Promise((res,rej)=>{const r=indexedDB.open('sideout');r.onsuccess=()=>res(r.result);r.onerror=()=>rej(r.error)}); const all=await new Promise((res,rej)=>{const r=db.transaction('recordings').objectStore('recordings').getAll();r.onsuccess=()=>res(r.result);r.onerror=()=>rej(r.error)}); db.close(); return JSON.stringify(all.map(r=>({id:r.id,match:r.match_id,next:r.next_n,confirmed:r.confirmed_n,created:r.created,error:r.error,imported:r.imported,device:r.device_id})))})()`).then((s) => JSON.parse(s));
  const click = (sel) => t.ev(`(()=>{const b=document.querySelector('${sel}'); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
  const clickText = (sel, text) => t.ev(`(()=>{const b=[...document.querySelectorAll('${sel}')].find(x=>x.textContent.trim().startsWith('${text}')); if(!b||b.disabled) return 'blocked'; b.click(); return 'clicked'})()`);
  const login = async (user) => { await t.ev(`fetch('/api/auth/logout',{method:'POST',headers:{'X-Requested-By':'x'}}).catch(()=>0)`); await t.ev(`fetch('/api/auth/login',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({username:'${user}',password:'geheim123'})}).then(r=>r.status)`); };
  return { ...t, port, go, txt, offline, shot, state, tap, tapThem, undo, local, click, clickText, login };
}
async function secondTab(b) {
  const r = await b.send('Target.createTarget', { url: 'about:blank' });
  const tid = r.result.targetId;
  const t = await attach(b.port, (pages) => pages.find((p) => p.id === tid));
  return wrap(t, b.port);
}

// server truth via node (own session)
const lr = await fetch(`${origin}/api/auth/login`, { method: 'POST', headers: { 'X-Requested-By': 'x', 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'jonas', password: 'geheim123' }) });
const cookie = lr.headers.get('set-cookie').split(';')[0];
const J = async (path, opts = {}) => { const r = await fetch(`${origin}/api${path}`, { method: opts.method || 'GET', headers: { 'X-Requested-By': 'x', 'Content-Type': 'application/json', cookie }, body: opts.body ? JSON.stringify(opts.body) : undefined }); return { status: r.status, data: await r.json().catch(() => null) }; };
const server = async (mid = 1) => { const m = (await J(`/matches/${mid}`)).data; return { us: m.state.us, them: m.state.them, last_seq: m.state.last_seq, status: m.status, selected: m.selected, recs: m.recordings.map((r) => ({ id: r.id, n: r.n, device: r.device, user: r.user, us: r.state?.us, active: r.active, origin_id: r.origin_id })) }; };
const waitServer = async (mid, pred, ms = 8000) => { const t0 = Date.now(); let s; do { s = await server(mid); if (pred(s)) return s; await sleep(300); } while (Date.now() - t0 < ms); return s; };

const A = await browser(9490, 'edge-profile70', 'jonas');          // device A: coach
const B = await browser(9491, 'edge-profile71', 'petra.k');        // device B: co-coach
const V = await browser(9492, 'edge-profile72', 'maxv', 1100, 800); // viewer, desktop

// 1. the first tap creates this device's recording; each tap goes to the server right away
const pl = (await J('/players')).data.players.map((p) => p.id);
const m2 = (await J('/matches', { method: 'POST', body: { opponent: 'Zweiter Gegner', date: '2026-09-22', lineup: { pos: pl.slice(0, 6), libero: pl[6] } } })).data;
check('second match created', m2.id === 2, { m2 });
await A.go('/spiele', 2500); // caches the list and both matches for the offline part below
await A.go('/live/2', 2000);
await A.go('/live/1');
let sA = await A.state();
check('A: pad enabled, no dialog, no recording yet', sA.padOn > 0 && sA.oppOn && !sA.dialog && (await A.local()).length === 0, { sA });
await A.tap(); await sleep(300); await A.tap(); await A.tap(); await sleep(1500);
let sv = await waitServer(1, (s) => s.recs.length === 1 && s.recs[0].n === 3);
sA = await A.state();
check('A: three taps → one recording with 3 edits on the server, selected, status live', sv.recs.length === 1 && sv.recs[0].n === 3 && sv.us === 3 && sv.status === 'live' && sv.selected === sv.recs[0].id && sA.us === 3, { sv, sA });
check('A: sync text says saved on server, no "other device" banner for its own recording', /auf Server gespeichert/.test(sA.last) && !/scoutet gerade/.test(sA.bar), { sA });
const recA = sv.recs[0].id;
await A.shot('rec-a-live');

// 2. offline taps on two matches, pages left, upload from the list page once online
await A.offline(true); await sleep(300);
await A.tap(); await A.tap(); await A.tap(); await A.tap(); await A.tap(); await sleep(800);
sA = await A.state();
check('A offline: five more taps count locally, marked as not uploaded', sA.us === 8 && /5 nicht hochgeladen/.test(sA.last), { sA });
await A.shot('rec-a-offline');
await A.go('/live/2', 2500); // from the cache
sA = await A.state();
check('A offline on match 2: opens from the cache, pad enabled', sA.padOn > 0 && sA.oppOn, { sA });
await A.tap(); await A.tapThem(); await sleep(800);
sA = await A.state();
check('A offline on match 2: two taps recorded locally', sA.us === 1 && sA.them === 1 && /2 nicht hochgeladen/.test(sA.last), { sA });
await A.go('/spiele', 2500);
const list = await A.txt('.matches');
check('A offline list shows pending counts per match', /5 nicht hochgeladen/.test(list) && /2 nicht hochgeladen/.test(list), { list, bar: await A.txt('.offline-bar') });
await A.shot('rec-a-list-offline');
await A.offline(false);
sv = await waitServer(1, (s) => s.recs[0]?.n === 8);
const sv2 = await waitServer(2, (s) => s.recs.length === 1 && s.recs[0].n === 2);
check('A online on the list page: both matches uploaded without the live page', sv.recs[0].n === 8 && sv.us === 8 && sv2.recs[0].n === 2 && sv2.us === 1 && sv2.them === 1, { sv, sv2 });
await sleep(1500);
const list2 = await A.txt('.matches');
check('A list: pending badges gone', !/nicht hochgeladen/.test(list2), { list2 });
let loc = await A.local();
check('A local store: both recordings confirmed', loc.every((r) => r.confirmed === r.next - 1 && r.created), { loc });

// 3. device B: sees who scouts, may record on its own; the choice on first tap; the selection stays
await B.go('/live/1');
let sB = await B.state();
check('B: banner names the active scout, pad enabled, no dialog yet', /Jonas Steitz scoutet gerade/.test(sB.bar) && sB.padOn > 0 && !sB.dialog && sB.us === 8, { sB });
await B.tap(); await sleep(400);
sB = await B.state();
check('B: first tap asks copy or fresh, nothing recorded yet', sB.dialog && sB.us === 8 && (await B.local()).length === 0, { sB });
await B.shot('rec-b-dialog');
check('B: fresh start', (await B.clickText('.scoutbar[role=dialog] button', 'Neu beginnen')) === 'clicked');
await sleep(600);
await B.tap(); await sleep(1500);
sB = await B.state();
sv = await waitServer(1, (s) => s.recs.length === 2 && s.recs[1].n === 2);
check('B: own recording from the planning (0 → 2), uploaded as second recording; selection unchanged', sB.us === 2 && sv.recs.length === 2 && sv.recs[1].n === 2 && sv.selected === recA && sv.us === 8, { sB, sv });
const recB = sv.recs[1].id;
check('B: told that another recording is the result', /Als Ergebnis zählt/.test(sB.bar) && /Aufzeichnungen \(2\)/.test(sB.recs), { sB });
await B.shot('rec-b-own');
// A sees the second recording too, still shows its own
await A.go('/live/1', 2500);
sA = await A.state();
check('A: still its own recording (8), lists 2 recordings, no dialog', sA.us === 8 && /Aufzeichnungen \(2\)/.test(sA.recs) && !sA.dialog, { sA });

// 4. the coach selects B's recording as the result (from the evaluation page)
await A.go('/auswertung/1', 2500);
check('A auswertung: recordings panel with 2 entries', /Aufzeichnungen \(2\)/.test(await A.txt('.recs summary')));
await A.ev(`document.querySelector('.recs').open = true`);
check('A selects B', (await A.clickText('.recs li:not(.sel) button', 'Als Ergebnis')) === 'clicked');
sv = await waitServer(1, (s) => s.selected === recB);
check('server: B is the result now, state follows B', sv.selected === recB && sv.us === 2, { sv });
await sleep(800);
await A.shot('rec-a-auswertung');
const aw = await A.txt('.srcbar');
check('A auswertung: own recording differs from the result, switch offered', /Als Ergebnis zählt/.test(aw) && /Dieses Gerät/.test(aw), { aw });
// B: the bar says its recording is the result now
await B.go('/live/1', 2500);
sB = await B.state();
check('B: no "other result" bar any more', !/Als Ergebnis zählt/.test(sB.bar), { sB });

// 5. a viewer never records
await V.go('/live/1');
const sV = await V.state();
check('viewer: pad disabled, shows the result (B, 2:0), no dialog', sV.padOn === 0 && !sV.oppOn && sV.us === 2 && !sV.dialog && (await V.local()).length === 0, { sV });
await V.tap();
check('viewer: tap blocked', (await V.tap()) === 'blocked');

// 6. undo is an edit; the result (B) does not move
await A.go('/live/1', 2500);
check('A undo', (await A.undo()) === 'clicked');
await sleep(1500);
sv = await waitServer(1, (s) => s.recs.find((r) => r.id === recA)?.n === 9);
sA = await A.state();
check('A: undo stored as edit 9, own state 7, result still B 2:0', sA.us === 7 && sv.recs.find((r) => r.id === recA).n === 9 && sv.us === 2 && sv.selected === recB, { sA, sv });

// 7. two tabs of one browser: one writer, shared store
const A2 = await secondTab(A);
await A2.go('/live/1', 2500);
const s2 = await A2.state();
check('A second tab: watches, told the other tab scouts, same local state', s2.padOn === 0 && /anderen Tab/.test(s2.bar) && s2.us === 7, { s2 });
await A.tap(); await sleep(800);
const s2b = await A2.state();
check('A second tab follows the writer tab (8)', s2b.us === 8, { s2b });
await A2.send('Page.navigate', { url: 'about:blank' }); await sleep(500);
sv = await waitServer(1, (s) => s.recs.find((r) => r.id === recA)?.n === 10);
check('server: edit 10 arrived', sv.recs.find((r) => r.id === recA).n === 10, { sv });

// 8. the old client's leftovers are imported once: a dropped queue on top of the cached match
const cached = (await J('/matches/1')).data;
const legacyOps = [{ type: 'add', action: { seq: cached.actions.length + 1, skill: 'opp', grade: '=', cid: 'x1' } }, { type: 'add', action: { seq: cached.actions.length + 2, skill: 'opp', grade: '#', cid: 'x2' } }];
await B.ev(`(()=>{localStorage.setItem('so_match_1', ${JSON.stringify(JSON.stringify(cached))}); localStorage.setItem('so_ops_dropped_1', JSON.stringify({ at: 'x', ops: ${JSON.stringify(legacyOps)} })); return 1})()`);
await B.go('/spiele', 3500);
sv = await waitServer(1, (s) => s.recs.length === 3);
const imp = sv.recs.find((r) => r.device === 'Import (beiseitegelegt)');
check('legacy dropped queue imported as its own recording and uploaded (2 edits on the cached base)', !!imp && imp.n === 2 && imp.us === cached.state.us + 1, { sv });
check('B legacy key untouched', (await B.ev(`!!localStorage.getItem('so_ops_dropped_1')`)) === true);
await B.go('/spiele', 2500);
sv = await server(1);
check('legacy import runs once', sv.recs.length === 3, { sv });
loc = await B.local();
check('B local: imported recording marked imported, own recording untouched', loc.some((r) => r.imported) && loc.some((r) => !r.imported && r.id === recB), { loc });

// 9. export → import on another device round-trips without duplicates
await A.go('/einstellungen', 2500);
await A.ev(`[...document.querySelectorAll('details')].forEach(d=>d.open=true)`);
const recsTxt = await A.txt('.reclist');
check('A einstellungen lists both recordings with their state', /auf Server gespeichert/.test(recsTxt) && (recsTxt.match(/Exportieren/g) || []).length >= 2, { recsTxt });
await A.shot('rec-a-settings');
// the export via the store API (a download cannot be caught headless): import the same content on V
const exported = await A.ev(`(async()=>{const db=await new Promise((res,rej)=>{const r=indexedDB.open('sideout');r.onsuccess=()=>res(r.result);r.onerror=()=>rej(r.error)}); const rec=await new Promise((res)=>{const r=db.transaction('recordings').objectStore('recordings').get('${recA}');r.onsuccess=()=>res(r.result)}); const edits=await new Promise((res)=>{const r=db.transaction('edits').objectStore('edits').index('rec').getAll('${recA}');r.onsuccess=()=>res(r.result)}); db.close(); return JSON.stringify({format:'sideout-recording',schema:1,recording:{id:rec.id,match_id:rec.match_id,device_id:rec.device_id,device_label:rec.device_label,base:rec.base,origin_id:rec.origin_id,origin_n:rec.origin_n},edits:edits.map(e=>({n:e.n,body:e.body}))})})()`);
check('export content has 10 edits', JSON.parse(exported).edits.length === 10);
await V.go('/einstellungen', 2500);
// the viewer's import goes through the page's file input handler
await V.ev(`(async()=>{const f=new File([${JSON.stringify(exported)}],'x.json',{type:'application/json'}); const dt=new DataTransfer(); dt.items.add(f); const inp=document.querySelector('input[type=file]'); inp.files=dt.files; inp.dispatchEvent(new Event('change',{bubbles:true})); return 1})()`);
await sleep(2500);
const vloc = await V.local();
check('viewer imported the file: recording present, foreign device, uploads (identical content is accepted, no duplicate)', vloc.some((r) => r.id === recA && r.imported), { vloc });
await sleep(2000);
sv = await server(1);
check('server unchanged by the identical import (still 3 recordings, A at 10)', sv.recs.length === 3 && sv.recs.find((r) => r.id === recA).n === 10, { sv });
const vloc2 = await V.local();
check('viewer: import confirmed after upload (a viewer may not write → visible error instead)', vloc2.find((r) => r.id === recA).error !== null || vloc2.find((r) => r.id === recA).confirmed === 10, { vloc2 });

// 10. A → B → A: A continues the result (B's recording) as a new copy of its own; the old one stays
await A.go('/live/1', 2500);
sA = await A.state();
check('A: bar offers "Ergebnis als Kopie fortsetzen"', /Ergebnis als Kopie fortsetzen/.test(sA.bar) && sA.us === 8, { sA });
check('A continues the result', (await A.clickText('.scoutbar button', 'Ergebnis als Kopie')) === 'clicked');
await sleep(800);
sA = await A.state();
check('A: now on a copy of B (2:0), no dialog', sA.us === 2 && !sA.dialog, { sA });
await A.tap(); await sleep(1500);
sv = await waitServer(1, (s) => s.recs.length === 4);
const copyA = (await J('/matches/1')).data.recordings.find((r) => r.origin_id === recB);
check('server: the copy arrived with origin = B, one edit, 3:0; A\'s older recording untouched (10)', !!copyA && copyA.n === 1 && copyA.state.us === 3 && sv.recs.find((r) => r.id === recA).n === 10, { sv, copyA });
loc = await A.local();
check('A local: three own recordings of match 1 exist side by side', loc.filter((r) => r.match === 1 && !r.imported).length === 2 && loc.filter((r) => r.match === 1).length >= 2, { loc });
await A.shot('rec-a-copy');

// 11. rapid first taps make exactly one recording (a fresh match, no dialog)
const m3 = (await J('/matches', { method: 'POST', body: { opponent: 'Dritter Gegner', date: '2026-09-23', lineup: { pos: pl.slice(0, 6), libero: pl[6] } } })).data;
check('third match created', m3.id === 3, { m3 });
await A.go('/live/3', 2500);
await A.ev(`(()=>{const b=document.querySelector('.btn.big.us'); b.click(); b.click(); b.click(); return 1})()`);
await sleep(1500);
loc = await A.local();
sv = await waitServer(3, (s) => s.recs.length === 1 && s.recs[0].n === 3);
check('A: three rapid first taps → one recording with 3 edits, 3:0', loc.filter((r) => r.match === 3).length === 1 && loc.find((r) => r.match === 3).next === 4 && sv.recs.length === 1 && sv.recs[0].n === 3 && sv.us === 3, { loc, sv });
const recA3 = sv.recs[0].id;

// 12. taps during the copy/fresh question are kept (B on match 3, where A already records)
await B.go('/live/3', 2500);
await B.ev(`(()=>{const b=document.querySelector('.btn.big.us'); b.click(); b.click(); return 1})()`);
await sleep(500);
sB = await B.state();
check('B: question open after two quick taps, nothing recorded yet', sB.dialog && (await B.local()).filter((r) => r.match === 3).length === 0, { sB });
check('B: fresh', (await B.clickText('.scoutbar[role=dialog] button', 'Neu beginnen')) === 'clicked');
await sleep(1200);
sB = await B.state();
sv = await waitServer(3, (s) => s.recs.length === 2);
check('B: both taps landed in the new recording (2:0), uploaded', sB.us === 2 && sv.recs.length === 2 && sv.recs.find((r) => r.id !== recA3)?.n === 2, { sB, sv });

// 13. a failing recording (5xx) does not block the others; it retries later on its own
A.setIntercept((method, url) => (method === 'PUT' && url.includes(`/api/matches/1/recordings/`) ? { status: 503, body: { error: 'down' } } : null));
await A.go('/live/1', 2500);
await A.tap(); await sleep(400);
await A.go('/live/3', 2500);
await A.tap(); await sleep(1500);
sv = await waitServer(3, (s) => s.recs.find((r) => r.id === recA3)?.n === 4);
loc = await A.local();
check('A: match 3 uploads (4) while match 1 is stuck on 503 and stays pending', sv.recs.find((r) => r.id === recA3).n === 4 && loc.find((r) => r.match === 1 && r.id !== recA && !r.imported && r.next - 1 > r.confirmed) != null, { sv, loc });
A.setIntercept(null);
sv = await waitServer(1, (s) => s.recs.find((r) => r.origin_id === recB)?.n === 2, 15000);
check('A: after the server recovers the paused recording is retried without any tap', sv.recs.find((r) => r.origin_id === recB)?.n === 2, { sv });

// 14. a permanent error (403) is shown once and does not loop
A.setIntercept((method, url) => (method === 'PUT' && url.includes(`/api/matches/3/recordings/`) ? { status: 403, body: { error: 'forbidden' } } : null));
const before = A.seen.filter((x) => x.startsWith('PUT /api/matches/3/')).length;
await A.tap(); await sleep(5000);
const after = A.seen.filter((x) => x.startsWith('PUT /api/matches/3/')).length;
loc = await A.local();
check('A: one refused attempt, then quiet (no retry storm), error recorded', after - before <= 2 && /forbidden/.test(loc.find((r) => r.match === 3)?.error || ''), { before, after, loc });
// a page load is a fresh uploader: one more attempt, still refused, the error stays visible
await A.go('/einstellungen', 2500);
await A.ev(`[...document.querySelectorAll('details')].forEach(d=>d.open=true)`);
const rl = await A.txt('.reclist');
check('A einstellungen shows the error', /Fehler: forbidden/.test(rl), { rl });
A.setIntercept(null);
const lift = await A.clickText('button', 'Jetzt hochladen');
check('A: "Jetzt hochladen" lifts the pause', lift === 'clicked', { lift });
sv = await waitServer(3, (s) => s.recs.find((r) => r.id === recA3)?.n === 5);
check('A: the recording uploads after the explicit retry', sv.recs.find((r) => r.id === recA3)?.n === 5, { sv });

// 15. importing a later backup of a known recording appends the missing edits
const exp1 = JSON.parse(exported); // 10 edits of A's first recording
const cut = { ...exp1, edits: exp1.edits.slice(0, 6) };
await B.go('/einstellungen', 2500);
const importOn = async (b, data) => b.ev(`(async()=>{const f=new File([${JSON.stringify(JSON.stringify(data))}],'x.json',{type:'application/json'}); const dt=new DataTransfer(); dt.items.add(f); const inp=document.querySelector('input[type=file]'); inp.files=dt.files; inp.dispatchEvent(new Event('change',{bubbles:true})); return 1})()`);
await importOn(B, cut); await sleep(1500);
let bl = (await B.local()).find((r) => r.id === recA);
check('B: early backup imported (6 edits)', bl && bl.next === 7, { bl });
await importOn(B, exp1); await sleep(1500);
bl = (await B.local()).find((r) => r.id === recA);
check('B: later backup appends the missing 4 edits (10)', bl && bl.next === 11, { bl });
const bad = { ...exp1, edits: exp1.edits.map((e, i) => (i === 2 ? { ...e, body: { op: 'add', action: { ...e.body.action, grade: '#' } } } : e)) };
await importOn(B, bad); await sleep(1200);
const msg = await B.txt('.importmsg');
bl = (await B.local()).find((r) => r.id === recA);
check('B: a backup with different content under the same id is refused, nothing changed', /anderer Änderung Nr. 3/.test(msg) && bl.next === 11, { msg, bl });

// 16. another account on the same browser neither continues nor uploads petra's recording
B.setIntercept((method, url) => (method === 'PUT' && url.includes(`/api/matches/1/recordings/${recB}`) ? { status: 503, body: { error: 'down' } } : null));
await B.go('/live/1', 2500);
await B.tap(); await sleep(1200);
bl = (await B.local()).find((r) => r.id === recB);
check('B (petra): one edit pending on her recording', bl && bl.next - 1 - bl.confirmed === 1, { bl });
await B.login('jonas');
await B.go('/live/1', 2500);
B.setIntercept(null);
await sleep(3000);
sB = await B.state();
bl = (await B.local()).find((r) => r.id === recB);
sv = await server(1);
check('B (jonas): petra\'s recording is not his, not shown as own, and not uploaded under his account', sB.us === 2 && !sB.dialog && bl.next - 1 - bl.confirmed === 1 && sv.recs.find((r) => r.id === recB).n === 2, { sB, bl, sv });
await B.go('/einstellungen', 2500);
check('B (jonas): settings mention the other account\'s pending recording', /anderen Kontos/.test(await A.txt('body') + await B.txt('.app-row .d')) || /anderen Kontos/.test(await B.ev(`document.body.textContent`)));
await B.login('petra.k');
await B.go('/spiele', 2500);
sv = await waitServer(1, (s) => s.recs.find((r) => r.id === recB)?.n === 3);
check('B (petra again): her pending edit uploads', sv.recs.find((r) => r.id === recB).n === 3, { sv });

// 17. a tap belongs to the match it was made on, even when the page moves on before it is saved
await A.go('/live/3', 2500);
sv = await server(3); const n3 = sv.recs.find((r) => r.id === recA3).n; const n2 = (await server(2)).recs[0].n;
// tap, then client-side navigation to match 2 in the same tick (an in-page link)
await A.ev(`(()=>{const b=document.querySelector('.btn.big.us'); b.click(); const a=document.createElement('a'); a.href='/live/2'; document.body.append(a); a.click(); return 1})()`);
await sleep(2500);
sA = await A.state();
sv = await waitServer(3, (s) => s.recs.find((r) => r.id === recA3)?.n === n3 + 1);
const sv2b = await server(2);
check('A: the tap landed in match 3 (n+1), match 2 untouched, page shows match 2', sv.recs.find((r) => r.id === recA3).n === n3 + 1 && sv2b.recs[0].n === n2 && sA.us === 1 && sA.them === 1, { sA, n3, sv, sv2b });
// tap, then leave the live page altogether (tab bar): the tap is still saved
await A.ev(`(()=>{const b=document.querySelector('.btn.big.us'); b.click(); document.querySelector('.tabbar a[href="/spiele"]').click(); return 1})()`);
await sleep(2500);
sv = await waitServer(2, (s) => s.recs[0].n === n2 + 1);
check('A: a tap right before leaving the page is saved and uploaded', sv.recs[0].n === n2 + 1 && sv.us === 2, { sv });

// 18. the account changes in another tab while an upload pass is running: nothing goes out under the new account
await A.go('/live/2', 2500);
await A.offline(true); await sleep(300);
await A.tap(); await sleep(500);
await A.go('/live/3', 2500);
await A.tap(); await sleep(500);
loc = await A.local();
check('A (jonas) offline: match 2 and match 3 have a pending edit', loc.find((r) => r.match === 2).next - 1 - loc.find((r) => r.match === 2).confirmed === 1 && loc.find((r) => r.id === recA3).next - 1 - loc.find((r) => r.id === recA3).confirmed === 1, { loc });
const n3b = (await server(3)).recs.find((r) => r.id === recA3).n;
// the first PUT of the pass (match 2, the older recording) is held for three seconds; meanwhile another tab signs in as petra
const A3 = await secondTab(A);
await A3.go('/login', 1500); // an app page (same origin), no identity, no uploader of its own
A.setIntercept((method, url) => (method === 'PUT' && url.includes('/api/matches/2/recordings/') ? { delay: 3000 } : null));
await A.offline(false);
await sleep(700);
await A3.login('petra.k');
await sleep(5000);
A.setIntercept(null);
sv = await server(3);
loc = await A.local();
check('A: match 3 was not uploaded under petra (server unchanged, still pending locally)', sv.recs.find((r) => r.id === recA3).n === n3b && loc.find((r) => r.id === recA3).next - 1 - loc.find((r) => r.id === recA3).confirmed === 1, { sv, loc, seen: A.seen.filter((x) => x.startsWith('PUT')).slice(-4) });
check('A: the page noticed the account change', /Petra Kuhn|petra/i.test(await A.ev(`document.body.textContent`)) || (await A.ev(`JSON.parse(localStorage.getItem('so_me')||'null')?.user?.username`)) === 'petra.k');
await A3.send('Page.navigate', { url: 'about:blank' });
await A.login('jonas');
await A.go('/spiele', 2500);
sv = await waitServer(3, (s) => s.recs.find((r) => r.id === recA3)?.n === n3b + 1);
check('A (jonas again): the pending edit uploads under jonas', sv.recs.find((r) => r.id === recA3).n === n3b + 1, { sv });

// 19. an import never confirms more than it sent: a shorter backup, then a longer one, is compared with the server
const eA = JSON.parse(await A.ev(`(async()=>{const db=await new Promise((res,rej)=>{const r=indexedDB.open('sideout');r.onsuccess=()=>res(r.result);r.onerror=()=>rej(r.error)}); const rec=await new Promise((res)=>{const r=db.transaction('recordings').objectStore('recordings').get('${recA3}');r.onsuccess=()=>res(r.result)}); const edits=await new Promise((res)=>{const r=db.transaction('edits').objectStore('edits').index('rec').getAll('${recA3}');r.onsuccess=()=>res(r.result)}); db.close(); edits.sort((a,b)=>a.n-b.n); return JSON.stringify({format:'sideout-recording',schema:1,recording:{id:rec.id,match_id:rec.match_id,device_id:rec.device_id,device_label:rec.device_label,base:rec.base,origin_id:rec.origin_id,origin_n:rec.origin_n},edits:edits.map(e=>({n:e.n,body:e.body}))})})()`));
const total3 = eA.edits.length;
await B.go('/einstellungen', 2500);
await importOn(B, { ...eA, edits: eA.edits.slice(0, 3) }); await sleep(2500);
bl = (await B.local()).find((r) => r.id === recA3);
check('B: short backup imported and confirmed only up to what was sent (3), although the server holds more', bl && bl.next === 4 && bl.confirmed === 3, { bl, total3 });
const wrong = { ...eA, edits: eA.edits.map((e, i) => (i === 4 ? { n: e.n, body: { op: 'add', action: { seq: e.body.action?.seq ?? 99, skill: 'opp', grade: e.body.action?.grade === '=' ? '#' : '=' } } } : e)) };
await importOn(B, wrong); await sleep(3000);
bl = (await B.local()).find((r) => r.id === recA3);
sv = await server(3);
check('B: a longer backup with a different edit 5 is appended locally, sent, refused by the server and shown as error; the server keeps its own', bl && bl.next === total3 + 1 && bl.confirmed <= 4 && /edit_mismatch/.test(bl.error || '') && sv.recs.find((r) => r.id === recA3).n === n3b + 1, { bl, sv });

for (const [n, b] of [['A', A], ['B', B], ['V', V]]) check(`${n}: no page errors`, b.errors.length === 0, b.errors);
console.log(fails ? `\n${fails} FAILED` : '\nall ok');
for (const p of procs) p.kill();
process.exit(fails ? 1 : 0);
