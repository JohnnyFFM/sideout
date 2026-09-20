<script>
  import { base } from '$app/paths';
  // Picks a match to evaluate: the running one first, then the finished ones.
  import { api } from '$lib/api.js';
  import { fmtDate } from '$lib/engine.js';
  let matches = $state(null);
  let error = $state('');
  $effect(() => { api('/matches').then((r) => (matches = r.matches.filter((m) => m.status !== 'planned'))).catch((e) => (error = e.offline ? 'Keine Verbindung.' : e.message)); });
</script>

<svelte:head><title>Sideout — Auswertung</title></svelte:head>
<main class="page">
  <div class="toolbar"><h1>Auswertung</h1></div>
  {#if error}<div class="panel empty">{error}</div>
  {:else if !matches}<div class="panel empty">Lade…</div>
  {:else if !matches.length}<div class="panel empty">Noch kein gescoutetes Spiel.</div>
  {:else}
    <section class="panel">
      <ul class="list">
        {#each matches as m (m.id)}
          <li><a href="{base}/auswertung/{m.id}"><span class="d">{fmtDate(m.date)}</span><b>{m.opponent}</b><span class="muted small">{m.state.sets.map((s) => s.us + ':' + s.them).join(' ')}{m.status === 'live' ? ' · läuft' : ''}</span><span class="res">{m.state.sets_won}:{m.state.sets_lost}</span></a></li>
        {/each}
      </ul>
    </section>
  {/if}
</main>

<style>
  .list { list-style: none; margin: 0; padding: 0; }
  .list li a { display: grid; grid-template-columns: 70px 1fr auto auto; gap: 10px; align-items: center; padding: 10px 4px; border-bottom: 1px solid var(--line-soft); color: var(--ink); text-decoration: none; }
  .list li:last-child a { border-bottom: 0; }
  .list li a:hover { background: var(--raised); }
  .d { color: var(--ink-3); font-size: 12px; }
  .res { font-family: var(--disp); font-size: 20px; font-weight: 700; }
</style>
