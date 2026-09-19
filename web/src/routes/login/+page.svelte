<script>
  import { goto, invalidateAll } from '$app/navigation';
  import { api } from '$lib/api.js';

  let mode = $state('login'); // login | register | join
  let busy = $state(false);
  let error = $state('');
  let username = $state('');
  let password = $state('');
  let display_name = $state('');
  let team_name = $state('');
  let code = $state('');

  let theme = $state(typeof window !== 'undefined' ? window.soTheme.get() : 'dark');
  const NEXT_ICON = { dark: '☀', light: '☾' };
  $effect(() => {
    const h = (e) => (theme = e.detail);
    window.addEventListener('so-theme', h);
    return () => window.removeEventListener('so-theme', h);
  });

  let signupOpen = $state(false);
  $effect(() => {
    api('/config').then((c) => (signupOpen = !!c.signup_open)).catch(() => (signupOpen = false));
  });

  async function submit(e) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      if (mode === 'login') await api('/auth/login', { method: 'POST', body: { username, password } });
      else if (mode === 'register') await api('/auth/register-team', { method: 'POST', body: { team_name, display_name, username, password } });
      else await api('/auth/join', { method: 'POST', body: { code, display_name, username, password } });
      await invalidateAll();
      goto('/team');
    } catch (err) {
      error = err.offline ? 'Keine Verbindung' : err.message;
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>Sideout — Anmelden</title></svelte:head>

<button class="icon-btn corner-theme" onclick={() => window.soTheme.cycle()} title="Theme wechseln">{NEXT_ICON[theme]}</button>

<div class="login-wrap">
  <div class="login-card">
    <div class="login-brand"><span class="tick">▲</span>SIDEOUT</div>
    <p class="login-sub">
      {#if mode === 'login'}Einmal anmelden, dieses Gerät bleibt angemeldet.
      {:else if mode === 'register'}Neues Team anlegen. Du wirst Trainer:in.
      {:else}Mit dem Team-Code beitreten.{/if}
    </p>
    <form onsubmit={submit}>
      {#if mode === 'register'}
        <label class="f">Teamname<input type="text" bind:value={team_name} placeholder="z. B. TSV Eintracht" required /></label>
      {/if}
      {#if mode === 'join'}
        <label class="f">Team-Code<input type="text" bind:value={code} placeholder="XXX-0000" autocapitalize="characters" required /></label>
      {/if}
      {#if mode !== 'login'}
        <label class="f">Dein Name<input type="text" bind:value={display_name} placeholder="z. B. Jonas" required /></label>
      {/if}
      <label class="f">Benutzername<input type="text" bind:value={username} autocomplete="username" autocapitalize="none" required /></label>
      <label class="f">Passwort<input type="password" bind:value={password} autocomplete={mode === 'login' ? 'current-password' : 'new-password'} required /></label>
      {#if error}<p class="err">{error}</p>{/if}
      <button class="btn primary big" type="submit" disabled={busy}>
        {#if mode === 'login'}Anmelden{:else if mode === 'register'}Team anlegen{:else}Beitreten{/if}
      </button>
    </form>
    {#if signupOpen}
      <p class="login-foot">
        {#if mode !== 'login'}<a href="/login" onclick={(e) => { e.preventDefault(); mode = 'login'; error = ''; }}>Zurück zur Anmeldung</a> · {/if}
        {#if mode !== 'register'}<a href="/login" onclick={(e) => { e.preventDefault(); mode = 'register'; error = ''; }}>Team anlegen</a> · {/if}
        {#if mode !== 'join'}<a href="/login" onclick={(e) => { e.preventDefault(); mode = 'join'; error = ''; }}>Team-Code eingeben</a>{/if}
      </p>
    {/if}
    <p class="login-foot">Sideout ist Open Source. Sitzungen gelten 12 Monate pro Gerät.</p>
  </div>
</div>
