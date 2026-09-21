<script>
  import { base } from '$app/paths';
  // Einstellungen: this device and this app — theme, install state,
  // connection, guide, diagnostics. Account and teams have their own pages.
  import { version } from '$app/environment';
  import { showToast, sseConnected, sseRole, online, readDiag, DIAG_KEY } from '$lib/stores.js';

  let theme = $state(typeof window !== 'undefined' ? window.soTheme.get() : 'dark');
  $effect(() => {
    const h = (e) => (theme = e.detail);
    window.addEventListener('so-theme', h);
    return () => window.removeEventListener('so-theme', h);
  });
  const setTheme = (v) => window.soTheme.set(v);

  const installed = typeof window !== 'undefined' && (window.matchMedia?.('(display-mode: standalone)').matches || navigator.standalone === true);
  const installHint = /iphone|ipad/i.test(navigator.userAgent) ? 'Safari: Teilen → „Zum Home-Bildschirm“.' : 'Browser-Menü → „App installieren“ oder „Zum Startbildschirm“.';

  const streamText = $derived(
    !$online ? 'Offline · Aktionen werden gespeichert und später gesendet'
      : $sseConnected ? `Live-Stream verbunden · ${$sseRole === 'leader' ? 'dieser Tab hält den Stream' : 'ein anderer Tab hält den Stream'}`
      : 'Live-Stream getrennt · verbindet neu'
  );

  let diag = $state([]);
  $effect(() => { diag = readDiag(); });
  // screen geometry (the iOS installed app has laid out the tab bar off the screen's edge)
  let geo = $state('');
  let probe = $state(null);
  function measure() {
    const bar = document.querySelector('.tabbar')?.getBoundingClientRect();
    const shell = document.querySelector('.shell')?.getBoundingClientRect();
    const cs = probe ? getComputedStyle(probe) : null;
    geo = `Bildschirm ${screen.width}×${screen.height} · Fenster ${innerWidth}×${innerHeight} · sichtbar ${Math.round(visualViewport?.height ?? innerHeight)} · dvh ${probe && cs ? probe.offsetHeight - parseInt(cs.paddingTop) - parseInt(cs.paddingBottom) : '?'}`
      + ` · Ränder ${cs ? parseInt(cs.paddingTop) : '?'}/${cs ? parseInt(cs.paddingBottom) : '?'} · Shell ${shell ? Math.round(shell.height) : '–'}`
      + ` · Leiste ${bar ? `${Math.round(bar.top)}–${Math.round(bar.bottom)}` : '–'}`;
  }
  $effect(() => { measure(); addEventListener('resize', measure); return () => removeEventListener('resize', measure); });
  const diagText = $derived(JSON.stringify({ version, ua: navigator.userAgent, online: $online, stream: $sseConnected, role: $sseRole, path: location.pathname, installed, geo, errors: diag }, null, 1));
  async function copyDiag() { try { await navigator.clipboard.writeText(diagText); showToast('Diagnose kopiert'); } catch { showToast('Kopieren nicht möglich, Text markieren', true); } }
  function clearDiag() { try { localStorage.removeItem(DIAG_KEY); } catch { /* ignore */ } diag = []; }
</script>

