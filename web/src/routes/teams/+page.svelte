<script>
  import { base } from '$app/paths';
  // Teams: add a team first, then one collapsible card per team with
  // Stammdaten, roster summary and the people (roles, join code, leave).
  import { goto } from '$app/navigation';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, meCache, showToast, refreshMe, switchTeam, reconnectSSE, mutations } from '$lib/stores.js';
  import { POS_NAME } from '$lib/engine.js';
  import { ROLES, ROLE_SHORT, initialsOf } from '$lib/people.js';

  const teams = $derived($me?.teams || []);
  const activeId = $derived($me?.team?.id);
  let open = $state({});      // team id → expanded
  let detail = $state({});    // team id → GET /teams/{id}
  let forms = $state({});     // team id → editable stammdaten
  let busy = $state(false);

  async function loadDetail(id) {
    try {
      const d = await api(`/teams/${id}`);
      detail = { ...detail, [id]: d };
      if (!forms[id]) forms = { ...forms, [id]: { name: d.name, short: d.short, league: d.league, season: d.season } };
    } catch (err) { showToast(err.message, true); }
  }
  // counts sit in every header, so all details load in the background once
  $effect(() => { const ids = teams.map((t) => t.id); untrack(() => ids.forEach((id) => { if (!detail[id]) loadDetail(id); })); });
  $effect(() => { const m = $mutations; if (m?.entity === 'team' || m?.entity === 'player' || m?.entity === 'resync') untrack(() => Object.keys(detail).forEach((id) => loadDetail(Number(id)))); });

  const toggle = (id) => (open = { ...open, [id]: !open[id] });
  function subline(t) {
    const d = detail[t.id];
    const parts = [t.league, t.season].filter(Boolean);
    const people = d ? [`${d.player_count} Spielerinnen`, `${d.member_count} ${d.member_count === 1 ? 'Person' : 'Personen'}`] : [];
    return { parts, people };
  }

  async function pick(t) {
    if (t.id === activeId) return;
    try { await switchTeam(t.id); showToast(`Aktiv: ${t.name}`); } catch (err) { showToast(err.message, true); }
  }
  async function toKader(t) {
    try { if (t.id !== activeId) await switchTeam(t.id); goto(`${base}/kader`); } catch (err) { showToast(err.message, true); }
  }

  async function saveTeam(e, t) {
    e.preventDefault();
    busy = true;
    try { await api(`/teams/${t.id}`, { method: 'PATCH', body: forms[t.id] }); await refreshMe(); await loadDetail(t.id); showToast('Gespeichert'); }
    catch (err) { showToast(err.message, true); }
    finally { busy = false; }
  }
  async function setRole(t, m, role) {
    try { await api(`/teams/${t.id}/members/${m.id}`, { method: 'PATCH', body: { role } }); await loadDetail(t.id); showToast('Rolle geändert'); }
    catch (err) { showToast(err.message, true); }
  }
  async function removeMember(t, m) {
    if (!confirm(`${m.display_name} aus ${t.name} entfernen?`)) return;
    try { await api(`/teams/${t.id}/members/${m.id}`, { method: 'DELETE' }); await loadDetail(t.id); }
    catch (err) { showToast(err.message, true); }
  }
  async function rotate(t) {
    if (!confirm('Neuen Team-Code erzeugen? Der alte Code wird ungültig.')) return;
    try { await api(`/teams/${t.id}/rotate-code`, { method: 'POST' }); await loadDetail(t.id); }
    catch (err) { showToast(err.message, true); }
  }
  async function leave(t) {
    if (!confirm(`${t.name} verlassen? Du siehst das Team danach nicht mehr.`)) return;
    try {
      const r = await api(`/teams/${t.id}/membership`, { method: 'DELETE' });
      if (r.active_team == null && teams.length <= 1) { meCache.value = null; goto(`${base}/login`); return; }
      await refreshMe(); reconnectSSE(); showToast(`${t.name} verlassen`);
    } catch (err) { showToast(err.message, true); }
  }

  let newTeam = $state('');
  let joinCode = $state('');
  async function createTeam(e) {
    e.preventDefault();
    busy = true;
    try {
      const r = await api('/teams', { method: 'POST', body: { name: newTeam } });
      newTeam = '';
      await refreshMe(); reconnectSSE();
      open = { ...open, [r.team.id]: true };
      showToast('Team angelegt und aktiv');
    } catch (err) { showToast(err.message, true); }
    finally { busy = false; }
  }
  async function joinTeam(e) {
    e.preventDefault();
    busy = true;
    try {
      const r = await api('/teams/join', { method: 'POST', body: { code: joinCode } });
      joinCode = '';
      await refreshMe(); reconnectSSE();
      open = { ...open, [r.team.id]: true };
      showToast('Beigetreten und aktiv');
    } catch (err) { showToast(err.message, true); }
    finally { busy = false; }
  }
  const coachNames = (d) => (d?.members || []).filter((m) => m.role === 'coach').map((m) => m.display_name).join(', ');
