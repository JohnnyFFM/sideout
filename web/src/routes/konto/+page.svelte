<script>
  // Konto: the login itself — display name and password. Team things live on /teams.
  import { api } from '$lib/api.js';
  import { me, showToast, refreshMe } from '$lib/stores.js';
  import { initialsOf } from '$lib/people.js';

  const initials = $derived(initialsOf($me?.user?.display_name || $me?.user?.username));
  const counts = $derived.by(() => {
    const n = { coach: 0, assistant: 0, viewer: 0 };
    for (const t of $me?.teams || []) n[t.role] = (n[t.role] || 0) + 1;
    return n;
  });
  const teamsWord = (n) => (n === 1 ? '1 Team' : `${n} Teams`);
  const roleLine = $derived(
    [counts.coach && `Trainer:in in ${teamsWord(counts.coach)}`, counts.assistant && `Co-Trainer:in in ${counts.assistant}`, counts.viewer && `Nur lesen in ${counts.viewer}`]
      .filter(Boolean)
      .join(' · ')
  );

  let display = $state('');
  let saved = $state(null); // the server's value the field was last filled from
  // an identity refresh (tab focus, reconnect) must not wipe what is being
  // typed: the field is reset only when the saved name really changed
  $effect(() => {
    const u = $me?.user;
    const now = u ? (u.display_name !== u.username ? u.display_name : '') : null;
    if (now !== null && now !== saved) { saved = now; display = now; }
  });
  let busy = $state(false);
  async function saveName(e) {
    e.preventDefault();
    busy = true;
    try { await api('/me', { method: 'PATCH', body: { display_name: display } }); await refreshMe(); showToast('Gespeichert'); }
    catch (err) { showToast(err.message, true); }
    finally { busy = false; }
  }

  let current = $state('');
  let next = $state('');
  let repeat = $state('');
  async function changePassword(e) {
    e.preventDefault();
    if (next.length < 8) { showToast('Passwort zu kurz (min. 8 Zeichen)', true); return; }
    if (next !== repeat) { showToast('Die Wiederholung stimmt nicht überein', true); return; }
    busy = true;
    try { await api('/me/password', { method: 'POST', body: { current, new: next } }); current = next = repeat = ''; showToast('Passwort geändert'); }
    catch (err) { showToast(err.message, true); }
    finally { busy = false; }
  }
</script>

<svelte:head><title>Sideout — Konto</title></svelte:head>
<main class="page narrow">
  <div class="toolbar"><div><h1>Konto</h1><div class="tsub">dein Login, gilt für alle Teams</div></div></div>
  <section class="panel">
    <div class="who">
      <div class="avatar">{initials}</div>
      <div><div class="nm">{$me?.user?.display_name}</div><div class="lg">{roleLine}</div></div>
    </div>
  </section>
  <section class="panel">
    <div class="panel-head"><h2>Name</h2></div>
    <form onsubmit={saveName}>
      <label class="f">Login<input type="text" value={$me?.user?.username || ''} readonly /></label>
      <label class="f" style="margin-top:10px">Anzeigename<input type="text" bind:value={display} placeholder={$me?.user?.username || ''} maxlength="60" /></label>
      <p class="hint">So siehst du für andere im Team aus. Leer lassen = Login.</p>
      <div style="margin-top:12px"><button class="btn primary" type="submit" disabled={busy}>Speichern</button></div>
    </form>
  </section>
  <section class="panel">
    <div class="panel-head"><h2>Passwort</h2></div>
    <form onsubmit={changePassword}>
      <label class="f">Aktuelles Passwort<input type="password" bind:value={current} autocomplete="current-password" required /></label>
      <div class="row" style="margin-top:10px">
        <label class="f">Neues Passwort<input type="password" bind:value={next} placeholder="mind. 8 Zeichen" autocomplete="new-password" required /></label>
        <label class="f">Wiederholen<input type="password" bind:value={repeat} autocomplete="new-password" required /></label>
      </div>
      <div style="margin-top:12px"><button class="btn primary" type="submit" disabled={busy}>Passwort ändern</button></div>
    </form>
  </section>
  <section class="panel later">
    <div class="panel-head"><h2>E-Mail</h2><span class="chip new">später</span></div>
    <p class="small muted" style="margin:0">Kommt mit dem Registrierungs-Konzept: Konto per E-Mail anlegen, Passwort zurücksetzen, Einladung per Link statt Code.</p>
  </section>
  <p class="foot">Abmelden findest du im Menü oben rechts.</p>
</main>

<style>
  .page.narrow { max-width: 640px; }
  .toolbar .tsub { font-size: 12px; color: var(--ink-3); }
  .who { display: grid; grid-template-columns: 56px 1fr; gap: 14px; align-items: center; margin-bottom: 4px; }
  .avatar { width: 56px; height: 56px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent-text); font-family: var(--disp); font-weight: 700; font-size: 22px; border: 1px solid var(--line-soft); }
  .who .nm { font-size: 19px; font-weight: 600; line-height: 1.15; }
  .who .lg { font-size: 12px; color: var(--ink-3); margin-top: 2px; }
  .hint { font-size: 12px; color: var(--ink-3); margin: 6px 0 0; }
  input:read-only { color: var(--ink-3); background: transparent; }
  .later { border-style: dashed; }
  .chip.new { height: 20px; padding: 0 7px; font-size: 10px; letter-spacing: 0.06em; text-transform: uppercase; border-style: dashed; color: var(--ink-3); }
  .foot { font-size: 12px; color: var(--ink-3); margin-top: 10px; }
</style>
