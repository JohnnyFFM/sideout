<script>
  import { base } from '$app/paths';
  // Box score, score flow, side-out by rotation, point sources, quality
  // stacks. Computed client-side with the engine from the action log, so
  // it also works from the offline cache.
  import { page } from '$app/stores';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations } from '$lib/stores.js';
  import { replay, stats, GRADES, GRADE_CLASS, GRADE_NAME, PAD, ROMAN, firstName, eff, fix, pct, fmtDate } from '$lib/engine.js';
  import { cachedMatch, cacheMatch, applyOps, loadOps } from '$lib/offline.js';

  const id = $derived(Number($page.params.id));
  let match = $state(null);
  let error = $state('');
  let set = $state(0);

  async function load() {
    try { const m = await api(`/matches/${id}`); match = m; cacheMatch(id, m); }
    catch (e) { const c = cachedMatch(id); if (c) match = c; else error = e.offline ? 'Keine Verbindung.' : e.message; }
  }
  $effect(() => { void id; untrack(load); });
  $effect(() => { if ($mutations && (($mutations.entity === 'action' || $mutations.entity === 'match') && $mutations.id === id || $mutations.entity === 'resync')) untrack(load); });

  const cfg = $derived(match ? { first_serve_us: match.first_serve === 'us', lineups: match.lineups } : null);
  const actions = $derived(match ? applyOps(match.actions, loadOps(id)) : []);
  const full = $derived(cfg ? replay(cfg, actions) : null);
  const s = $derived(cfg ? stats(cfg, match.players, actions, set) : null);
  const byId = $derived(Object.fromEntries((match?.players || []).map((p) => [p.id, p])));
  const nSets = $derived(full ? full.sets.length + (full.finished ? 0 : 1) : 0);

  const tot = $derived.by(() => {
    if (!s) return null;
    const t = { pts: 0, A: { n: 0, k: 0, e: 0, blk: 0 }, S: { n: 0, ace: 0, err: 0 }, R: { n: 0, sum: 0, pos: 0, err: 0 }, B: { pts: 0, touch: 0, err: 0 }, D: { good: 0, n: 0, err: 0 }, E: { n: 0, ast: 0, err: 0 } };
    for (const p of s.players) {
      t.pts += p.pts;
      for (const k of ['n', 'k', 'e', 'blk']) t.A[k] += p.A[k];
      for (const k of ['n', 'ace', 'err']) t.S[k] += p.S[k];
      for (const k of ['n', 'sum', 'pos', 'err']) t.R[k] += p.R[k];
      for (const k of ['pts', 'touch', 'err']) t.B[k] += p.B[k];
      for (const k of ['good', 'n', 'err']) t.D[k] += p.D[k];
      for (const k of ['n', 'ast', 'err']) t.E[k] += p.E[k];
    }
    return t;
  });
  const errs = $derived(s ? Object.values(s.team.errBy).reduce((a, b) => a + b, 0) + s.players.reduce((a, p) => a + p.A.blk, 0) : 0);
  const cls = (v, good, bad) => (v == null ? '' : v >= good ? 'good' : v <= bad ? 'bad' : '');

  // score flow for the selected (or current) set
  const flow = $derived.by(() => {
    if (!full) return null;
    const fset = set || full.set;
    const rl = full.rally_log.filter((r) => r.set === fset);
    const pts = [[0, 0]]; let d = 0;
    rl.forEach((r, i) => { d += r.won ? 1 : -1; pts.push([i + 1, d]); });
    const maxX = Math.max(pts.length - 1, 10), maxD = Math.max(4, ...pts.map((p) => Math.abs(p[1])));
    const X = (x) => 20 + (x / maxX) * 570, Y = (y) => 75 - (y / maxD) * 60;
    const lastX = X(pts[pts.length - 1][0]);
    return {
      fset, n: rl.length, maxD, y0: Y(0), yTop: Y(maxD), yBot: Y(-maxD),
      path: pts.map((p, i) => (i ? 'L' : 'M') + X(p[0]).toFixed(1) + ' ' + Y(p[1]).toFixed(1)).join(' '),
      areaUs: 'M' + X(0) + ' ' + Y(0) + ' ' + pts.map((p) => 'L' + X(p[0]).toFixed(1) + ' ' + Y(Math.max(0, p[1])).toFixed(1)).join(' ') + ' L' + lastX + ' ' + Y(0) + ' Z',
      areaThem: 'M' + X(0) + ' ' + Y(0) + ' ' + pts.map((p) => 'L' + X(p[0]).toFixed(1) + ' ' + Y(Math.min(0, p[1])).toFixed(1)).join(' ') + ' L' + lastX + ' ' + Y(0) + ' Z'
    };
  });

  const rotations = $derived.by(() => {
    if (!s || !match) return [];
    const lu = (match.lineups?.[set || full.set] || match.lineups?.[1] || Object.values(match.lineups)[0])?.pos || [];
    const setter = lu.find((id) => byId[id]?.position === 'Z');
    return lu.map((pid, i) => {
      // rotation i = the set's starting lineup rotated i times: the player who
      // started on I+i is on I, whoever substitutes her later included
      const r = s.team.byRot[i] || { so: { won: 0, n: 0 }, brk: { won: 0, n: 0 } };
      const setterPos = setter ? ((lu.indexOf(setter) - i + 6) % 6) + 1 : null;
      return { pid, r, sop: r.so.n ? Math.round((r.so.won / r.so.n) * 100) : null, setterPos };
    });
  });

  const cats = $derived(s ? {
    pts: [{ label: 'Angriff', n: s.team.ptsBy.A, cls: 's1' }, { label: 'Aufschlag', n: s.team.ptsBy.S, cls: 's2' }, { label: 'Block', n: s.team.ptsBy.B, cls: 's3' }, { label: 'Gegnerfehler', n: s.team.ptsBy.opp, cls: 's4' }],
    lost: [{ label: 'Gegner-Punkt', n: s.team.lostBy.oppKill, cls: 's1' }, { label: 'Angriffsfehler', n: s.team.errBy.A + s.players.reduce((a, p) => a + p.A.blk, 0), cls: 's2' }, { label: 'Aufschlagfehler', n: s.team.errBy.S, cls: 's3' }, { label: 'Andere Fehler', n: s.team.errBy.R + s.team.errBy.E + s.team.errBy.B + s.team.errBy.D, cls: 's4' }]
  } : null);
  const sum = (items) => items.reduce((a, b) => a + b.n, 0);
  const maxPts = $derived(s ? Math.max(1, ...s.players.map((p) => p.pts)) : 1);
  // team row from the first attempt, players from `min` attempts (one attempt is not a distribution)
  const gsum = (g) => Object.values(g).reduce((a, b) => a + b, 0);
  const gradeRows = (key, min) => {
    const rows = s.players.filter((p) => gsum(p.grades[key]) >= min).map((p) => ({ p, g: p.grades[key], n: gsum(p.grades[key]) }));
    const tn = gsum(s.team.grades[key]);
    return tn ? [{ p: { id: 'team', number: '', name: 'Team' }, g: s.team.grades[key], n: tn }, ...rows] : [];
  };
