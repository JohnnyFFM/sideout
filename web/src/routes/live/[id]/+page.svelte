<script>
  // The capture screen. Renders from confirmed actions + a local op queue,
  // so every tap shows instantly and survives a dead connection.
  import { page } from '$app/stores';
  import { untrack } from 'svelte';
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
  const logRows = $derived(st ? st.rows.filter((r) => r.set === st.set).slice(-14).reverse() : []);
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
  let armed = $state(null);   // chip id waiting for a slot
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
  function chipMove(e) {
    if (!drag) return;
    const moved = drag.moved || Math.hypot(e.clientX - drag.x, e.clientY - drag.y) > 6;
    drag = { ...drag, x: e.clientX, y: e.clientY, moved };
    over = moved ? slotUnder(e.clientX, e.clientY) : null;
  }
  function chipUp(e) {
    if (!drag) return;
    const d = drag; drag = null; over = null;
    if (d.moved) { const pos = slotUnder(e.clientX, e.clientY); if (pos) dropOn(d.id, pos); }
    else armed = armed === d.id ? null : d.id;   // a tap arms the chip
  }
  function slotTap(pid, pos) {
    if (armed) { dropOn(armed, pos); armed = null; return; }
    selected = selected === pid ? null : pid;
  }
  function dropOn(chipId, pos) {
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
    if (e.key === 'Escape') { selected = null; armed = null; }
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
    <div class="live">
      <div class="col">
        <section class="score">
          <div class="side us"><span class="serve" class:on={st.serving}></span><span class="team">{$me?.team?.short || 'Wir'}</span><span class="pts">{st.us}</span></div>
          <div class="colon">:</div>
          <div class="side them"><span class="pts">{st.them}</span><span class="team">{match.opponent.split(' ')[0]}</span><span class="serve" class:on={!st.serving}></span></div>
          <div class="meta">
            <span>Satz <b>{st.set}</b></span><span>Sätze <b>{st.sets_won}:{st.sets_lost}</b></span>
            {#if st.sets.length}<span><b>{st.sets.map((s) => s.us + ':' + s.them).join('  ')}</b></span>{/if}
            <span>{st.serving ? 'Aufschlag' : 'Annahme'} · Rally <b>{st.rally}</b></span>
          </div>
        </section>
        {#if missingLineupForSet}
          <div class="panel hint">Satz {st.set}: Aufstellung von Satz {st.set - 1} übernommen. <a href="/spiele/{id}?set={st.set}">Anpassen</a></div>
        {/if}
        <section class="court-wrap">
          <Court {court} {byId} {selected} serving={st.serving} {suggested} {over} armed={!!armed} onselect={slotTap} />
          {#if benchChips.length}
            <div class="bench" onpointermove={chipMove} onpointerup={chipUp} onpointercancel={chipUp}>
              {#each benchChips as p (p.id)}
                <button class="chipb" class:libero={p.id === st.libero} class:out={liberoCard?.replaced === p.id} class:armed={armed === p.id} class:dragging={drag?.id === p.id} onpointerdown={(e) => chipDown(e, p.id)} disabled={!canScout || st.finished}>
                  <b>{p.number}</b><span>{firstName(p)}</span><small>{chipNote(p)}</small>
                </button>
              {/each}
            </div>
          {/if}
          {#if drag?.moved}
            <div class="dragghost" style="left:{drag.x}px; top:{drag.y}px">{byId[drag.id]?.number} {firstName(byId[drag.id])}</div>
          {/if}
          <div class="court-foot">
            {#if armed}
              <span class="chip pos-{byId[armed]?.position}">{byId[armed]?.number} {firstName(byId[armed])}</span><span>{armed === st.libero ? 'auf eine Hinterzonen-Karte tippen (I, V, VI)' : liberoCard?.replaced === armed ? 'auf ihre Karte tippen, dann geht die Libera raus' : 'Position tippen, die sie übernimmt'}</span><span class="spacer"></span><button class="btn ghost sm" onclick={() => (armed = null)}>Abbrechen</button>
            {:else if selected}
              <span class="chip pos-{byId[selected]?.position}">{byId[selected]?.number} {firstName(byId[selected])}</span><span>ausgewählt, jetzt Aktion tippen</span>
            {:else}
              <span>Spielerin tippen, dann Aktion. Wechsel: Chip auf eine Karte ziehen.</span>
            {/if}
          </div>
        </section>
        <section class="panel last" id="lastBox">
          <div class="txt">{last ? 'Zuletzt: ' + describe(last) : 'Noch keine Aktion.'}{#if ops.length}<span class="pending"> · {ops.length} ausstehend</span>{/if}</div>
          <button class="btn" onclick={undo} disabled={!last || !canScout}>↶ Rückgängig</button>
        </section>
      </div>

      <div class="col">
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

      <div class="col">
        <section class="panel">
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
        <section class="panel">
          <div class="panel-head"><h2>Verlauf</h2><a class="small" href="/auswertung/{id}">Auswertung →</a></div>
          <ul class="log">
            {#each logRows as r (r.seq)}
              {@const sc = r.outcome ? `${r.outcome === 'us' ? r.us + 1 : r.us}:${r.outcome === 'them' ? r.them + 1 : r.them}` : ''}
              <li class:pt-us={r.outcome === 'us'} class:pt-them={r.outcome === 'them'}>
                <span class="sc">{sc}</span>
                {#if r.skill === 'sub'}<span class="who">Wechsel {byId[r.sub_out]?.number} → {byId[r.sub_in]?.number}</span><span></span>
                {:else if r.skill === 'lib'}<span class="who">Libera {r.sub_in == null ? 'raus' : r.sub_out ? 'für ' + byId[r.sub_out]?.number : 'automatisch'}</span><span></span>
                {:else if r.skill === 'opp'}<span class="who"><small>Gegner</small> {r.grade === '=' ? 'Fehler' : 'Punkt'}</span><span></span>
                {:else}<span class="who"><b>{byId[r.player_id]?.number}</b> {firstName(byId[r.player_id])} <small>{SKILL[r.skill].name}</small></span><span class="g {GRADE_CLASS[r.grade]}">{r.grade}</span>{/if}
              </li>
            {:else}
              <li class="muted">Satz beginnt.</li>
            {/each}
          </ul>
        </section>
      </div>
    </div>
  {/if}
</main>



<style>
  .live { display: grid; gap: 12px; grid-template-columns: 1fr; }
  @media (min-width: 760px) { .live { grid-template-columns: 340px 1fr; align-items: start; } }
  @media (min-width: 1100px) { .live { grid-template-columns: 340px 1fr 360px; } }
  .col { display: grid; gap: 12px; align-content: start; }
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
  .chipb.armed { border-color: var(--accent); background: var(--accent-soft); box-shadow: 0 0 0 2px var(--accent-soft); }
  .chipb.dragging { opacity: 0.4; }
  .chipb:disabled { opacity: 0.4; cursor: default; }
  .dragghost {
    position: fixed; z-index: 50; transform: translate(-50%, -120%); pointer-events: none;
    padding: 6px 12px; border-radius: 18px; background: var(--accent); color: #fff; font-weight: 700; box-shadow: 0 6px 20px #0008;
  }
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
  .log { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; font-size: 13px; }
  .log li { display: grid; grid-template-columns: 40px 1fr auto; gap: 8px; align-items: center; padding: 4px 0; border-bottom: 1px solid var(--line-soft); }
  .log li .sc { color: var(--ink-3); font-variant-numeric: tabular-nums; font-size: 12px; }
  .log li .g { width: 22px; height: 22px; border-radius: 5px; display: grid; place-items: center; font-family: var(--disp); font-weight: 700; font-size: 14px; }
  .log li .who { display: flex; gap: 6px; align-items: center; }
  .log li .who small { color: var(--ink-3); }
  .log li.pt-us .sc { color: var(--g-win); font-weight: 700; }
  .log li.pt-them .sc { color: var(--g-err); font-weight: 700; }
  @media (max-width: 759px) {
    .live-page { padding-bottom: 130px; }
    .live { gap: 8px; } .col { gap: 8px; }
    .court-wrap { padding: 8px; }
    .court-foot { margin-top: 6px; }
    .pad-foot .btn { height: 48px; }
    #lastBox { position: fixed; left: 8px; right: 8px; bottom: calc(66px + env(safe-area-inset-bottom)); z-index: 20; box-shadow: 0 6px 20px #0009; padding: 8px 10px; }
  }
  @media (max-width: 759px) {
    :global(.score) { padding: 4px 12px; }
    :global(.score .pts) { font-size: 40px; }
    :global(.score .colon) { font-size: 28px; }
    :global(.score .meta) { gap: 10px; font-size: 11px; }
  }
</style>
