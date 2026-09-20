<script>
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, showToast, mutations } from '$lib/stores.js';
  import MatchForm from '$lib/components/MatchForm.svelte';
  let match = $state(null);
  let error = $state('');
  const id = $derived(Number($page.params.id));
  const set = $derived(Number($page.url.searchParams.get('set') || match?.state?.set || 1));
  const load = () => api(`/matches/${id}`).then((m) => (match = m)).catch((e) => (error = e.message));
  $effect(() => { void id; untrack(load); });
  // ownership moved (a scout event) or the match changed: the form's notice follows
  $effect(() => { const m = $mutations; if (m && m.entity === 'match' && m.id === id) untrack(load); });
  async function remove() {
    if (!confirm(`Spiel gegen ${match.opponent} mit allen Aktionen löschen?`)) return;
    try { await api(`/matches/${id}`, { method: 'DELETE' }); showToast('Gelöscht'); goto(`${base}/spiele`); }
    catch (e) { showToast(e.message, true); }
  }
</script>

<svelte:head><title>Sideout — Spiel bearbeiten</title></svelte:head>
<main class="page">
  <div class="toolbar"><h1>Spiel bearbeiten</h1><span class="spacer"></span>
    {#if match}<a class="btn" href="{base}/auswertung/{id}">Auswertung</a>{/if}
    {#if match && $me?.user?.role === 'coach'}<button class="btn danger" onclick={remove}>Löschen</button>{/if}
  </div>
  {#if error}<div class="panel empty">{error}</div>
  {:else if match}
    {#if match.state?.set > 1 || match.actions?.length}
      <p class="small muted" style="margin:0 0 12px">Aufstellung für Satz
        {#each Array.from({ length: Math.max(match.state.set, 1) }, (_, i) => i + 1) as s}<a href="{base}/spiele/{id}?set={s}" style="margin-left:6px; font-weight:{s === set ? 700 : 400}">{s}</a>{/each}
        — bereits gescoutete Sätze werden nicht rückwirkend geändert.</p>
    {/if}
    {#key set}<MatchForm {match} players={match.players} {set} onscout={(m) => (match = { ...match, scout: m.scout })} />{/key}
  {:else}<div class="panel empty">Lade Spiel…</div>{/if}
</main>
