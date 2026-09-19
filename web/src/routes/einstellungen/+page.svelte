<script>
  // Team settings, members & roles, join code, logout.
  import { goto, invalidateAll } from '$app/navigation';
  import { api } from '$lib/api.js';
  import { me, showToast } from '$lib/stores.js';

  const isCoach = $derived($me?.user?.role === 'coach');
  let name = $state('');
  let short = $state('');
  let league = $state('');
  let season = $state('');
  $effect(() => { if ($me?.team) { name = $me.team.name; short = $me.team.short; league = $me.team.league; season = $me.team.season; } });
  const ROLES = { coach: 'Trainer:in', assistant: 'Co-Trainer:in / Scout', viewer: 'Nur lesen' };

  async function saveTeam(e) {
    e.preventDefault();
    try { await api('/team', { method: 'PATCH', body: { name, short, league, season } }); await invalidateAll(); showToast('Gespeichert'); }
    catch (err) { showToast(err.message, true); }
  }
  async function rotate() {
    if (!confirm('Neuen Team-Code erzeugen? Der alte Code wird ungültig.')) return;
    try { await api('/team/rotate-code', { method: 'POST' }); await invalidateAll(); } catch (err) { showToast(err.message, true); }
  }
  async function setRole(m, role) {
    try { await api(`/team/members/${m.id}`, { method: 'PATCH', body: { role } }); await invalidateAll(); showToast('Rolle geändert'); } catch (err) { showToast(err.message, true); }
  }
  async function removeMember(m) {
    if (!confirm(`${m.display_name} aus dem Team entfernen?`)) return;
    try { await api(`/team/members/${m.id}`, { method: 'DELETE' }); await invalidateAll(); } catch (err) { showToast(err.message, true); }
  }
  async function logout() { await api('/auth/logout', { method: 'POST' }); goto('/login'); }
  function installHint() { return /iphone|ipad/i.test(navigator.userAgent) ? 'Safari: Teilen → „Zum Home-Bildschirm“.' : 'Browser-Menü → „App installieren“ oder „Zum Startbildschirm“.'; }
</script>

<svelte:head><title>Sideout — Einstellungen</title></svelte:head>
<main class="page">
  <div class="toolbar"><h1>Einstellungen</h1><span class="spacer"></span><button class="btn" onclick={logout}>Abmelden</button></div>
  <div class="grid2">
    <div>
      <section class="panel">
        <div class="panel-head"><h2>Team</h2></div>
        <form onsubmit={saveTeam} class="col">
          <div class="row">
            <label class="f" style="flex:2">Name<input type="text" bind:value={name} disabled={!isCoach} required /></label>
            <label class="f">Kürzel<input type="text" bind:value={short} maxlength="4" disabled={!isCoach} /></label>
          </div>
          <div class="row" style="margin-top:10px">
            <label class="f">Liga<input type="text" bind:value={league} disabled={!isCoach} placeholder="z. B. Landesliga Süd" /></label>
            <label class="f">Saison<input type="text" bind:value={season} disabled={!isCoach} placeholder="2026/27" /></label>
          </div>
          {#if isCoach}<div style="margin-top:12px"><button class="btn primary" type="submit">Speichern</button></div>{/if}
        </form>
      </section>
      {#if isCoach}
        <section class="panel">
          <div class="panel-head"><h2>Team-Code</h2><span class="small muted">zum Beitreten von Co-Trainer:innen</span></div>
          <div class="row" style="align-items:center">
            <div class="code">{$me?.team?.join_code || '–'}</div>
            <button class="btn" onclick={rotate}>Neu erzeugen</button>
          </div>
          <p class="small muted" style="margin:10px 0 0">Wer beitritt, wird Co-Trainer:in und kann scouten. Die Rolle lässt sich unten ändern.</p>
        </section>
      {/if}
      <section class="panel">
        <div class="panel-head"><h2>Anleitung</h2></div>
        <p class="small" style="margin:0">Alles zum Scouten, zur Bewertungsskala und zur Auswertung: <a href="/hilfe/">Anleitung öffnen</a>.</p>
      </section>
      <section class="panel">
        <div class="panel-head"><h2>Als App installieren</h2></div>
        <p class="small" style="margin:0">{installHint()} Danach läuft Sideout im Vollbild, auch ohne Empfang in der Halle: Aktionen werden lokal gespeichert und später gesendet.</p>
      </section>
    </div>
    <section class="panel">
      <div class="panel-head"><h2>Mitglieder</h2></div>
      <ul class="members">
        {#each $me?.members || [] as m (m.id)}
          <li>
            <span><b>{m.display_name}</b><div class="small muted">{m.username}</div></span>
            {#if isCoach && m.id !== $me.user.id}
              <select value={m.role} onchange={(e) => setRole(m, e.target.value)}>{#each Object.entries(ROLES) as [k, v]}<option value={k}>{v}</option>{/each}</select>
              <button class="icon-btn" title="Entfernen" onclick={() => removeMember(m)}>🗑</button>
            {:else}
              <span class="chip">{ROLES[m.role]}</span>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  </div>
  <p class="small muted" style="margin-top:16px">Sideout ist Open Source (MIT). Angemeldet als {$me?.user?.display_name} ({ROLES[$me?.user?.role] || ''}).</p>
</main>

<style>
  .code { font-family: var(--disp); font-size: 30px; font-weight: 700; letter-spacing: 0.08em; }
  .members { list-style: none; margin: 0; padding: 0; }
  .members li { display: grid; grid-template-columns: 1fr auto auto; gap: 8px; align-items: center; padding: 8px 0; border-bottom: 1px solid var(--line-soft); }
  .members li:last-child { border-bottom: 0; }
  .members select { width: auto; }
</style>
