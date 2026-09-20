<script>
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { me, sseConnected, online, switchTeam, showToast } from '$lib/stores.js';
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
  const initials = $derived(($me?.user?.display_name || '??').slice(0, 2).toUpperCase());
  // "Live" jumps to the running match if there is one, else to the match list
  const liveHref = $derived($me?.live_match_id ? `${base}/live/${$me.live_match_id}` : '/spiele');
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
  <a class="icon-btn" href="{base}/einstellungen" title={$me?.user?.display_name || 'Einstellungen'}>{initials}</a>
</header>

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
</style>
