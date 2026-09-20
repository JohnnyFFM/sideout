<script>
  // The capture screen. Renders from confirmed actions + a local op queue,
  // so every tap shows instantly and survives a dead connection.
  import { page } from '$app/stores';
  import { untrack, tick } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations, online, showToast } from '$lib/stores.js';
  import { replay, courtView, expectedSkills, stats, SKILL, PAD, GRADE_CLASS, ROMAN, firstName, eff, fix } from '$lib/engine.js';
  import { loadOps, saveOps, applyOps, cacheMatch, cachedMatch } from '$lib/offline.js';
  import Court from '$lib/components/Court.svelte';
  import Pad from '$lib/components/Pad.svelte';

  const id = $derived(Number($page.params.id));
  let match = $state(null);
  let ops = $state([]);
  let selected = $state(null);
  let scope = $state('set');
  let flushing = false;
  let loadError = $state('');

  const canScout = $derived(['coach', 'assistant'].includes($me?.user?.role));
  const byId = $derived(Object.fromEntries((match?.players || []).map((p) => [p.id, p])));
  const cfg = $derived(match ? { first_serve_us: match.first_serve === 'us', lineups: match.lineups } : null);
  const actionsAll = $derived(match ? applyOps(match.actions, ops) : []);
  const st = $derived(cfg ? replay(cfg, actionsAll) : null);
  const hasLineup = $derived(!!match && Object.keys(match.lineups || {}).length > 0);
  const court = $derived(st ? courtView(st.lineup, st.libero, byId, st.libero_for, st.libero_off, st.serving) : []);
  const expected = $derived(st ? expectedSkills(st) : []);
  // who the next action most likely belongs to: the server on I, the setter for a set
  const setters = $derived(court.filter((c) => byId[c.id]?.position === 'Z' && !c.libero).map((c) => c.id));
  const suggested = $derived.by(() => {
    if (!st) return [];
    if (expected.includes('S') && st.serving) return [st.lineup[0]];
    if (expected.includes('E')) return setters;
    return [];
  });
  const last = $derived(actionsAll[actionsAll.length - 1] || null);
  const stat = $derived(st ? stats(cfg, match.players, actionsAll, scope === 'set' ? st.set : 0) : null);
  // timeline: every action left to right, a score pill after each rally, a divider per set
  const timeline = $derived.by(() => {
    if (!st) return [];
    const out = []; let set = 0;
    for (const r of st.rows) {
      if (r.set !== set) { set = r.set; out.push({ t: 'set', n: set, key: 'set' + set }); }
      out.push({ t: 'act', r, key: 'a' + r.seq });
      if (r.outcome) out.push({ t: 'score', us: r.us + (r.outcome === 'us' ? 1 : 0), them: r.them + (r.outcome === 'them' ? 1 : 0), won: r.outcome === 'us', key: 's' + r.seq });
    }
    return out;
  });
  let tlEl = $state(null);
  // newest entry stays in view
  $effect(() => { void timeline.length; const el = tlEl; if (el) tick().then(() => { el.scrollLeft = el.scrollWidth; }); });
  const missingLineupForSet = $derived(st && st.set > 1 && !match.lineups?.[st.set]);

  async function load() {
    const saved = loadOps(id);
    if (JSON.stringify(saved) !== JSON.stringify(ops)) ops = saved;
    try {
      const m = await api(`/matches/${id}`);
      match = m;
      cacheMatch(id, m);
      loadError = '';
    } catch (e) {
      const c = cachedMatch(id);
      if (c) { match = c; showToast('Offline: Stand vom letzten Laden'); }
      else loadError = e.offline ? 'Keine Verbindung und kein lokaler Stand.' : e.message;
    }
    flush();
  }

  // effects only track their trigger; everything they call runs untracked,
  // otherwise a reload that rewrites `ops` would re-trigger itself forever
  $effect(() => { void id; untrack(load); });

  // another device wrote to this match → refetch when we have nothing pending
  $effect(() => {
    const m = $mutations;
    if (!m) return;
    untrack(() => {
      if (match && ((m.entity === 'action' || m.entity === 'match') && m.id === id || m.entity === 'resync') && ops.length === 0) load();
    });
  });
  $effect(() => { if ($online) untrack(flush); });

  // phone: the live screen has no top bar. Scoped via a body class, because a
  // :global rule in this component would stay loaded after leaving the page.
  $effect(() => {
    document.body.classList.add('live-phone');
    return () => document.body.classList.remove('live-phone');
  });

  // keep the screen on while scouting
  $effect(() => {
    let lock = null;
    const req = () => navigator.wakeLock?.request?.('screen').then((l) => (lock = l)).catch(() => {});
    req();
    const vis = () => { if (document.visibilityState === 'visible') req(); };
    document.addEventListener('visibilitychange', vis);
    return () => { document.removeEventListener('visibilitychange', vis); lock?.release?.(); };
  });

  function describe(a) {
    if (!a) return '';
    if (a.skill === 'adj') return a.grade === '#' ? 'Nachtrag: +1 wir' : 'Nachtrag: +1 Gegner';
    if (a.skill === 'rot') return 'Nachtrag: rotiert';
    if (a.skill === 'srv') return a.grade === '#' ? 'Nachtrag: Aufschlag wir' : 'Nachtrag: Aufschlag Gegner';
    if (a.skill === 'lib') return a.sub_in == null ? 'Libera raus' : a.sub_out ? `Libera für ${byId[a.sub_out]?.number} ${firstName(byId[a.sub_out])}` : 'Libera automatisch (Mitte)';
    if (a.skill === 'sub') return `Wechsel ${byId[a.sub_out]?.number} → ${byId[a.sub_in]?.number}`;
    if (a.skill === 'opp') return a.grade === '=' ? 'Fehler Gegner' : 'Punkt Gegner';
    const p = byId[a.player_id];
    return `${p?.number} ${firstName(p)} ${SKILL[a.skill].name} ${a.grade} ${PAD[a.skill][a.grade] || ''}`;
  }

  function queue(a) {
    if (!canScout) return;
    if (st.finished) { showToast('Das Spiel ist beendet'); return; }
    const before = st;
    const seq = (actionsAll[actionsAll.length - 1]?.seq || 0) + 1;
    ops = [...ops, { type: 'add', action: { id: -seq, seq, grade: null, player_id: null, sub_out: null, sub_in: null, ...a } }];
    saveOps(id, ops);
    selected = null;
    const after = replay(cfg, applyOps(match.actions, ops));
    if (after.set !== before.set) { const s = after.sets[after.sets.length - 1]; showToast(`Satz ${before.set} beendet ${s.us}:${s.them}`); }
    else if (after.finished) showToast(`Spiel beendet ${after.sets_won}:${after.sets_lost}`);
    flush();
  }

  async function flush() {
    if (flushing || !match) return;
    flushing = true;
    try {
      while (ops.length) {
        const op = ops[0];
        try {
          if (op.type === 'add') {
            const res = await api(`/matches/${id}/actions`, { method: 'POST', body: op.action });
            match = { ...match, actions: [...match.actions.filter((x) => x.seq !== res.action.seq), res.action] };
          } else {
            await api(`/matches/${id}/actions/last`, { method: 'DELETE' });
            match = { ...match, actions: match.actions.slice(0, -1) };
          }
          ops = ops.slice(1);
          saveOps(id, ops);
          cacheMatch(id, match);
        } catch (e) {
          if (e.offline) break; // retry when back online
          if (e.status === 409) {
            showToast('Ein anderes Gerät hat gescoutet, neu geladen', true);
            ops = []; saveOps(id, ops);
            const m = await api(`/matches/${id}`); match = m; cacheMatch(id, m);
            break;
          }
          showToast(e.message, true);
          ops = ops.slice(1); saveOps(id, ops);
        }
      }
    } finally {
      flushing = false;
    }
  }

  function tapCell(skill, grade) {
    let who = selected;
    if (!who && skill === 'S' && st.serving) who = st.lineup[0];
    if (!who && skill === 'E' && setters.length === 1) who = setters[0];
    if (!who) { showToast('Erst Spielerin auf dem Feld tippen'); return; }
    queue({ skill, grade, player_id: who });
  }

  // ----- bench strip: libero pinned first, then the bench. A chip is
  // dragged onto a court slot (pointer events, works with touch) or tapped
  // to arm it, then the target slot is tapped. -----
  let phoneView = $state('pad');   // phone: 'pad' | 'stats'
  let extrasOpen = $state(false);  // phone: bench + catch-up shown
  let drag = $state(null);    // { id, x, y, moved }
  let over = $state(null);    // court position under the dragged chip
  // the bench = everyone not on the court right now. The libero leads it
  // while she sits; while she stands in for someone, that someone leads it.
  const liberoCard = $derived(court.find((c) => c.libero) || null);
  const benchChips = $derived.by(() => {
    if (!match || !st) return [];
    const rest = match.players.filter((p) => p.active && !st.lineup.includes(p.id) && p.id !== st.libero).sort((a, b) => a.number - b.number);
    const first = [];
    if (st.libero && !liberoCard && byId[st.libero]) first.push(byId[st.libero]);
    if (liberoCard && byId[liberoCard.replaced]) first.push(byId[liberoCard.replaced]);
    return first.concat(rest);
  });
  const chipNote = (p) => (p.id === st?.libero ? 'Libera' : liberoCard?.replaced === p.id ? `${p.position} · draußen für L` : p.position);

  function slotUnder(x, y) {
    const el = document.elementFromPoint(x, y)?.closest('.slot');
    return el ? Number(el.dataset.pos) : null;
  }
  function chipDown(e, pid) {
    if (!canScout || st.finished) return;
    e.preventDefault();
    try { e.currentTarget.setPointerCapture?.(e.pointerId); } catch { /* synthetic or already-released pointer */ }
    drag = { id: pid, x: e.clientX, y: e.clientY, moved: false };
  }
  // the court positions a chip may be dropped on
  function validTargets(chipId) {
    if (!st) return [];
    if (chipId === st.libero) {
      return [5, 6, 1].filter((pos) => !(pos === 1 && st.serving) && court[pos - 1].id !== st.libero);
    }
    if (liberoCard && chipId === liberoCard.replaced) return [liberoCard.pos];
    return [1, 2, 3, 4, 5, 6];
  }
  // targets light up the moment a finger lands on a chip, not only once it moves
  const targets = $derived(drag ? validTargets(drag.id) : []);
  function chipMove(e) {
    if (!drag) return;
    const moved = drag.moved || Math.hypot(e.clientX - drag.x, e.clientY - drag.y) > 6;
    drag = { ...drag, x: e.clientX, y: e.clientY, moved };
    const pos = moved ? slotUnder(e.clientX, e.clientY) : null;
    over = pos && validTargets(drag.id).includes(pos) ? pos : null;
  }
  function chipUp(e) {
    if (!drag) return;
    const d = drag; drag = null; over = null;
    if (!d.moved) { showToast('Zum Wechseln den Chip auf eine Karte ziehen'); return; }
    const pos = slotUnder(e.clientX, e.clientY);
    if (!pos) return;
    if (!validTargets(d.id).includes(pos)) {
      showToast(d.id === st.libero ? 'Die Libera kann nur hinten stehen: V, VI und I bei gegnerischem Aufschlag' : liberoCard && d.id === liberoCard.replaced ? `${byId[d.id]?.number} gehört zu Position ${ROMAN[liberoCard.pos - 1]}` : 'Hier nicht ablegbar');
      return;
    }
    dropOn(d.id, pos);
  }
  function slotTap(pid) {
    selected = selected === pid ? null : pid;
  }
  function dropOn(chipId, pos) {
    extrasOpen = false;
    const slot = court[pos - 1];
    const target = slot.replaced || slot.id;   // the player who actually holds the position
    if (chipId === st.libero) {
      if (pos === 1 && st.serving) { showToast('Auf I schlägt gerade unsere Spielerin auf, die Libera kann dort erst bei gegnerischem Aufschlag stehen'); return; }
      if (pos !== 1 && pos !== 5 && pos !== 6) { showToast('Die Libera kann nur hinten stehen: I, V oder VI'); return; }
      queue({ skill: 'lib', sub_out: target, sub_in: st.libero });
      showToast(`Libera für ${byId[target]?.number} ${firstName(byId[target])}`);
      return;
    }
    if (liberoCard && chipId === liberoCard.replaced) {
      // the player the libero stands in for comes back onto her own card
      if (slot.replaced === chipId) { queue({ skill: 'lib', sub_out: null, sub_in: null }); showToast(`Libera raus, ${byId[chipId]?.number} ${firstName(byId[chipId])} wieder auf ${ROMAN[pos - 1]}`); }
      else showToast(`${byId[chipId]?.number} gehört zu Position ${ROMAN[liberoCard.pos - 1]}, dort ablegen`);
      return;
    }
    if (target === chipId) return;
    queue({ skill: 'sub', sub_out: target, sub_in: chipId });
    showToast(`Wechsel ${byId[target]?.number} → ${byId[chipId]?.number}`);
  }
  function undo() {
    if (!actionsAll.length) return;
    const a = last;
    const lastOp = ops[ops.length - 1];
    if (lastOp?.type === 'add') ops = ops.slice(0, -1);
    else ops = [...ops, { type: 'undo' }];
    saveOps(id, ops);
    selected = null;
    showToast('Rückgängig: ' + describe(a));
    flush();
  }

  function keys(e) {
    if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT') return;
    if (e.key >= '1' && e.key <= '6' && court.length) { selected = court[+e.key - 1].id; }
    if (e.key === 'z' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); undo(); }
    if (e.key === 'Escape') { selected = null; }
  }
