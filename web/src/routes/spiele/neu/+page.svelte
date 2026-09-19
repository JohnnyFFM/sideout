<script>
  import { api } from '$lib/api.js';
  import MatchForm from '$lib/components/MatchForm.svelte';
  let players = $state(null);
  $effect(() => { api('/players').then((r) => (players = r.players)); });
</script>

<svelte:head><title>Sideout — Spiel anlegen</title></svelte:head>
<main class="page">
  <div class="toolbar"><h1>Spiel anlegen</h1></div>
  {#if players}<MatchForm {players} />{:else}<div class="panel empty">Lade Kader…</div>{/if}
</main>
