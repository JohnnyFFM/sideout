<script>
  import '../app.css';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import Nav from '$lib/components/Nav.svelte';
  import { updated } from '$app/state';
  import { me, toast, online, connectSSE, disconnectSSE } from '$lib/stores.js';

  let { data, children } = $props();

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
      if ($page.url.pathname !== '/login') goto('/login');
    }
  });
</script>

{#if data.me}
  <Nav />
  {#if !$online}
    <div class="offline-bar">Offline — Aktionen werden gespeichert und später gesendet</div>
  {/if}
{/if}
{@render children()}

{#if updated.current}
  <button class="update-bar" onclick={() => location.reload()}>Neue Version verfügbar – neu laden</button>
{/if}

{#if $toast}
  <div class="toast show" class:err={$toast.isErr}>{$toast.text}</div>
{/if}