</script>

<svelte:window onkeydown={keys} />
<svelte:head><title>Sideout — Live{match ? ` · ${match.opponent}` : ''}</title></svelte:head>

<main class="page live-page">
  {#if loadError}
    <div class="panel empty">{loadError}</div>
  {:else if !match}
    <div class="panel empty">Lade Spiel…</div>
  {:else if !hasLineup}
    <div class="panel">
      <h2>Aufstellung fehlt</h2>
      <p class="muted">Ohne Startaufstellung kann nicht gescoutet werden.</p>
      <a class="btn primary big" href="/spiele/{id}">Aufstellung eintragen</a>
    </div>
  {:else}
    <div class="live" class:pv-stats={phoneView === 'stats'} class:extras={extrasOpen}>
      <div class="col left">
        <section class="score">
          <button class="board-btn left" class:on={phoneView === 'stats'} onclick={() => (phoneView = phoneView === 'stats' ? 'pad' : 'stats')} title="Live-Werte"><svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M4 20h16v-2H4v2zm2-4h3V7H6v9zm5 0h3V4h-3v12zm5 0h3v-6h-3v6z"/></svg></button>
          <button class="board-btn right" class:on={extrasOpen} onclick={() => (extrasOpen = !extrasOpen)} title="Nachtragen: Spielstand, Rotation, Aufschlag"><svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zm17.71-10.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z"/></svg></button>
          <div class="side us"><span class="serve" class:on={st.serving}></span><span class="team">{$me?.team?.short || 'Wir'}</span><span class="pts">{st.us}</span></div>
          <div class="colon">:</div>
          <div class="side them"><span class="pts">{st.them}</span><span class="team">{match.opponent.split(' ')[0]}</span><span class="serve" class:on={!st.serving}></span></div>
          <div class="meta">
            <span>Satz <b>{st.set}</b></span><span>Sätze <b>{st.sets_won}:{st.sets_lost}</b></span>
            {#if st.sets.length}<span><b>{st.sets.map((s) => s.us + ':' + s.them).join('  ')}</b></span>{/if}
            <span>{st.serving ? 'Aufschlag' : 'Annahme'} · Rally <b>{st.rally}</b></span>
          </div>
          {#if canScout && !st.finished}
            <div class="catch" title="Nachtragen: Spielstand, Rotation und Aufschlag ohne Aktionen angleichen">
              <button onclick={() => queue({ skill: 'adj', grade: '#' })}>+1 wir</button>
              <button onclick={() => queue({ skill: 'adj', grade: '=' })}>+1 Gegner</button>
              <button onclick={() => queue({ skill: 'rot' })}>⟳ Rotieren</button>
              <button onclick={() => queue({ skill: 'srv', grade: st.serving ? '=' : '#' })}>Aufschlag {st.serving ? 'Gegner' : 'wir'}</button>
            </div>
          {/if}
        </section>
        {#if missingLineupForSet}
          <div class="panel hint">Satz {st.set}: Aufstellung von Satz {st.set - 1} übernommen. <a href="/spiele/{id}?set={st.set}">Anpassen</a></div>
        {/if}
        <section class="court-wrap">
          <Court {court} {byId} {selected} serving={st.serving} {suggested} {over} dragging={!!drag} {targets} onselect={slotTap} />
          {#if benchChips.length}
            <div class="bench" onpointermove={chipMove} onpointerup={chipUp} onpointercancel={chipUp}>
              {#each benchChips as p (p.id)}
                <button class="chipb" class:libero={p.id === st.libero} class:out={liberoCard?.replaced === p.id} class:dragging={drag?.id === p.id} onpointerdown={(e) => chipDown(e, p.id)} disabled={!canScout || st.finished}>
                  <b>{p.number}</b><span>{firstName(p)}</span><small>{chipNote(p)}</small>
                </button>
              {/each}
            </div>
          {/if}
          {#if selected && !drag}
            <div class="court-foot">
              <span class="chip pos-{byId[selected]?.position}">{byId[selected]?.number} {firstName(byId[selected])}</span><span>ausgewählt, jetzt Aktion tippen</span>
            </div>
          {/if}
        </section>
        <section class="panel last" id="lastBox">
          <div class="txt">{last ? 'Zuletzt: ' + describe(last) : 'Noch keine Aktion.'}{#if ops.length}<span class="pending"> · {ops.length} ausstehend</span>{/if}</div>
          <button class="btn" onclick={undo} disabled={!last || !canScout}><svg class="ico-undo" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M12.5 8c-2.65 0-5.05.99-6.9 2.6L2 7v9h9l-3.62-3.62c1.39-1.16 3.16-1.88 5.12-1.88 3.54 0 6.55 2.31 7.6 5.5l2.37-.78C21.08 11.03 17.15 8 12.5 8z"/></svg> Rückgängig</button>
        </section>
      </div>

      <div class="col mid">
        <Pad {expected} idle={!selected} disabled={!canScout || st.finished} ontap={tapCell} />
        <div class="pad-foot">
          <button class="btn big us" disabled={!canScout || st.finished} onclick={() => queue({ skill: 'opp', grade: '=' })}>Fehler Gegner <span class="muted">+1 wir</span></button>
          <button class="btn big them" disabled={!canScout || st.finished} onclick={() => queue({ skill: 'opp', grade: '#' })}>Punkt Gegner</button>
        </div>
        {#if st.finished}
          <div class="panel done">
            <h2>Spiel beendet {st.sets_won}:{st.sets_lost}</h2>
            <p class="muted">{st.sets.map((s) => s.us + ':' + s.them).join('   ')}</p>
            <a class="btn primary" href="/auswertung/{id}">Zur Auswertung</a>
          </div>
        {/if}
      </div>

      <div class="col right">
        <section class="panel stats-panel">
          <div class="panel-head"><h2>{scope === 'set' ? 'Dieser Satz' : 'Ganzes Spiel'}</h2>
            <span class="tabs"><button class:active={scope === 'set'} onclick={() => (scope = 'set')}>Satz</button><button class:active={scope === 'match'} onclick={() => (scope = 'match')}>Spiel</button></span></div>
          {#if stat}
            {@const t = stat.team}
            {@const A = stat.players.reduce((a, p) => ({ k: a.k + p.A.k, e: a.e + p.A.e + p.A.blk, n: a.n + p.A.n }), { k: 0, e: 0, n: 0 })}
            <div class="tiles three">
              <div class="tile"><div class="v">{t.sideout.n ? Math.round((t.sideout.won / t.sideout.n) * 100) : '–'}<small>%</small></div><div class="k">Sideout</div><div class="d">{t.sideout.won}/{t.sideout.n} Annahme-Rallys</div></div>
              <div class="tile"><div class="v">{t.brk.n ? Math.round((t.brk.won / t.brk.n) * 100) : '–'}<small>%</small></div><div class="k">Break</div><div class="d">{t.brk.won}/{t.brk.n} Aufschlag-Rallys</div></div>
              <div class="tile"><div class="v">{A.n ? eff((A.k - A.e) / A.n) : '–'}</div><div class="k">Angriff Eff.</div><div class="d">{A.k} Pkt · {A.e} Fehler · {A.n} Vers.</div></div>
            </div>
            <table class="mini">
              <thead><tr><th class="l">Spielerin</th><th>Pkt</th><th>Ang</th><th>Eff</th><th>Ann Ø</th><th>Blo</th><th>Abw</th></tr></thead>
              <tbody>
                {#each stat.players as p (p.id)}
                  <tr><td class="l">{p.number}<small>{firstName(p)}</small></td><td>{p.pts}</td><td>{p.A.k}/{p.A.n}</td><td>{eff(p.A.pct)}</td><td>{fix(p.R.avg)}</td><td>{p.B.pts}</td><td>{p.D.good}/{p.D.n}</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </section>
      </div>
      <section class="panel timeline">
        <div class="tl-head"><h2>Verlauf</h2><span class="small muted">{actionsAll.length} Aktionen</span><span class="spacer"></span><a class="small" href="/auswertung/{id}">Auswertung →</a></div>
        <button class="tl-undo" onclick={undo} disabled={!last || !canScout} title={last ? 'Rückgängig: ' + describe(last) : 'Nichts zum Rückgängigmachen'}><svg class="ico-undo" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M12.5 8c-2.65 0-5.05.99-6.9 2.6L2 7v9h9l-3.62-3.62c1.39-1.16 3.16-1.88 5.12-1.88 3.54 0 6.55 2.31 7.6 5.5l2.37-.78C21.08 11.03 17.15 8 12.5 8z"/></svg>{#if ops.length}<i>{ops.length}</i>{/if}</button>
        <div class="tl" bind:this={tlEl}>
          {#each timeline as e (e.key)}
            {#if e.t === 'set'}
              <div class="tl-set">Satz {e.n}</div>
            {:else if e.t === 'score'}
              <div class="tl-score" class:us={e.won} class:them={!e.won}>{e.us}:{e.them}</div>
            {:else if e.r.skill === 'opp'}
              <div class="tl-item opp"><b>{e.r.grade === '=' ? '✕' : '●'}</b><small>Gegner</small></div>
            {:else if e.r.skill === 'adj'}
              <div class="tl-item sub"><b>+1</b><small>{e.r.grade === '#' ? 'wir' : 'Gegner'}</small></div>
            {:else if e.r.skill === 'rot'}
              <div class="tl-item sub"><b>⟳</b><small>rotiert</small></div>
            {:else if e.r.skill === 'srv'}
              <div class="tl-item sub"><b>S</b><small>{e.r.grade === '#' ? 'wir' : 'Gegner'}</small></div>
            {:else if e.r.skill === 'sub'}
              <div class="tl-item sub"><b>⇄</b><small>{byId[e.r.sub_out]?.number}→{byId[e.r.sub_in]?.number}</small></div>
            {:else if e.r.skill === 'lib'}
              <div class="tl-item sub"><b>L</b><small>{e.r.sub_in == null ? 'raus' : e.r.sub_out ? 'für ' + byId[e.r.sub_out]?.number : 'auto'}</small></div>
            {:else}
              <div class="tl-item" style="background: var(--{GRADE_CLASS[e.r.grade]}); color: var(--{GRADE_CLASS[e.r.grade]}-ink)"><b>{byId[e.r.player_id]?.number}</b><small>{SKILL[e.r.skill].short} {e.r.grade}</small></div>
            {/if}
          {:else}
            <div class="muted small">Noch keine Aktion. Der Verlauf wächst nach rechts.</div>
          {/each}
        </div>
      </section>
    </div>
  {/if}
</main>



<style>
  .live { display: grid; gap: 12px; grid-template-columns: 1fr; }
  @media (min-width: 760px) { .live { grid-template-columns: 340px 1fr; align-items: start; } .timeline { grid-column: 1 / -1; } }
  .col { display: grid; gap: 12px; align-content: start; min-width: 0; }
  .timeline { min-width: 0; }
  :global(.score .catch) { grid-column: 1 / -1; display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; margin-top: 4px; }
  :global(.score .catch button) { height: 30px; border-radius: 6px; border: 1px dashed var(--line); background: transparent; color: var(--ink-2); font-size: 12px; font-weight: 600; cursor: pointer; }
  :global(.score .catch button:hover) { background: var(--raised); color: var(--ink); }
  /* desktop: one screen. Row 1 = three cards of equal height (each scrolls
     inside if it must), row 2 = the timeline across the full width. */
  @media (min-width: 1100px) {
    .live-page { padding-bottom: 16px; }
    .live { grid-template-columns: 340px 1fr 360px; grid-template-rows: minmax(0, 1fr) auto; height: calc(100dvh - 52px - 32px); align-items: stretch; }
    .col { display: flex; flex-direction: column; min-height: 0; }
    .col > :global(*) { flex: 0 0 auto; }
    .col.left .court-wrap { flex: 1 1 auto; display: flex; flex-direction: column; min-height: 0; }
  .col.left :global(.catch) { grid-column: 1 / -1; }
    .col.left .court-wrap :global(.court) { flex: 1 1 auto; }
    .col.mid :global(.pad) { flex: 1 1 auto; grid-template-rows: auto repeat(6, minmax(0, 1fr)); }
    .col.right .stats-panel { flex: 1 1 auto; min-height: 0; overflow: auto; }
    .timeline { grid-column: 1 / -1; }
  }
  /* timeline */
  .timeline { padding: 8px 12px 6px; }
  .tl-head { display: flex; align-items: baseline; gap: 10px; margin-bottom: 6px; }
  .tl-head h2 { font-size: 15px; }
  .tl { display: flex; align-items: center; gap: 4px; overflow-x: auto; padding: 6px 2px 6px; scrollbar-width: thin; }
  .tl-item { flex: 0 0 auto; display: grid; justify-items: center; gap: 2px; width: 50px; padding: 5px 0 4px; border-radius: 7px; background: var(--raised); color: var(--ink-2); }
  .tl-item b { font-family: var(--disp); font-size: 19px; font-weight: 700; line-height: 1; }
  .tl-item small { font-size: 9px; line-height: 1; white-space: nowrap; opacity: 0.85; font-weight: 600; }
  .tl-item.opp { background: var(--overlay); color: var(--ink-2); }
  .tl-item.sub { background: var(--accent-soft); color: var(--accent-text); }
  .tl-score { flex: 0 0 auto; font-family: var(--disp); font-weight: 700; font-size: 13px; padding: 2px 7px; border-radius: 10px; margin: 0 3px; }
  .tl-score.us { background: var(--g-win); color: var(--g-win-ink); }
  .tl-score.them { background: var(--g-err); color: #fff; }
  .tl-set { flex: 0 0 auto; align-self: stretch; display: grid; align-items: center; padding: 0 8px 0 10px; margin: 0 4px; border-left: 2px solid var(--line); font-size: 11px; font-weight: 600; color: var(--ink-3); white-space: nowrap; }
  .court-wrap { background: var(--panel); border: 1px solid var(--line-soft); border-radius: var(--r-l); padding: 10px; }
  .court-foot { display: flex; align-items: center; gap: 8px; margin-top: 8px; font-size: 12px; color: var(--ink-2); min-height: 22px; }
  .bench { display: flex; gap: 6px; margin-top: 8px; overflow-x: auto; padding: 2px 0 4px; scrollbar-width: none; touch-action: none; }
  .bench::-webkit-scrollbar { display: none; }
  .chipb {
    flex: 0 0 auto; display: grid; grid-template-columns: auto auto; align-items: baseline; column-gap: 5px;
    height: 40px; padding: 0 10px; border-radius: 20px; border: 1px solid var(--line); background: var(--raised);
    cursor: grab; touch-action: none; user-select: none; -webkit-user-select: none;
  }
  .chipb b { font-family: var(--disp); font-size: 18px; font-weight: 700; }
  .chipb span { font-size: 12px; color: var(--ink-2); }
  .chipb small { grid-column: 1 / -1; font-size: 10px; color: var(--ink-3); line-height: 1; margin-top: -2px; }
  .chipb.libero { border-color: var(--court-line); background: var(--court-soft); }
  .chipb.libero small { color: var(--court-line); }
  .chipb.out { border-style: dashed; }
  .chipb.dragging { border-color: var(--accent); background: var(--accent-soft); box-shadow: 0 0 0 2px var(--accent-soft); }
  .chipb:disabled { opacity: 0.4; cursor: default; }
  .btn.sm { height: 28px; padding: 0 10px; font-size: 12px; }
  .hint { padding: 8px 12px; font-size: 13px; color: var(--ink-2); }
  .pad-foot { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .pad-foot .btn { justify-content: center; }
  .pad-foot .btn.us { border-color: var(--g-win); color: var(--g-win); }
  .pad-foot .btn.them { border-color: var(--g-err); color: var(--g-err); }
  .done { text-align: center; display: grid; gap: 8px; justify-items: center; }
  .last { display: flex; align-items: center; gap: 10px; }
  .last .txt { flex: 1; font-size: 13px; color: var(--ink-2); min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pending { color: var(--g-neg); }
  .tiles.three { grid-template-columns: repeat(3, 1fr); }
  .mini { width: 100%; border-collapse: collapse; font-size: 13px; margin-top: 12px; }
  .mini th, .mini td { padding: 5px 4px; text-align: right; white-space: nowrap; }
  .mini th { color: var(--ink-3); font-weight: 600; font-size: 11px; border-bottom: 1px solid var(--line-soft); }
  .mini td.l, .mini th.l { text-align: left; }
  .mini td.l { font-weight: 600; }
  .mini td.l small { color: var(--ink-3); font-weight: 500; margin-left: 4px; }
  .board-btn { display: none; }
  .tl-undo { display: none; }
  .ico-undo { display: inline-block; vertical-align: -3px; }
  /* phone: one screen. Top bar hidden (tab bar navigates), score strip with
     Feld/Werte toggle, court, pad, opponent buttons; the timeline is docked
     above the tab bar with the undo button; bench and catch-up fold away. */
  @media (max-width: 759px) {
    .live-page { padding: calc(6px + env(safe-area-inset-top)) 8px calc(var(--tabbar-h) + 8px); }
    /* the columns dissolve so score, timeline, court, pad and buttons stack
       in the order a scouting thumb wants */
    .live { gap: 6px; }
    .col { display: contents; }
    .col.left > :global(.score) { order: 1; }
    .timeline { order: 2; }
    .col.left > .hint { order: 3; }
    .col.left > .court-wrap { order: 4; }
    .col.mid > :global(.pad) { order: 5; }
    .col.mid > .pad-foot { order: 6; }
    .col.mid > .done { order: 7; }
    .col.right > .stats-panel { order: 8; }
    /* the board carries two icon buttons: live stats (left), catch-up (right) */
    .board-btn { display: grid; place-items: center; position: absolute; top: 8px; width: 32px; height: 32px; border-radius: 8px; border: 1px solid var(--line-soft); background: var(--raised); color: var(--ink-2); z-index: 1; }
    .board-btn.left { left: 8px; }
    .board-btn.right { right: 8px; }
    .board-btn.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
    .hint-txt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    :global(.score .catch) { display: none; grid-template-columns: 1fr 1fr 1fr 1.25fr; margin-top: 8px; }
    :global(.score .catch button) { font-size: 11px; padding: 0 2px; white-space: nowrap; }
    .live.extras :global(.score .catch) { display: grid; }
    .bench { display: flex; margin-top: 6px; }
    .chipb { height: 38px; }
    .court-wrap { padding: 6px; }
    .court-foot { margin-top: 4px; min-height: 18px; font-size: 11px; }
    .pad-foot .btn { height: 44px; font-size: 15px; }
    #lastBox { display: none; }
    .col.right > .stats-panel { display: none; }
    .live.pv-stats .col.right > .stats-panel { display: block; }
    .live.pv-stats .col.left > .court-wrap, .live.pv-stats .col.mid > :global(.pad), .live.pv-stats .col.mid > .pad-foot { display: none; }
    .timeline { display: flex; align-items: center; gap: 6px; padding: 6px 8px; }
    .tl-head { display: none; }
    /* undo sits at the right, next to the newest entry, and stays small */
    .tl-undo { display: grid; place-items: center; position: relative; flex: 0 0 auto; order: 2; width: 36px; height: 36px; border-radius: 8px; border: 1px solid var(--line); background: var(--raised); }
    .tl-undo .ico-undo { width: 16px; height: 16px; }
    .tl { order: 1; }
    .tl-undo:disabled { opacity: 0.4; }
    .tl-undo i { position: absolute; top: -5px; right: -5px; min-width: 16px; height: 16px; border-radius: 8px; background: var(--g-neg); color: var(--g-neg-ink); font-size: 10px; font-style: normal; font-weight: 700; display: grid; place-items: center; padding: 0 3px; }
    .tl { flex: 1 1 auto; min-width: 0; padding: 2px 0; gap: 4px; }
    .tl-item { width: 46px; padding: 4px 0 3px; }
    .tl-item b { font-size: 17px; }
    .tl-score { font-size: 12px; padding: 1px 6px; }
  }
  @media (max-width: 759px) and (max-height: 700px) {
    :global(.score) { padding: 6px 42px 6px !important; }
    :global(.score .pts) { font-size: 32px !important; }
    :global(.score .colon) { font-size: 22px !important; }
    :global(.score .meta) { display: none; }
    .board-btn { width: 28px; height: 28px; top: 6px; }
    .chipb { height: 32px; }
    .chipb b { font-size: 15px; }
    .pad-foot .btn { height: 36px; font-size: 14px; }
    .live-page { padding-top: calc(4px + env(safe-area-inset-top)); }
    .tl-item { width: 40px; padding: 3px 0 2px; }
    .tl-item b { font-size: 15px; }
    .tl-undo { width: 32px; height: 32px; }
  }
  @media (max-width: 759px) {
    :global(.score) { position: relative; padding: 8px 46px 8px; row-gap: 2px; }
    :global(.score .pts) { font-size: 42px; }
    :global(.score .colon) { font-size: 28px; }
    :global(.score .meta) { gap: 10px; font-size: 12px; }
    :global(.score .team) { font-size: 13px; }
  }
</style>