</script>

<svelte:head><title>Sideout — Auswertung{match ? ` · ${match.opponent}` : ''}</title></svelte:head>
<main class="page">
  {#if error}<div class="panel empty">{error}</div>
  {:else if !match}<div class="panel empty">Lade…</div>
  {:else}
    <div class="head">
      <h1>{$me?.team?.name || 'Wir'} – {match.opponent}</h1>
      <span class="res">{full.sets_won}:{full.sets_lost}</span>
      <span class="sets">{full.sets.map((x) => x.us + ':' + x.them).join('  ')}{#if !full.finished && actions.length} <em>(Satz {full.set} läuft: {full.us}:{full.them})</em>{/if}</span>
      <span class="muted small">{fmtDate(match.date)}{match.hall ? ' · ' + match.hall : ''} · {match.home ? 'Heim' : 'Auswärts'}</span>
    </div>
    <div class="toolbar">
      <div class="tabs">
        <button class:active={set === 0} onclick={() => (set = 0)}>Gesamt</button>
        {#each Array.from({ length: nSets }, (_, i) => i + 1) as n}<button class:active={set === n} onclick={() => (set = n)}>Satz {n}</button>{/each}
      </div>
      <span class="spacer"></span>
      {#if match.status !== 'done'}<a class="btn" href="{base}/live/{id}">Live</a>{/if}
      <a class="btn" href="{base}/api/matches/{id}/export.csv" download>Export CSV</a>
    </div>

    {@const t = s.team}
    {@const so = t.sideout.n ? t.sideout.won / t.sideout.n : null}
    {@const br = t.brk.n ? t.brk.won / t.brk.n : null}
    <section class="tiles" style="margin-bottom:14px">
      <div class="tile"><div class="v">{t.us}<small>:{t.them}</small></div><div class="k">Punkte</div><div class="d">{set ? 'Satz ' + set : 'alle Sätze'}</div></div>
      <div class="tile"><div class="v">{so == null ? '–' : Math.round(so * 100)}<small>%</small></div><div class="k">Sideout</div><div class="d">{t.sideout.won} von {t.sideout.n} Annahme-Rallys gewonnen</div></div>
      <div class="tile"><div class="v">{br == null ? '–' : Math.round(br * 100)}<small>%</small></div><div class="k">Break</div><div class="d">{t.brk.won} von {t.brk.n} eigenen Aufschlägen gewonnen</div></div>
      <div class="tile"><div class="v">{tot.A.n ? eff((tot.A.k - tot.A.e - tot.A.blk) / tot.A.n) : '–'}</div><div class="k">Angriff Eff.</div><div class="d">{tot.A.k} Pkt · {tot.A.e + tot.A.blk} Fehler · {tot.A.n} Versuche</div></div>
      <div class="tile"><div class="v">{tot.R.n ? (tot.R.sum / tot.R.n).toFixed(2) : '–'}</div><div class="k">Annahme Ø</div><div class="d">{tot.R.n} Annahmen, Skala 0–3</div></div>
      <div class="tile"><div class="v">{errs}</div><div class="k">Eigene Fehler</div><div class="d">von {t.them} Gegnerpunkten</div></div>
    </section>

    <section class="tbl-wrap" style="margin-bottom:6px">
      <table class="tbl">
        <thead>
          <tr><th class="l name" rowspan="2">Spielerin</th><th rowspan="2">Pkt</th><th class="grp sep" colspan="6">Angriff</th><th class="grp sep" colspan="3">Aufschlag</th><th class="grp sep" colspan="4">Annahme</th><th class="grp sep" colspan="3">Block</th><th class="grp sep" colspan="3">Abwehr</th><th class="grp sep" colspan="3">Zuspiel</th></tr>
          <tr><th class="sub sep">Vers</th><th class="sub">Pkt</th><th class="sub">Feh</th><th class="sub">Blk</th><th class="sub">Eff</th><th class="sub">Quote</th>
              <th class="sub sep">Vers</th><th class="sub">Ass</th><th class="sub">Feh</th>
              <th class="sub sep">Vers</th><th class="sub">Ø</th><th class="sub">Pos%</th><th class="sub">Feh</th>
              <th class="sub sep">Pkt</th><th class="sub">Touch</th><th class="sub">Feh</th>
              <th class="sub sep">Abw</th><th class="sub">Vers</th><th class="sub">Feh</th>
              <th class="sub sep">Vers</th><th class="sub">Vorl</th><th class="sub">Feh</th></tr>
        </thead>
        <tbody>
          {#each s.players as p (p.id)}
            <tr><td class="l name"><span class="jersey">{p.number}</span>{p.name} <span class="muted small">{p.pos}</span></td><td><b>{p.pts}</b></td>
              <td class="sep">{p.A.n}</td><td>{p.A.k}</td><td>{p.A.e}</td><td>{p.A.blk}</td><td class={cls(p.A.pct, 0.3, 0.1)}>{eff(p.A.pct)}</td><td>{pct(p.A.kpct)}</td>
              <td class="sep">{p.S.n}</td><td>{p.S.ace}</td><td>{p.S.err}</td>
              <td class="sep">{p.R.n}</td><td class={cls(p.R.avg, 2.2, 1.5)}>{fix(p.R.avg)}</td><td>{pct(p.R.pospct)}</td><td>{p.R.err}</td>
              <td class="sep">{p.B.pts}</td><td>{p.B.touch}</td><td>{p.B.err}</td>
              <td class="sep">{p.D.good}</td><td>{p.D.n}</td><td>{p.D.err}</td>
              <td class="sep">{p.E.n}</td><td>{p.E.ast}</td><td>{p.E.err}</td></tr>
          {:else}
            <tr><td colspan="24" class="empty">Noch keine Aktionen.</td></tr>
          {/each}
          <tr class="total"><td class="l name">Team</td><td>{tot.pts}</td>
            <td class="sep">{tot.A.n}</td><td>{tot.A.k}</td><td>{tot.A.e}</td><td>{tot.A.blk}</td><td>{tot.A.n ? eff((tot.A.k - tot.A.e - tot.A.blk) / tot.A.n) : '–'}</td><td>{pct(tot.A.n ? tot.A.k / tot.A.n : null)}</td>
            <td class="sep">{tot.S.n}</td><td>{tot.S.ace}</td><td>{tot.S.err}</td>
            <td class="sep">{tot.R.n}</td><td>{tot.R.n ? (tot.R.sum / tot.R.n).toFixed(2) : '–'}</td><td>{pct(tot.R.n ? tot.R.pos / tot.R.n : null)}</td><td>{tot.R.err}</td>
            <td class="sep">{tot.B.pts}</td><td>{tot.B.touch}</td><td>{tot.B.err}</td>
            <td class="sep">{tot.D.good}</td><td>{tot.D.n}</td><td>{tot.D.err}</td>
            <td class="sep">{tot.E.n}</td><td>{tot.E.ast}</td><td>{tot.E.err}</td></tr>
        </tbody>
      </table>
    </section>
    <p class="small muted" style="margin:0 0 14px 4px">Eff. = (Punkte − Fehler − geblockt) / Versuche · Ann Ø auf der 0–3-Skala (Perfekt 3 · Gut 2 · OK 1 · Schwach/Overpass/Fehler 0) · Pos% = Anteil Perfekt + Gut</p>

    <div class="grid2">
      <section class="panel">
        <div class="panel-head"><h2>Spielverlauf</h2><span class="small muted">Satz {flow.fset} · Vorsprung nach jedem Ballwechsel</span></div>
        <svg class="flow" viewBox="0 0 600 150" preserveAspectRatio="none">
          <line class="grid" x1="20" x2="590" y1={flow.yTop} y2={flow.yTop} /><line class="grid" x1="20" x2="590" y1={flow.yBot} y2={flow.yBot} /><line class="zero" x1="20" x2="590" y1={flow.y0} y2={flow.y0} />
          <path class="ar-us" d={flow.areaUs} /><path class="ar-them" d={flow.areaThem} /><path class="ln" d={flow.path} />
          <text x="2" y={flow.yTop + 4}>+{flow.maxD}</text><text x="2" y={flow.yBot + 4}>−{flow.maxD}</text><text x="2" y={flow.y0 + 4}>0</text><text x="560" y="146">{flow.n} Rallys</text>
        </svg>
        <div class="legend"><span><i class="gdot g-win"></i> Wir führen</span><span><i class="gdot g-err"></i> Gegner führt</span></div>
      </section>
      <section class="panel">
        <div class="panel-head"><h2>Sideout nach Rotation</h2><span class="small muted">Rotation = wer auf Position I steht</span></div>
        <div class="rot">
          {#each rotations as r (r.pid)}
            <div class="r"><div class="t">{r.setterPos ? 'Zuspiel auf ' + ROMAN[r.setterPos - 1] : 'Rotation'}</div><div class="v">{r.sop == null ? '–' : r.sop}<small>%</small></div><div class="w">{r.r.so.won}/{r.r.so.n} SO · {r.r.brk.won}/{r.r.brk.n} Break</div><div class="t">{byId[r.pid]?.number} {firstName(byId[r.pid])} auf I</div></div>
          {/each}
        </div>
      </section>
      {#each [['Woher die Punkte kommen', cats.pts], ['Wie Punkte verloren gehen', cats.lost]] as [title, items]}
        <section class="panel">
          <div class="panel-head"><h2>{title}</h2></div>
          {#if sum(items)}
            <div class="stack" style="height:22px">{#each items.filter((i) => i.n) as i}<span class={i.cls} style="width:{(i.n / sum(items)) * 100}%" title="{i.label}: {i.n}"></span>{/each}</div>
            <div class="legend">{#each items as i}<span><i class="gdot {i.cls}"></i> {i.label} <b>{i.n}</b> ({Math.round((i.n / sum(items)) * 100)} %)</span>{/each}</div>
          {:else}<div class="empty">Keine Daten.</div>{/if}
        </section>
      {/each}
      <section class="panel">
        <div class="panel-head"><h2>Punkte pro Spielerin</h2></div>
        <div class="bars">
          {#each s.players.filter((p) => p.pts || p.A.n) as p (p.id)}
            <div class="bar-row p"><div class="lbl"><b>{p.number}</b> {firstName(p)} <span class="muted small">{p.A.k} Ang · {p.S.ace} Ass · {p.B.pts} Blk</span></div><div class="trk"><div class="fill" style="width:{(p.pts / maxPts) * 100}%"></div></div><div class="val">{p.pts}</div></div>
          {:else}<div class="empty">Keine Daten.</div>{/each}
        </div>
      </section>
      {#each [['Annahme-Qualität', 'R', 2], ['Angriff-Qualität', 'A', 2], ['Aufschlag-Qualität', 'S', 1]] as [title, key, min]}
        {@const rows = gradeRows(key, min)}
        <section class="panel">
          <div class="panel-head"><h2>{title}</h2><span class="small muted">Anteil je Bewertung · Spielerinnen ab {min} Versuchen</span></div>
          {#each rows as { p, g, n } (p.id)}
            <div class="stackrow" class:team={p.id === 'team'}><div class="lbl"><b>{p.number || p.name}</b>{#if p.number} {firstName(p)}{/if}</div><div class="stack">{#each GRADES.filter((x) => g[x]) as x}<span class={GRADE_CLASS[x]} style="width:{(g[x] / n) * 100}%" title="{x} {PAD[key][x] || GRADE_NAME[x]}: {g[x]} ({Math.round((g[x] / n) * 100)} %)"></span>{/each}</div><div class="n">{n}</div></div>
          {:else}<div class="empty">Keine Daten.</div>{/each}
          {#if rows.length}<div class="legend">{#each GRADES as g}<span><i class="gdot {GRADE_CLASS[g]}"></i> {g} {GRADE_NAME[g]}</span>{/each}</div>{/if}
        </section>
      {/each}
    </div>
  {/if}
</main>

<style>
  .head { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 14px; margin-bottom: 12px; }
  .head h1 { font-size: 24px; }
  .head .res { font-family: var(--disp); font-size: 30px; font-weight: 700; }
  .head .sets { color: var(--ink-2); }
  :global(:root) { --s1: #3987e5; --s2: #d95926; --s3: #199e70; --s4: #c98500; }
  :global(:root[data-theme='light']) { --s1: #2a78d6; --s2: #eb6834; --s3: #1baf7a; --s4: #eda100; }
  :global(.s1) { background: var(--s1); } :global(.s2) { background: var(--s2); } :global(.s3) { background: var(--s3); } :global(.s4) { background: var(--s4); }
  .bar-row.p { grid-template-columns: 120px 1fr 44px; }
  .bar-row.p .lbl b { color: var(--ink); }
  .stackrow { display: grid; grid-template-columns: 120px 1fr 36px; gap: 8px; align-items: center; font-size: 13px; margin-bottom: 6px; }
  .stackrow .lbl { color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .stackrow .lbl b { color: var(--ink); }
  .stackrow .n { color: var(--ink-3); text-align: right; font-size: 12px; }
  .stackrow.team { padding-bottom: 6px; margin-bottom: 8px; border-bottom: 1px solid var(--line); }
  svg.flow { width: 100%; height: 150px; display: block; }
  .flow .grid { stroke: var(--line-soft); stroke-width: 1; }
  .flow .zero { stroke: var(--ink-3); stroke-width: 1; }
  .flow .ln { fill: none; stroke: var(--accent); stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
  .flow .ar-us { fill: var(--g-win); opacity: 0.18; } .flow .ar-them { fill: var(--g-err); opacity: 0.18; }
  .flow text { font-size: 10px; fill: var(--ink-3); font-family: var(--ui); }
  .rot { display: grid; grid-template-columns: repeat(6, 1fr); gap: 6px; }
  .rot .r { background: var(--raised); border-radius: 8px; padding: 8px 6px; text-align: center; }
  .rot .r .t { font-size: 11px; color: var(--ink-3); }
  .rot .r .v { font-family: var(--disp); font-size: 22px; font-weight: 700; line-height: 1.1; }
  .rot .r .v small { font-size: 11px; color: var(--ink-3); font-weight: 600; }
  .rot .r .w { font-size: 11px; color: var(--ink-2); }
  @media (max-width: 600px) { .rot { grid-template-columns: repeat(3, 1fr); } }
  .tbl th.sub { font-size: 11px; color: var(--ink-3); font-weight: 500; }
</style>