<svelte:head><title>Sideout — Einstellungen</title></svelte:head>
<main class="page narrow">
  <div class="toolbar"><div><h1>Einstellungen</h1><div class="tsub">dieses Gerät, diese App</div></div></div>
  <section class="panel">
    <div class="app-row">
      <div><div class="k">Darstellung</div><div class="d">Dunkel für die Halle, hell fürs Büro</div></div>
      <div class="seg" role="group" aria-label="Darstellung">
        <button class:active={theme === 'dark'} onclick={() => setTheme('dark')}>Dunkel</button>
        <button class:active={theme === 'light'} onclick={() => setTheme('light')}>Hell</button>
      </div>
    </div>
    <div class="app-row">
      <div><div class="k">Als App installieren</div><div class="d">{installHint} Danach Vollbild, auch ohne Empfang in der Halle.</div></div>
      <span class="chip" class:ok={installed}>{installed ? 'installiert' : 'nicht installiert'}</span>
    </div>
    <div class="app-row">
      <div><div class="k">Verbindung</div><div class="d">{streamText}</div></div>
      <span class="status"><span class="dot" class:bad={!$online || !$sseConnected}></span><span class="small">{$online ? 'online' : 'offline'}</span></span>
    </div>
    <div class="app-row">
      <div><div class="k">Anleitung</div><div class="d">Scouten, Bewertungsskala, Auswertung</div></div>
      <a class="btn" href="{base}/anleitung">Öffnen</a>
    </div>
    <details class="diag app-row">
      <summary>
        <div><div class="k">Diagnose <span class="caret">›</span></div><div class="d">Version {version} · bei Problemen kopieren und schicken</div></div>
        <span class="chip" class:warn={diag.length}>{diag.length ? `${diag.length} ${diag.length === 1 ? 'Fehler' : 'Fehler'}` : 'keine Fehler'}</span>
      </summary>
      <div class="diag-body">
        {#if diag.length}
          <ul>{#each diag as e}<li><span class="muted">{e.t.slice(11, 19)}</span> <b>{e.kind}</b> {e.msg} <span class="muted">{e.path}</span></li>{/each}</ul>
        {:else}<p class="small muted" style="margin:0 0 10px">Keine Fehler aufgezeichnet.</p>{/if}
        <p class="small muted geo">{geo}</p><div class="probe" bind:this={probe}></div>
        <div class="row"><button class="btn" onclick={copyDiag}>Diagnose kopieren</button>{#if diag.length}<button class="btn ghost" onclick={clearDiag}>Leeren</button>{/if}</div>
      </div>
    </details>
  </section>
  <p class="foot">Sideout · Version {version} · Open Source (MIT)</p>
</main>

<style>
  .page.narrow { max-width: 640px; }
  .toolbar .tsub { font-size: 12px; color: var(--ink-3); }
  .seg { display: inline-grid; grid-auto-flow: column; background: var(--raised); padding: 3px; border-radius: 10px; }
  .seg button { height: 32px; padding: 0 14px; border: 0; background: transparent; border-radius: 8px; color: var(--ink-2); font-weight: 600; cursor: pointer; font: inherit; font-weight: 600; }
  .seg button.active { background: var(--panel); color: var(--ink); box-shadow: 0 1px 2px #0006; }
  :global(:root[data-theme='light']) .seg button.active { box-shadow: 0 1px 2px #161B2622; }
  .app-row { display: grid; grid-template-columns: 1fr auto; gap: 12px; align-items: center; padding: 12px 0; border-bottom: 1px solid var(--line-soft); }
  .app-row:first-child { padding-top: 0; }
  .app-row:last-child { border-bottom: 0; padding-bottom: 0; }
  .app-row .k { font-weight: 600; font-size: 14px; }
  .app-row .d { font-size: 12px; color: var(--ink-3); margin-top: 2px; }
  .chip.ok { border-color: var(--ok); color: var(--ok); }
  .chip.warn { border-color: var(--g-neg); color: var(--g-neg); }
  details.diag { display: block; }
  details.diag summary { cursor: pointer; list-style: none; display: grid; grid-template-columns: 1fr auto; gap: 12px; align-items: center; }
  details.diag summary::-webkit-details-marker { display: none; }
  details.diag .caret { display: inline-block; color: var(--ink-3); font-size: 12px; transition: transform .15s; }
  details.diag[open] .caret { transform: rotate(90deg); }
  .diag-body { margin-top: 10px; }
  .geo { margin: 0 0 10px; word-break: break-word; }
  /* measures env() insets and 100dvh */
  .probe { position: absolute; left: 0; top: 0; width: 1px; height: 100dvh; padding-top: env(safe-area-inset-top); padding-bottom: env(safe-area-inset-bottom); box-sizing: content-box; visibility: hidden; pointer-events: none; }
  .diag ul { list-style: none; margin: 0 0 10px; padding: 0; font-size: 12px; }
  .diag li { padding: 4px 0; border-bottom: 1px solid var(--line-soft); word-break: break-word; }
  .status { display: inline-flex; align-items: center; gap: 6px; }
  .status .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 3px #2EB87233; }
  .status .dot.bad { background: var(--g-neg); box-shadow: 0 0 0 3px #D39A2E33; }
  .foot { font-size: 12px; color: var(--ink-3); margin-top: 10px; }
</style>
