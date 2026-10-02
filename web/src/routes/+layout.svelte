<script>
  import '../app.css';
  import { goto } from '$app/navigation';
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import Nav from '$lib/components/Nav.svelte';
  import { updated } from '$app/state';
  import { me, toast, online, connectSSE, disconnectSSE, showToast } from '$lib/stores.js';
  import { startUploader, kick, sync } from '$lib/uploader.js';
  import { importLegacy } from '$lib/legacy.js';

  let { data, children } = $props();

  $effect(() => {
    me.set(data.me);
  });

  // one live stream per tab for the whole session; no cleanup on
  // navigation, so the stream is not torn down and reopened on every page.
  // The uploader starts with the identity (also offline: pending counts
  // show) and is nudged on every identity refresh (a login).
  $effect(() => {
    if (data.me) {
      startUploader();
      kick();
      if (!data.me.offline) {
        connectSSE();
        importLegacy(data.me.user?.id ?? null).then((n) => { if (n) showToast(`${n} Aktionen aus der alten App-Version übernommen`); }).catch(() => {});
      }
    } else {
      disconnectSSE();
      if ($page.url.pathname !== `${base}/login`) goto(`${base}/login`);
    }
  });
</script>

{#if data.me}
  <Nav />
  {#if !$online}
    <div class="offline-bar">Offline — Aktionen werden auf dem Gerät gespeichert und später hochgeladen{#if $sync.total} · {$sync.total} ausstehend{/if}</div>
  {:else if $sync.transient && $sync.total}
    <div class="offline-bar">{$sync.total} Änderungen noch nicht hochgeladen ({$sync.transient}), neuer Versuch folgt</div>
  {/if}
{/if}
{@render children()}

{#if updated.current}
  <button class="update-bar" onclick={() => location.reload()}>Neue Version verfügbar – neu laden</button>
{/if}

{#if $toast}
  <div class="toast show" class:err={$toast.isErr}>{$toast.text}</div>
{/if}
