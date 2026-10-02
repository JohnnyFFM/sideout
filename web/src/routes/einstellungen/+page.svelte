<script>
  import { base } from '$app/paths';
  // Einstellungen: this device and this app — theme, install state,
  // connection, the recordings stored on this device (upload state,
  // export, import), guide, diagnostics. Account and teams have their own pages.
  import { version } from '$app/environment';
  import { untrack } from 'svelte';
  import { showToast, sseConnected, sseRole, online, readDiag, DIAG_KEY, me } from '$lib/stores.js';
  import { sync, kick } from '$lib/uploader.js';
  import { allRecordings, exportRecording, importRecording, pendingOf, onChange, deviceId } from '$lib/recstore.js';
  import { cachedMatch } from '$lib/offline.js';
  import { downloadJson } from '$lib/people.js';
  import { fmtDate } from '$lib/engine.js';
  import { isHere } from '$lib/recording.js';

  let theme = $state(typeof window !== 'undefined' ? window.soTheme.get() : 'dark');
  $effect(() => {
    const h = (e) => (theme = e.detail);
    window.addEventListener('so-theme', h);
    return () => window.removeEventListener('so-theme', h);
  });
  const setTheme = (v) => window.soTheme.set(v);

  const installed = typeof window !== 'undefined' && (window.matchMedia?.('(display-mode: standalone)').matches || navigator.standalone === true);
  const isIos = typeof navigator !== 'undefined' && /iphone|ipad/i.test(navigator.userAgent);
  const installHint = isIos ? 'Safari: Teilen → „Zum Home-Bildschirm“.' : 'Browser-Menü → „App installieren“ oder „Zum Startbildschirm“.';

  const streamText = $derived(
    !$online ? 'Offline · Änderungen werden auf dem Gerät gespeichert und später hochgeladen'
      : $sseConnected ? `Live-Stream verbunden · ${$sseRole === 'leader' ? 'dieser Tab hält den Stream' : 'ein anderer Tab hält den Stream'}`
      : 'Live-Stream getrennt · verbindet neu'
  );

  // ----- recordings on this device -----
  let recs = $state([]);
  let myDevice = $state('');
  async function loadRecs() {
    try {
      const all = await allRecordings();
      recs = all.sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1)).map((r) => {
        const m = cachedMatch(r.match_id);
        return { ...r, pending: pendingOf(r) || (r.created ? 0 : 1), title: m ? `${m.opponent} · ${fmtDate(m.date)}` : `Spiel ${r.match_id}`, edits: r.next_n - 1 };
      });
    } catch { recs = []; }
  }
  $effect(() => { untrack(() => { loadRecs(); deviceId().then((d) => (myDevice = d)).catch(() => {}); }); return onChange(() => untrack(loadRecs)); });
  const legacyKeys = typeof localStorage === 'undefined' ? 0 : Object.keys(localStorage).filter((k) => /^so_ops_(dropped_)?\d+$/.test(k)).length;
  async function exportRec(r) {
    const data = await exportRecording(r.id);
    if (data) downloadJson(`sideout-aufzeichnung-${r.match_id}-${r.id.slice(0, 8)}.json`, data);
  }
  let importMsg = $state('');
  async function importFile(e) {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    try {
      const data = JSON.parse(await file.text());
      if (data?.format !== 'sideout-recording' || !data.recording?.id || !data.recording.base) throw new Error('Keine Sideout-Aufzeichnung');
      const r = await importRecording(data.recording, data.edits || []);
      importMsg = r.existed ? 'Diese Aufzeichnung ist auf dem Gerät schon vorhanden.' : `Aufzeichnung ${r.id.slice(0, 8)} übernommen, wird hochgeladen.`;
      kick();
    } catch (err) {
      importMsg = 'Import fehlgeschlagen: ' + (err?.message || err);
    }
  }
  const syncText = $derived((!$sync.supported ? $sync.unsupportedReason : $sync.total ? `${$sync.total} Änderungen noch nicht hochgeladen${$sync.transient ? ' · ' + $sync.transient : ''}` : 'Alles auf dem Server') + ($sync.foreign ? ` · ${$sync.foreign} Aufzeichnung${$sync.foreign === 1 ? '' : 'en'} eines anderen Kontos wartet auf dessen Anmeldung` : ''));

  let diag = $state([]);
  $effect(() => { diag = readDiag(); });
  function captureDiag() {
    return JSON.stringify({ version, ua: navigator.userAgent, online: $online, stream: $sseConnected, role: $sseRole, path: location.pathname, installed, device: myDevice, pending: $sync.total, recordings: recs.map((r) => ({ id: r.id, match: r.match_id, edits: r.edits, confirmed: r.confirmed_n, created: r.created, error: r.error, imported: r.imported })), legacyKeys, errors: readDiag() }, null, 1);
  }
  async function copyDiag() {
    try { await navigator.clipboard.writeText(captureDiag()); showToast('Diagnose kopiert'); }
    catch { showToast('Kopieren nicht möglich. Bitte „Datei herunterladen“ nutzen.', true); }
  }
  function downloadDiag() {
    downloadJson('sideout-diagnose.json', JSON.parse(captureDiag()));
  }
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
      <div><div class="k">Als App installieren</div><div class="d">{installHint} Danach Vollbild, auch ohne Empfang in der Halle.{#if isIos} Wichtig: Safari löscht den Speicher einer Website nach sieben Tagen Safari-Nutzung ohne Besuch; die installierte App ist davon ausgenommen.{/if}</div></div>
      <span class="chip" class:ok={installed}>{installed ? 'installiert' : 'nicht installiert'}</span>
    </div>
    <div class="app-row">
      <div><div class="k">Verbindung</div><div class="d">{streamText}</div></div>
      <span class="status"><span class="dot" class:bad={!$online || !$sseConnected}></span><span class="small">{$online ? 'online' : 'offline'}</span></span>
    </div>
    <div class="app-row">
      <div><div class="k">Upload</div><div class="d">{syncText}</div></div>
      <button class="btn" onclick={() => kick({ force: true })} disabled={!$sync.supported || !$sync.total || $sync.uploading}>{$sync.uploading ? 'lädt…' : 'Jetzt hochladen'}</button>
    </div>
    <div class="app-row">
      <div><div class="k">Anleitung</div><div class="d">Scouten, Bewertungsskala, Auswertung</div></div>
      <a class="btn" href="{base}/anleitung">Öffnen</a>
    </div>
    <details class="diag app-row">
      <summary>
        <div><div class="k">Aufzeichnungen auf diesem Gerät <span class="caret">›</span></div><div class="d">Jede Aufzeichnung bleibt hier, auch nach dem Upload · Export als Datei, Import auf einem anderen Gerät</div></div>
        <span class="chip" class:warn={recs.some((r) => r.pending || r.error)}>{recs.length}</span>
      </summary>
      <div class="diag-body">
        {#if recs.length}
          <ul class="reclist">
            {#each recs as r (r.id)}
              <li>
                <div class="rt"><b>{r.title}</b>{#if isHere(r, myDevice)}<span title="auf diesem Gerät">📱</span>{/if} <span class="muted">{r.imported ? 'importiert' : r.user_id != null && r.user_id !== $me?.user?.id ? 'anderes Konto' : ''}{r.imported || (r.user_id != null && r.user_id !== $me?.user?.id) ? ' · ' : ''}{r.edits} Änderungen</span></div>
                <div class="rs" class:bad={!!r.error} class:pend={!r.error && r.pending && !r.deleted}>{r.deleted ? 'gelöscht (Daten bleiben)' : r.error ? 'Fehler: ' + r.error : r.pending ? `${r.pending} nicht hochgeladen` : 'auf Server gespeichert'}</div>
                <div class="ra"><a class="small" href="{base}/auswertung/{r.match_id}">Auswertung</a><button class="btn" onclick={() => exportRec(r)}>Exportieren</button></div>
              </li>
            {/each}
          </ul>
        {:else}<p class="small muted" style="margin:0 0 10px">Noch keine Aufzeichnung auf diesem Gerät.</p>{/if}
        <div class="row" style="flex-wrap:wrap; align-items:center; gap:8px">
          <label class="btn">Aufzeichnung importieren<input type="file" accept="application/json,.json" style="display:none" onchange={importFile} /></label>
          {#if importMsg}<span class="small importmsg">{importMsg}</span>{/if}
        </div>
        {#if legacyKeys}<p class="small muted" style="margin:10px 0 0">{legacyKeys} Rohdaten-Eintrag{legacyKeys === 1 ? '' : 'e'} der alten App-Version liegen noch im Browser-Speicher (werden nicht gelöscht).</p>{/if}
      </div>
    </details>
    <details class="diag app-row">
      <summary>
        <div><div class="k">Diagnose <span class="caret">›</span></div><div class="d">Version {version} · bei Problemen kopieren und schicken</div></div>
        <span class="chip" class:warn={diag.length}>{diag.length ? `${diag.length} ${diag.length === 1 ? 'Eintrag' : 'Einträge'}` : 'keine Fehler'}</span>
      </summary>
      <div class="diag-body">
        {#if diag.length}
          <ul>{#each diag as e}<li><span class="muted">{e.t.slice(11, 19)}</span> <b>{e.kind}</b> {e.msg} <span class="muted">{e.path}</span></li>{/each}</ul>
        {:else}<p class="small muted" style="margin:0 0 10px">Keine Fehler aufgezeichnet.</p>{/if}
        <div class="row" style="flex-wrap:wrap"><button class="btn" onclick={downloadDiag}>Datei herunterladen</button><button class="btn" onclick={copyDiag}>Diagnose kopieren</button>{#if diag.length}<button class="btn ghost" onclick={clearDiag}>Leeren</button>{/if}</div>
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
  .diag ul { list-style: none; margin: 0 0 10px; padding: 0; font-size: 12px; }
  .diag li { padding: 4px 0; border-bottom: 1px solid var(--line-soft); word-break: break-word; }
  .reclist li { display: grid; grid-template-columns: 1fr auto; gap: 2px 10px; align-items: center; padding: 6px 0; font-size: 13px; }
  .reclist .rt { grid-column: 1; }
  .reclist .rs { grid-column: 1; font-size: 12px; color: var(--ink-3); }
  .reclist .rs.pend { color: var(--g-neg); }
  .reclist .rs.bad { color: var(--g-err); }
  .reclist .ra { grid-column: 2; grid-row: 1 / span 2; display: flex; gap: 8px; align-items: center; }
  .status { display: inline-flex; align-items: center; gap: 6px; }
  .status .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 3px #2EB87233; }
  .status .dot.bad { background: var(--g-neg); box-shadow: 0 0 0 3px #D39A2E33; }
  .foot { font-size: 12px; color: var(--ink-3); margin-top: 10px; }
</style>