</script>

<svelte:head><title>Sideout — Teams</title></svelte:head>
<main class="page mid">
  <div class="toolbar"><div><h1>Teams</h1><div class="tsub">ein Login, mehrere Teams · antippen zum Aufklappen</div></div></div>

  <section class="panel">
    <div class="panel-head"><h2>Team hinzufügen</h2><span class="small muted">neu anlegen oder per Code beitreten</span></div>
    <div class="addteam">
      <form onsubmit={createTeam}><label class="f">Neues Team anlegen<input type="text" bind:value={newTeam} placeholder="z. B. TSV Eintracht III" required /></label><button class="btn" type="submit" disabled={busy}>Anlegen</button></form>
      <form onsubmit={joinTeam}><label class="f">Team beitreten<input type="text" bind:value={joinCode} placeholder="Team-Code XXX-0000" autocapitalize="characters" required /></label><button class="btn" type="submit" disabled={busy}>Beitreten</button></form>
    </div>
    <p class="hint">Wer per Code beitritt, kann erst mal nur lesen. Trainer:innen befördern im Team zur Co-Trainer:in.</p>
  </section>
  <p class="foot" style="margin:14px 0 8px">Das aktive Team steht oben in der Leiste. Spiele, Auswertung und Kader gehören immer zum aktiven Team.</p>

  {#each teams as t (t.id)}
    {@const d = detail[t.id]}
    {@const s = subline(t)}
    {@const coach = t.role === 'coach'}
    <section class="panel team" class:open={open[t.id]} class:cur={t.id === activeId}>
      <div class="team-head" role="button" tabindex="0" onclick={() => toggle(t.id)} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(t.id); } }}>
        <span>
          <span class="t">{t.name}</span>
          <div class="s">{s.parts.length ? s.parts.join(' · ') + ' · ' : ''}<b>{ROLE_SHORT[t.role]}</b>{s.people.length ? ' · ' + s.people.join(' · ') : ''}</div>
        </span>
        <span class="slot">
          {#if t.id === activeId}<span class="chip aktiv">aktiv</span>{:else}<button class="btn" onclick={(e) => { e.stopPropagation(); pick(t); }}>Wechseln</button>{/if}
        </span>
        <button class="fold" title="Auf- / zuklappen" aria-expanded={!!open[t.id]} onclick={(e) => { e.stopPropagation(); toggle(t.id); }}><i>▾</i></button>
      </div>
      {#if open[t.id]}
        <div class="team-body">
          <div class="sub">
            <div class="sub-head"><h3>Stammdaten</h3>{#if !coach}<span class="small muted">nur die Trainer:in ändert das</span>{/if}</div>
            {#if coach && forms[t.id]}
              <form onsubmit={(e) => saveTeam(e, t)}>
                <div class="row">
                  <label class="f" style="flex:2">Name<input type="text" bind:value={forms[t.id].name} required /></label>
                  <label class="f short">Kürzel<input type="text" bind:value={forms[t.id].short} maxlength="4" /></label>
                </div>
                <div class="row" style="margin-top:10px">
                  <label class="f">Liga<input type="text" bind:value={forms[t.id].league} placeholder="z. B. Landesliga Süd" /></label>
                  <label class="f">Saison<input type="text" bind:value={forms[t.id].season} placeholder="2026/27" /></label>
                </div>
                <div style="margin-top:10px"><button class="btn primary" type="submit" disabled={busy}>Speichern</button></div>
              </form>
            {:else}
              <div class="small">{[t.name, t.short, t.league, t.season].filter(Boolean).join(' · ')}</div>
            {/if}
          </div>
          <div class="sub">
            <div class="sub-head"><h3>Kader</h3><span class="small muted">öffnet den Kader dieses Teams</span></div>
            {#if d}
              <div class="kader">
                <div class="n">{d.player_count}<small>aktiv{#if d.inactive_count} · {d.inactive_count} ehemalige{/if}</small></div>
                <div class="pos">{#each Object.entries(POS_NAME) as [k, v]}{#if d.positions?.[k]}<span class="chip pos-{k}">{d.positions[k]} {v}</span>{/if}{/each}</div>
                <button class="btn go" onclick={() => toKader(t)} title="wählt das Team und öffnet den Kader">Kader bearbeiten →</button>
              </div>
            {:else}<div class="small muted">Lade…</div>{/if}
          </div>
          <div class="sub">
            <div class="sub-head"><h3>Trainer:innen &amp; Helfer</h3><span class="small muted">Rollen ändert nur die Trainer:in</span></div>
            {#if d}
              <ul class="members">
                {#each d.members as m (m.id)}
                  <li>
                    <span class="av" class:me={m.id === $me?.user?.id}>{initialsOf(m.display_name)}</span>
                    <span><b>{m.display_name}</b>{#if m.id === $me?.user?.id} <span class="muted small">(du)</span>{/if}<div class="u">{m.username}</div></span>
                    {#if coach && m.id !== $me?.user?.id}
                      <select value={m.role} onchange={(e) => setRole(t, m, e.target.value)}>{#each Object.entries(ROLES) as [k, v]}<option value={k}>{v}</option>{/each}</select>
                      <button class="icon-btn" title="Entfernen" onclick={() => removeMember(t, m)}>🗑</button>
                    {:else}
                      <span class="chip">{ROLES[m.role]}</span><span></span>
                    {/if}
                  </li>
                {/each}
              </ul>
              {#if coach}
                <div class="invite">
                  <div class="code">{d.join_code || '–'}</div>
                  <div class="small muted">{d.member_count <= 1 ? 'Noch niemand dabei. Code weitergeben; wer beitritt, kann erst mal nur lesen.' : 'Team-Code zum Beitreten. Wer beitritt, kann erst mal nur lesen. Zum Scouten oben auf Co-Trainer:in setzen.'}</div>
                  <button class="btn" onclick={() => rotate(t)}>Neu erzeugen</button>
                </div>
              {:else}
                <p class="small muted" style="margin:10px 0 0">Du bist {ROLE_SHORT[t.role]}: Rollen und Team-Code verwaltet {coachNames(d) || 'die Trainer:in'}. <button class="danger-link" onclick={() => leave(t)}>Team verlassen</button></p>
              {/if}
            {:else}<div class="small muted">Lade…</div>{/if}
          </div>
        </div>
      {/if}
    </section>
  {/each}
</main>

<style>
  .page.mid { max-width: 760px; }
  .toolbar .tsub { font-size: 12px; color: var(--ink-3); }
  .hint { font-size: 12px; color: var(--ink-3); margin: 6px 0 0; }
  .foot { font-size: 12px; color: var(--ink-3); }
  .chip.aktiv { background: var(--accent); border-color: var(--accent); color: #fff; }
  .addteam { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  @media (max-width: 640px) { .addteam { grid-template-columns: 1fr; } }
  .addteam form { display: grid; grid-template-columns: 1fr auto; gap: 8px; align-items: end; }
  .team { padding: 0; overflow: hidden; }
  .team + .team { margin-top: 10px; }
  .team-head { display: grid; grid-template-columns: 1fr auto auto; gap: 10px; align-items: center; padding: 12px 12px 12px 16px; cursor: pointer; }
  .team-head:hover { background: var(--raised); }
  .team-head .t { font-size: 16px; font-weight: 600; }
  .team-head .s { font-size: 12px; color: var(--ink-3); margin-top: 2px; }
  .team-head .s b { color: var(--ink-2); font-weight: 600; }
  .team-head .fold { width: 34px; height: 34px; border-radius: var(--r-m); border: 1px solid var(--line-soft); background: var(--raised); color: var(--ink-2); display: grid; place-items: center; cursor: pointer; padding: 0; font-size: 18px; line-height: 1; }
  .team-head .fold:hover { color: var(--ink); }
  .team-head .fold i { display: inline-block; font-style: normal; transition: transform .15s; }
  .team.open .team-head .fold i { transform: rotate(180deg); }
  .team.open .team-head { border-bottom: 1px solid var(--line-soft); background: var(--raised); }
  .team.cur { border-color: var(--accent); }
  .team-body { padding: 4px 16px 16px; }
  .sub { margin-top: 16px; }
  .sub-head { display: flex; align-items: baseline; gap: 10px; margin-bottom: 8px; }
  .sub-head h3 { flex: 1; color: var(--ink); font-weight: 600; font-size: 14px; }
  .f.short { flex: 0; min-width: 90px; }
  .kader { display: grid; grid-template-columns: auto 1fr auto; gap: 12px; align-items: center; background: var(--raised); border-radius: var(--r-m); padding: 10px 12px; }
  .kader .n { font-family: var(--disp); font-size: 30px; font-weight: 700; line-height: 1; }
  .kader .n small { font-size: 12px; color: var(--ink-3); font-family: var(--ui); font-weight: 500; display: block; margin-top: 2px; }
  .kader .pos { display: flex; flex-wrap: wrap; gap: 4px; }
  .kader .pos .chip { height: 22px; font-size: 11px; padding: 0 8px; }
  @media (max-width: 520px) { .kader { grid-template-columns: auto 1fr; } .kader .go { grid-column: 1 / -1; } }
  .members { list-style: none; margin: 0; padding: 0; }
  .members li { display: grid; grid-template-columns: 32px 1fr auto auto; gap: 10px; align-items: center; padding: 8px 0; border-bottom: 1px solid var(--line-soft); font-size: 14px; }
  .members li:last-child { border-bottom: 0; }
  .members .av { width: 32px; height: 32px; border-radius: 50%; display: grid; place-items: center; background: var(--raised); border: 1px solid var(--line-soft); font-family: var(--disp); font-weight: 700; font-size: 13px; color: var(--ink-2); }
  .members .av.me { background: var(--accent-soft); color: var(--accent-text); }
  .members .u { font-size: 12px; color: var(--ink-3); }
  .members select { width: auto; height: 32px; font-size: 13px; padding: 0 6px; }
  .members .icon-btn { width: 32px; height: 32px; font-size: 13px; }
  @media (max-width: 520px) { .members select { max-width: 130px; } }
  .invite { display: grid; grid-template-columns: auto 1fr auto; gap: 12px; align-items: center; border: 1px dashed var(--line); border-radius: var(--r-m); padding: 10px 12px; margin-top: 10px; }
  .invite .code { font-family: var(--disp); font-size: 26px; font-weight: 700; letter-spacing: 0.08em; }
  .invite .small { line-height: 1.35; }
  @media (max-width: 520px) { .invite { grid-template-columns: 1fr auto; grid-auto-flow: dense; } .invite .small { grid-column: 1 / -1; } }
  .danger-link { color: var(--g-err); background: none; border: 0; font-weight: 600; cursor: pointer; font-size: 13px; padding: 0; font-family: inherit; }
</style>
