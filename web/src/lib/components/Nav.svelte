<script>
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api.js';
  import { me, meCache, sseConnected, online, switchTeam, showToast } from '$lib/stores.js';
  import { ROLE_SHORT, initialsOf } from '$lib/people.js';
  const teams = $derived($me?.teams || []);
  async function onSwitch(e) {
    const id = Number(e.target.value);
    if (!id || id === $me?.team?.id) return;
    try { await switchTeam(id); goto(`${base}/team`); } catch (err) { showToast(err.message, true); }
  }

  let theme = $state(typeof window !== 'undefined' ? window.soTheme.get() : 'dark');
  const NEXT_ICON = { dark: '☀', light: '☾' };
  $effect(() => {
    const h = (e) => (theme = e.detail);
    window.addEventListener('so-theme', h);
    return () => window.removeEventListener('so-theme', h);
  });

  const path = $derived($page.url.pathname);
  const active = (p) => (path === base + p || path.startsWith(base + p + '/') ? 'active' : '');
  const initials = $derived(initialsOf($me?.user?.display_name || $me?.user?.username));
  // "Live" jumps to the running match if there is one, else to the match list
  const liveHref = $derived($me?.live_match_id ? `${base}/live/${$me.live_match_id}` : '/spiele');

  // ----- account menu: floats under the avatar, above the phone tab bar;
  // closes on outside click, Escape and navigation -----
  let menuOpen = $state(false);
  let avatarEl = $state(null);
  let menuPos = $state({ top: 0, right: 8 });
  function toggleMenu(e) {
    e.stopPropagation();
    if (!menuOpen && avatarEl) {
      const r = avatarEl.getBoundingClientRect();
      menuPos = { top: Math.round(r.bottom + 6), right: Math.max(8, Math.round(window.innerWidth - r.right)) };
    }
    menuOpen = !menuOpen;
  }
  $effect(() => {
    if (!menuOpen) return;
    const close = () => (menuOpen = false);
    const key = (e) => { if (e.key === 'Escape') close(); };
    document.addEventListener('click', close);
    document.addEventListener('keydown', key);
    return () => { document.removeEventListener('click', close); document.removeEventListener('keydown', key); };
  });
  $effect(() => { void path; menuOpen = false; });
  async function logout() {
    menuOpen = false;
    try { await api('/auth/logout', { method: 'POST' }); } catch { /* the cookie is gone either way */ }
    meCache.value = null;
    goto(`${base}/login`);
  }
  const current = (p) => (path === base + p ? 'current' : '');
</script>

