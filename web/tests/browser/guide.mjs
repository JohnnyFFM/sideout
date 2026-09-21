// the guide inside the app shell (/anleitung): top bar, tab bar, theme, content, no horizontal scroll
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const [outDir] = process.argv.slice(2);
const origin = 'http://127.0.0.1:8081';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0; const check = (name, cond, detail) => { console.log((cond ? 'ok  ' : 'FAIL') + ' ' + name + (cond ? '' : '  ' + JSON.stringify(detail))); if (!cond) fails++; };
const PORT = 9402;
const edge = spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--remote-debugging-port=${PORT}`, `--user-data-dir=${outDir}/edge-profile90-${Date.now().toString(36)}`, '--no-first-run', '--window-size=390,844', 'about:blank'], { stdio: 'ignore' });
let list; for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json(); break; } catch { await sleep(200); } }
const ws = new WebSocket(list.find((t) => t.type === 'page').webSocketDebuggerUrl); await new Promise((r) => (ws.onopen = r));
let id = 0; const pending = new Map(); const errors = [];
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } if (d.method === 'Runtime.exceptionThrown') errors.push((d.params.exceptionDetails?.exception?.description || '').split('\n')[0]); };
const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) return 'EXC:' + (r.result.exceptionDetails.exception?.description || '').split('\n')[0]; return r.result?.result?.value; };
const shot = async (name) => { const r = await send('Page.captureScreenshot', { format: 'png' }); writeFileSync(`${outDir}/${name}.png`, Buffer.from(r.result.data, 'base64')); };
await send('Runtime.enable');
await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
await send('Page.navigate', { url: `${origin}/login` }); await sleep(1200);
await ev(`fetch('/api/auth/login',{method:'POST',headers:{'X-Requested-By':'x','Content-Type':'application/json'},body:JSON.stringify({username:'jonas',password:'geheim123'})}).then(r=>r.status)`);
await send('Page.navigate', { url: `${origin}/einstellungen` }); await sleep(2000);
await ev(`[...document.querySelectorAll('a.btn')].find(a=>a.textContent.trim()==='Öffnen').click()`); await sleep(2500);
const st = JSON.parse(await ev(`JSON.stringify({ path: location.pathname, topbar: !!document.querySelector('.topbar .avatar-btn'), tabs: document.querySelectorAll('.tabbar a').length, h1: document.querySelector('.guide article h1')?.textContent || '', sections: document.querySelectorAll('.guide article h2').length, toc: !!document.querySelector('.guide .toc-inline'), hscroll: document.documentElement.scrollWidth <= innerWidth + 1, theme: document.documentElement.dataset.theme, tocLink: document.querySelector('.guide .toc-inline a')?.getAttribute('href') || '', appLink: [...document.querySelectorAll('.guide a')].map(a=>a.getAttribute('href')).find(h=>h && h.endsWith('/team')) || '' })`));
check('guide opens in the app shell on /anleitung (top bar with avatar, 4 tabs)', st.path === '/anleitung' && st.topbar && st.tabs === 4, st);
check('guide content rendered with its own styles and inline TOC', st.h1.length > 0 && st.sections >= 8 && st.toc, st);
check('no horizontal scroll at 390 px', st.hscroll, st);
check('links rewritten to the app root', st.tocLink.startsWith('#') && st.appLink === '/team', st);
await shot('guide-phone-dark');
await ev(`window.soTheme.set('light')`); await sleep(300);
const bg = await ev(`getComputedStyle(document.body).backgroundColor`);
check('light theme applies to the guide page', (await ev(`document.documentElement.dataset.theme`)) === 'light' && bg !== 'rgb(15, 19, 27)', { bg });
await shot('guide-phone-light');
// desktop: one wide column, inline contents still visible
await send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false }); await sleep(500);
const dk = JSON.parse(await ev(`JSON.stringify({ article: Math.round(document.querySelector('.guide article').getBoundingClientRect().width), main: Math.round(document.querySelector('main.guide').getBoundingClientRect().width), toc: getComputedStyle(document.querySelector('.guide .toc-inline')).display, sideToc: !!document.querySelector('.guide .toc') })`));
check('desktop 1280: article fills the column (≥ 600 px), inline contents visible, no sidebar', dk.article >= 600 && dk.article <= dk.main && dk.toc !== 'none' && !dk.sideToc, dk);
await shot('guide-desktop');
await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 2, mobile: true }); await sleep(300);
// deep link with anchor
await send('Page.navigate', { url: `${origin}/anleitung#zwei` }); await sleep(2500);
const y = await ev(`document.getElementById('zwei')?.getBoundingClientRect().top`);
check('anchor deep link scrolls to the section', typeof y === 'number' && y < 200 && y > -50, { y });
// the standalone page still works for logged-out readers
await send('Page.navigate', { url: `${origin}/hilfe/` }); await sleep(1500);
check('standalone /hilfe/ still serves the full page', (await ev(`!!document.querySelector('article') && !!document.querySelector('.topbar')`)) === true);
console.log('js errors:', errors.length ? errors : 'none');
console.log(fails ? `${fails} FAILED` : 'all guide checks passed');
edge.kill(); process.exit(fails ? 1 : 0);
