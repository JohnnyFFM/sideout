<script>
  import { page } from '$app/stores';
  import { me, sseConnected, online } from '$lib/stores.js';

  let theme = $state(typeof window !== 'undefined' ? window.soTheme.get() : 'dark');
  const NEXT_ICON = { dark: '☀', light: '☾' };
  $effect(() => {
    const h = (e) => (theme = e.detail);
    window.addEventListener('so-theme', h);
    return () => window.removeEventListener('so-theme', h);
  });

  const path = $derived($page.url.pathname);
  const active = (p) => (path === p || path.startsWith(p + '/') ? 'active' : '');
  const initials = $derived(($me?.user?.display_name || '??').slice(0, 2).toUpperCase());
  // "Live" jumps to the running match if there is one, else to the match list
  const liveHref = $derived($me?.live_match_id ? `/live/${$me.live_match_id}` : '/spiele');
</script>

<header class="topbar">
  <a class="brand" href="/team"><span class="tick">▲</span>SIDEOUT</a>
  <nav class="nav">
    <a class={active('/team')} href="/team">Team</a>
    <a class={active('/spiele') || active('/live')} href="/spiele">Spiele</a>
    <a class={active('/auswertung')} href="/auswertung">Auswertung</a>
    <a class={active('/kader')} href="/kader">Kader</a>
  </nav>
  <span class="spacer"></span>
  <span class="sync-chip" title={$sseConnected ? 'Live verbunden' : 'Verbindung getrennt'}>
    <span class="dot" style:background={$online && $sseConnected ? 'var(--ok)' : 'var(--g-neg)'}></span>
    <span class="desk">{$me?.team?.name || ''}</span>
  </span>
  <button class="icon-btn" onclick={() => window.soTheme.cycle()} title="Theme wechseln">{NEXT_ICON[theme]}</button>
  <a class="icon-btn" href="/einstellungen" title={$me?.user?.display_name || 'Einstellungen'}>{initials}</a>
</header>

<nav class="tabbar">
  <a class={active('/team')} href="/team"><span class="ico">⌂</span>Team</a>
  <a class={active('/spiele') || active('/live')} href="/spiele"><span class="ico">●</span>Spiele</a>
  <a class={active('/auswertung')} href="/auswertung"><span class="ico">≡</span>Auswertung</a>
  <a class={active('/kader')} href="/kader"><span class="ico">☷</span>Kader</a>
</nav>

<style>
  @media (max-width: 760px) {
    .desk { display: none; }
  }
</style>
