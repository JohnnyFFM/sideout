<script>
  import '../app.css';
  import { goto, afterNavigate } from '$app/navigation';
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import Nav from '$lib/components/Nav.svelte';
  import { updated } from '$app/state';
  import { me, toast, online, connectSSE, disconnectSSE } from '$lib/stores.js';

  let { data, children } = $props();

  // the app is a screen-sized shell: top bar, a scrolling content area and
  // the tab bar as ordinary elements inside a non-fixed shell, avoiding
  // iOS fixed-position viewport anchoring for the persistent navigation.
  // The document itself never scrolls, so the
  // content area resets its own scroll on navigation (not for hash jumps).
  let scrollEl = $state(null);
  afterNavigate((nav) => { if (scrollEl && !nav.to?.url.hash) scrollEl.scrollTop = 0; });

  $effect(() => {
    me.set(data.me);
  });

  // one live stream per tab for the whole session; no cleanup on
  // navigation, so the stream is not torn down and reopened on every page
  $effect(() => {
    if (data.me && !data.me.offline) {
      connectSSE();
    } else if (!data.me) {
      disconnectSSE();
      if ($page.url.pathname !== `${base}/login`) goto(`${base}/login`);
    }
  });
</script>

<div class="shell">
  {#if data.me}
    <Nav />
    {#if !$online}
      <div class="offline-bar">Offline — Aktionen werden gespeichert und später gesendet</div>
    {/if}
  {/if}
  <div class="shell-main" bind:this={scrollEl}>
    {@render children()}
  </div>

  <!-- overlays are positioned within the shell, not the viewport, so they follow its height -->
  {#if updated.current}
    <button class="update-bar" onclick={() => location.reload()}>Neue Version verfügbar – neu laden</button>
  {/if}
  {#if $toast}
    <div class="toast show" class:err={$toast.isErr}>{$toast.text}</div>
  {/if}
</div>