<header class="topbar">
  <a class="brand" href="{base}/team"><img class="brandico" src="{base}/icon.svg" alt="" width="22" height="22" />SIDEOUT</a>
  <nav class="nav">
    <a class={active('/team')} href="{base}/team">Team</a>
    <a class={active('/spiele') || active('/live')} href="{base}/spiele">Spiele</a>
    <a class={active('/auswertung')} href="{base}/auswertung">Auswertung</a>
    <a class={active('/kader')} href="{base}/kader">Kader</a>
  </nav>
  <span class="spacer"></span>
  <span class="sync-chip" title={$sseConnected ? 'Live verbunden' : 'Verbindung getrennt'}>
    <span class="dot" style:background={$online && $sseConnected ? 'var(--ok)' : 'var(--g-neg)'}></span>
    {#if teams.length > 1}
      <select class="teamsel" value={$me?.team?.id} onchange={onSwitch} title="Team wechseln">
        {#each teams as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
      </select>
    {:else}
      <span class="desk">{$me?.team?.name || ''}</span>
    {/if}
  </span>
  <button class="icon-btn" onclick={() => window.soTheme.cycle()} title="Theme wechseln">{NEXT_ICON[theme]}</button>
  <button class="avatar-btn" class:on={menuOpen} bind:this={avatarEl} onclick={toggleMenu} title="{$me?.user?.display_name || ''} — Konto" aria-haspopup="menu" aria-expanded={menuOpen}>{initials}</button>
</header>

{#if menuOpen}
  <div class="menu" role="menu" style="top:{menuPos.top}px; right:{menuPos.right}px" onclick={(e) => e.stopPropagation()}>
    <div class="head"><b>{$me?.user?.display_name}</b><span class="sub">{ROLE_SHORT[$me?.user?.role] || ''} · {$me?.team?.name || ''}</span></div>
    <div class="sep"></div>
    <a class={current('/konto')} href="{base}/konto" role="menuitem"><span class="glyph av">{initials}</span><span>Konto<span class="sub2">Name, Passwort</span></span></a>
    <a class={current('/einstellungen')} href="{base}/einstellungen" role="menuitem"><span class="glyph">⚙</span><span>Einstellungen<span class="sub2">Darstellung, App, Diagnose</span></span></a>
    <a class={current('/teams')} href="{base}/teams" role="menuitem"><span class="glyph">⚑</span><span>Teams<span class="sub2">{teams.length} {teams.length === 1 ? 'Team' : 'Teams'} · Stammdaten, Helfer, Team-Code</span></span></a>
    <div class="sep"></div>
    <button class="row-btn out" onclick={logout} role="menuitem"><span class="glyph">⏻</span><span>Abmelden</span></button>
  </div>
{/if}

<nav class="tabbar">
  <a class={active('/team')} href="{base}/team"><span class="ico">⌂</span>Team</a>
  <a class={active('/spiele') || active('/live')} href="{base}/spiele"><span class="ico">●</span>Spiele</a>
  <a class={active('/auswertung')} href="{base}/auswertung"><span class="ico">≡</span>Auswertung</a>
  <a class={active('/kader')} href="{base}/kader"><span class="ico">☷</span>Kader</a>
</nav>

<style>
  .teamsel { height: 30px; max-width: 160px; padding: 0 6px; font-size: 13px; font-weight: 600; background: var(--raised); border: 1px solid var(--line-soft); border-radius: var(--r-m); color: var(--ink); }
  @media (max-width: 760px) {
    .desk { display: none; }
    .teamsel { max-width: 120px; font-size: 16px; height: 32px; }
  }
  .avatar-btn { width: 34px; height: 34px; border-radius: 50%; border: 1px solid var(--line); background: var(--accent-soft); color: var(--accent-text); font-family: var(--disp); font-weight: 700; font-size: 14px; display: grid; place-items: center; cursor: pointer; padding: 0; flex: none; }
  .avatar-btn:hover { filter: brightness(1.15); }
  .avatar-btn.on { box-shadow: 0 0 0 2px var(--accent); }
  .menu { position: fixed; z-index: 1000; min-width: 240px; background: var(--panel); border: 1px solid var(--line); border-radius: var(--r-l); box-shadow: 0 16px 48px #00000059; padding: 6px; }
  .menu .head { padding: 9px 12px 8px; }
  .menu .head b { display: block; font-size: 14px; }
  .menu .head .sub { display: block; font-size: 11.5px; color: var(--ink-3); margin-top: 1px; }
  .menu .sep { border-top: 1px solid var(--line-soft); margin: 6px 4px; }
  .menu a, .menu .row-btn { display: flex; align-items: center; gap: 12px; width: 100%; text-align: left; padding: 9px 12px; border-radius: var(--r-m); color: var(--ink); font-size: 14px; text-decoration: none; background: none; border: 0; cursor: pointer; font: inherit; }
  .menu a:hover, .menu .row-btn:hover, .menu a.current { background: var(--raised); }
  .menu .glyph { width: 28px; height: 28px; border-radius: 7px; flex: none; display: grid; place-items: center; background: var(--raised); border: 1px solid var(--line-soft); font-size: 14px; }
  .menu .glyph.av { border-radius: 50%; background: var(--accent-soft); color: var(--accent-text); font-family: var(--disp); font-weight: 700; font-size: 12px; }
  .menu .sub2 { display: block; font-size: 11px; color: var(--ink-3); }
  .menu .out { color: var(--g-err); }
</style>
