// handover fixture on 8081: coach jonas, co-coach petra (assistant), viewer max, roster, one match with lineup
const origin = 'http://127.0.0.1:8081';
const J = async (path, opts = {}, cookie) => { const r = await fetch(`${origin}/api${path}`, { method: opts.method || 'GET', headers: { 'X-Requested-By': 'x', 'Content-Type': 'application/json', ...(cookie ? { cookie } : {}) }, body: opts.body ? JSON.stringify(opts.body) : undefined }); const d = await r.json().catch(() => null); return { status: r.status, data: d, cookie: (r.headers.get('set-cookie') || '').split(';')[0] }; };
const reg = await J('/auth/register-team', { method: 'POST', body: { team_name: 'TSV Eintracht', display_name: 'Jonas Steitz', username: 'jonas', password: 'geheim123' } });
const C = reg.cookie; const code = reg.data.team.join_code; const teamId = reg.data.team.id;
const ids = [];
for (const [n, name, p] of [[1, 'Lena Vogt', 'Z'], [3, 'Mia Brandt', 'A'], [4, 'Sara Kunz', 'M'], [5, 'Jule Hartmann', 'D'], [7, 'Nele Fischer', 'A'], [8, 'Emma Roth', 'M'], [12, 'Jo Lindner', 'L'], [9, 'Kim Sauer', 'A']]) { const r = await J('/players', { method: 'POST', body: { number: n, name, position: p } }, C); ids.push(r.data.id); }
const m = await J('/matches', { method: 'POST', body: { opponent: 'VfL Bad Vilbel', date: '2026-09-21', lineup: { pos: ids.slice(0, 6), libero: ids[6] } } }, C);
console.log('match', m.data.id, 'scout', JSON.stringify(m.data.scout));
const petra = await J('/auth/join', { method: 'POST', body: { code, display_name: 'Petra Kuhn', username: 'petra.k', password: 'geheim123' } });
const petraId = petra.data.user.id;
console.log('promote petra', (await J(`/teams/${teamId}/members/${petraId}`, { method: 'PATCH', body: { role: 'assistant' } }, C)).status);
const max = await J('/auth/join', { method: 'POST', body: { code, display_name: 'Max Vogt', username: 'maxv', password: 'geheim123' } });
console.log('max role', max.data.user.role);
