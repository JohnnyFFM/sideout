<script>
  // Roster: add, edit inline, deactivate/delete.
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations, showToast } from '$lib/stores.js';
  import { POS_NAME } from '$lib/engine.js';

  let players = $state(null);
  let error = $state('');
  let number = $state('');
  let name = $state('');
  let position = $state('A');
  let editing = $state(null); // player id being edited
  let draft = $state({});
  let showInactive = $state(false);
  const canEdit = $derived(['coach', 'assistant'].includes($me?.user?.role));

  async function load() { try { players = (await api('/players')).players; } catch (e) { error = e.offline ? 'Keine Verbindung.' : e.message; } }
  $effect(() => { untrack(load); });
  $effect(() => { if ($mutations?.entity === 'player') untrack(load); });

  async function add(e) {
    e.preventDefault();
    try {
      await api('/players', { method: 'POST', body: { number: Number(number), name, position } });
      number = ''; name = ''; showToast('Hinzugefügt'); load();
    } catch (err) { showToast(err.message, true); }
  }
  function startEdit(p) { editing = p.id; draft = { number: p.number, name: p.name, position: p.position }; }
  async function saveEdit(p) {
    try { await api(`/players/${p.id}`, { method: 'PATCH', body: { ...draft, number: Number(draft.number) } }); editing = null; showToast('Gespeichert'); load(); }
    catch (err) { showToast(err.message, true); }
  }
  async function toggleActive(p) {
    try { await api(`/players/${p.id}`, { method: 'PATCH', body: { active: !p.active } }); load(); } catch (err) { showToast(err.message, true); }
  }
  async function remove(p) {
    if (!confirm(`${p.name} aus dem Kader entfernen?`)) return;
    try { const r = await api(`/players/${p.id}`, { method: 'DELETE' }); showToast(r.deactivated ? 'Deaktiviert (hat gescoutete Spiele)' : 'Entfernt'); load(); }
    catch (err) { showToast(err.message, true); }
  }
  const shown = $derived((players || []).filter((p) => showInactive || p.active));
</script>

<svelte:head><title>Sideout — Kader</title></svelte:head>
<main class="page">
  <div class="toolbar"><h1>Kader</h1><span class="spacer"></span>
    <label class="small muted" style="display:flex;gap:6px;align-items:center"><input type="checkbox" bind:checked={showInactive} /> Ehemalige zeigen</label></div>
  {#if error}<div class="panel empty">{error}</div>
  {:else if !players}<div class="panel empty">Lade…</div>
  {:else}
    <div class="grid2">
      <section class="panel">
        <div class="panel-head"><h2>Spielerinnen</h2><span class="small muted">{players.filter((p) => p.active).length} aktiv</span></div>
        {#if !shown.length}<div class="empty">Noch niemand im Kader. Rechts hinzufügen.</div>{/if}
        <div class="roster">
          {#each shown as p (p.id)}
            {#if editing === p.id}
              <div class="row edit">
                <input type="number" min="0" max="99" bind:value={draft.number} style="max-width:70px" />
                <input type="text" bind:value={draft.name} />
                <select bind:value={draft.position}>{#each Object.entries(POS_NAME) as [k, v]}<option value={k}>{v}</option>{/each}</select>
                <button class="btn primary" onclick={() => saveEdit(p)}>OK</button><button class="btn ghost" onclick={() => (editing = null)}>✕</button>
              </div>
            {:else}
              <div class="prow" class:inactive={!p.active}>
                <span class="jersey">{p.number}</span>
                <span class="nm">{p.name}{#if !p.active}<span class="muted small"> · ehemalig</span>{/if}</span>
                <span class="chip pos-{p.position}">{POS_NAME[p.position]}</span>
                {#if canEdit}
                  <span class="acts">
                    <button class="icon-btn" title="Bearbeiten" onclick={() => startEdit(p)}>✎</button>
                    <button class="icon-btn" title={p.active ? 'Deaktivieren' : 'Reaktivieren'} onclick={() => toggleActive(p)}>{p.active ? '⏸' : '▶'}</button>
                    <button class="icon-btn" title="Entfernen" onclick={() => remove(p)}>🗑</button>
                  </span>
                {/if}
              </div>
            {/if}
          {/each}
        </div>
      </section>
      {#if canEdit}
        <section class="panel">
          <div class="panel-head"><h2>Hinzufügen</h2></div>
          <form onsubmit={add} class="addrow">
            <label class="f">Nr.<input type="number" min="0" max="99" bind:value={number} placeholder="7" required /></label>
            <label class="f">Name<input type="text" bind:value={name} placeholder="Vorname Nachname" required /></label>
            <label class="f">Position<select bind:value={position}>{#each Object.entries(POS_NAME) as [k, v]}<option value={k}>{v}</option>{/each}</select></label>
            <button class="btn primary" type="submit">Hinzufügen</button>
          </form>
          <p class="small muted" style="margin:12px 0 0">Z Zuspiel · A Außen · M Mitte · D Diagonal · L Libero. Die Positionen steuern die Libera-Anzeige und die Plausibilitätsprüfung der Aufstellung, nicht die Statistik.</p>
        </section>
      {/if}
    </div>
  {/if}
</main>

<style>
  .roster { display: grid; gap: 4px; }
  .prow { display: grid; grid-template-columns: 40px 1fr auto auto; align-items: center; gap: 8px; padding: 6px 4px; border-radius: 8px; font-size: 14px; }
  .prow:hover { background: var(--raised); }
  .prow.inactive { opacity: 0.55; }
  .jersey { font-family: var(--disp); font-weight: 700; font-size: 18px; text-align: center; }
  .acts { display: flex; gap: 4px; }
  .acts .icon-btn { width: 32px; height: 32px; font-size: 13px; }
  .edit { align-items: center; padding: 4px; }
  .edit input, .edit select { min-width: 0; }
  .addrow { display: grid; grid-template-columns: 70px 1fr 120px auto; gap: 8px; align-items: end; }
  @media (max-width: 520px) { .addrow { grid-template-columns: 70px 1fr; } .prow { grid-template-columns: 36px 1fr auto; } .prow .chip { display: none; } }
</style>
