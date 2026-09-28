<script>
  import { base } from '$app/paths';
  // Create / edit a match: opponent & frame, starting six per set, libero.
  // Before scouting starts, lineup and first serve are the match's planning
  // (server). Once this device records the match, they are edits of its own
  // recording; another device's recording is never touched from here — the
  // form offers a copy instead.
  import { goto } from '$app/navigation';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { showToast, online, me, logDiag } from '$lib/stores.js';
  import { ROMAN, POS_NAME, todayIso } from '$lib/engine.js';
  import { fold, baseFromMatch } from '$lib/recording.js';
  import { myRecording, editsFrom, appendEdit, createRecording } from '$lib/recstore.js';
  import { kick, sync } from '$lib/uploader.js';
  import { tabWriter } from '$lib/scoutlock.js';

  let { match = null, players = [], set = 1 } = $props();

  // the editor is a writer too: it takes the same per-match writer lock as
  // the live page, so a second tab cannot append edits at the same time
  let tabOwner = $state(false);
  const mid = $derived(match?.id ?? null);
  $effect(() => {
    const m = mid;
    if (!m) { tabOwner = true; return; }
    const lock = tabWriter(m, (held) => (tabOwner = held));
    return () => lock.release();
  });

  // this device's recording of the match, if any
  let rec = $state(null);
  let edits = $state([]);
  let recLoaded = $state(false);
  $effect(() => {
    const m = mid;
    void $me?.user?.id;
    recLoaded = false;
    if (!m) { rec = null; edits = []; recLoaded = true; return; }
    untrack(async () => {
      try { const r = await myRecording(m, $me?.user?.id ?? null); rec = r; edits = r ? (await editsFrom(r.id, 1)).map((e) => e.body) : []; }
      catch (e) { logDiag('store', e?.message || e); }
      recLoaded = true;
    });
  });
  const snap = $derived(rec ? fold(rec.base, edits) : null);
  const hasRecordings = $derived((match?.recordings?.length || 0) > 0);
  // lineup and first serve belong to a recording once one exists
  const mode = $derived(!match ? 'new' : rec ? 'mine' : hasRecordings ? 'foreign' : 'planning');
  // my recording exists but the coach chose another one: a copy of the result is offered too
  const resultIsOther = $derived(mode === 'mine' && !!match?.selected && match.selected !== rec.id);

  let opponent = $state(match?.opponent || '');
  let date = $state(match?.date || todayIso());
  let time = $state(match?.time || '');
  let hall = $state(match?.hall || '');
  let home = $state(match?.home ?? true);
  let first_serve = $state('us');
  let notes = $state(match?.notes || '');
  let busy = $state(false);
  let error = $state('');

  const fieldPlayers = $derived(players.filter((p) => p.active && p.position !== 'L'));
  const liberos = $derived(players.filter((p) => p.active));
  // the lineups shown: the recording's own, else the planning / the result
  const lineups = $derived(snap ? snap.cfg.lineups : mode === 'planning' ? match?.planning?.lineups || match?.lineups || {} : match?.lineups || {});
  // a set without its own lineup inherits the closest lower one, exactly as the engine replays it
  const existing = $derived.by(() => {
    const l = lineups || {};
    const keys = Object.keys(l).map(Number);
    const lower = keys.filter((k) => k <= set).sort((a, b) => b - a);
    return lower.length ? l[lower[0]] : keys.length ? l[Math.min(...keys)] : null;
  });
  let pos = $state([0, 0, 0, 0, 0, 0]);
  let libero = $state(null);
  $effect(() => {
    // initialize from the match (or a sensible default: first six by number);
    // reads of pos/players are untracked so the write never re-triggers this
    const ex = existing;
    const fs = snap ? (snap.cfg.first_serve_us ? 'us' : 'them') : mode === 'planning' ? match?.planning?.first_serve || match?.first_serve || 'us' : match?.first_serve || 'us';
    untrack(() => {
      first_serve = fs;
      if (ex) { pos = ex.pos.slice(); libero = ex.libero ?? null; }
      else if (!pos.some(Boolean)) {
        pos = fieldPlayers.slice(0, 6).map((p) => p.id).concat([0, 0, 0, 0, 0, 0]).slice(0, 6);
        libero = players.find((p) => p.position === 'L' && p.active)?.id ?? null;
      }
    });
  });

  const byId = $derived(Object.fromEntries(players.map((p) => [p.id, p])));
  const check = $derived.by(() => {
    if (pos.some((p) => !p)) return { ok: false, text: 'Alle sechs Positionen besetzen.' };
    if (new Set(pos).size !== 6) return { ok: false, text: 'Eine Spielerin steht doppelt auf dem Feld.' };
    if (libero && pos.includes(libero)) return { ok: false, text: 'Die Libera kann nicht in der Startsechs stehen.' };
    const setters = pos.filter((p) => byId[p]?.position === 'Z').length;
    const mids = pos.map((p, i) => (byId[p]?.position === 'M' ? i : -1)).filter((i) => i >= 0);
    if (setters !== 1) return { ok: true, warn: true, text: `${setters === 0 ? 'Kein' : 'Mehr als ein'} Zuspiel in der Aufstellung.` };
    if (mids.length === 2 && Math.abs(mids[0] - mids[1]) !== 3) return { ok: true, warn: true, text: 'Die Mittelblockerinnen stehen nicht gegenüber, Libera-Anzeige wird ungenau.' };
    return { ok: true, text: 'Aufstellung ist plausibel: 1 Zuspiel, Mitten gegenüber.' };
  });
  const lineupChanged = $derived(!existing || existing.pos.join() !== pos.join() || (existing.libero ?? null) !== (libero || null) || !lineups?.[set]);
  const serveChanged = $derived(snap ? (snap.cfg.first_serve_us ? 'us' : 'them') !== first_serve : false);

  async function saveFrame() {
    if (!match) return;
    const body = { version: match.version, opponent, date, time, hall, home, notes };
    if (mode === 'planning') body.first_serve = first_serve;
    await api(`/matches/${match.id}`, { method: 'PATCH', body });
  }

  async function save(e) {
    e?.preventDefault();
    if (!check.ok) { error = check.text; return; }
    busy = true; error = '';
    const lineup = { pos: pos.slice(), libero: libero || null };
    try {
      if (!match) {
        const m = await api('/matches', { method: 'POST', body: { opponent, date, time, hall, home, first_serve, notes, lineup } });
        showToast('Spiel angelegt');
        goto(`${base}/live/${m.id}`);
        return;
      }
      if (!tabOwner) { error = 'Dieses Spiel wird in einem anderen Tab dieses Browsers gescoutet.'; return; }
      if (mode === 'planning') {
        await saveFrame();
        await api(`/matches/${match.id}/lineups/${set}`, { method: 'PUT', body: lineup });
      } else if (mode === 'mine') {
        if (!$sync.supported) { error = $sync.unsupportedReason; return; }
        if (lineupChanged) await appendEdit(rec.id, { op: 'lineup', set, lineup });
        if (serveChanged) await appendEdit(rec.id, { op: 'first_serve', us: first_serve === 'us' });
        kick();
        if ($online) { try { await saveFrame(); } catch (err) { if (err.status !== 409) throw err; error = 'Rahmendaten: jemand hat das Spiel zwischenzeitlich geändert. Aufstellung ist gespeichert.'; } }
      } else {
        // foreign recordings only: the frame is still the match's
        await saveFrame();
      }
      showToast('Gespeichert');
      goto(`${base}/live/${match.id}`);
    } catch (err) {
      error = err.status === 409 ? 'Jemand hat das Spiel zwischenzeitlich geändert. Seite neu laden.' : err.message;
    } finally {
      busy = false;
    }
  }

  /** a copy of the shown result on this device, with this lineup as its first edit */
  async function copyAndSave() {
    if (!check.ok) { error = check.text; return; }
    if (!$sync.supported) { error = $sync.unsupportedReason; return; }
    busy = true; error = '';
    try {
      const b = baseFromMatch(match);
      const sel = (match.recordings || []).find((r) => r.selected);
      const r = await createRecording({
        match_id: match.id, team_id: $me?.team?.id ?? null, user_id: $me?.user?.id ?? null, base: b,
        origin_id: match.selected || null, origin_n: sel?.n ?? null,
        firstEdit: { op: 'lineup', set, lineup: { pos: pos.slice(), libero: libero || null } }
      });
      if ((b.first_serve_us ? 'us' : 'them') !== first_serve) await appendEdit(r.id, { op: 'first_serve', us: first_serve === 'us' });
      kick();
      showToast('Eigene Aufzeichnung als Kopie begonnen');
      goto(`${base}/live/${match.id}`);
    } catch (err) {
      error = err.message;
    } finally {
      busy = false;
    }
  }
</script>

<form class="grid2" onsubmit={save}>
  <div>
    <section class="panel">
      <div class="panel-head"><h2>Gegner &amp; Rahmen</h2></div>
      <div class="row">
        <label class="f" style="flex:2">Gegner<input type="text" bind:value={opponent} placeholder="z. B. VfL Bad Vilbel" required /></label>
        <label class="f">Datum<input type="date" bind:value={date} required /></label>
        <label class="f">Anpfiff<input type="time" bind:value={time} /></label>
      </div>
      <div class="row" style="margin-top:10px">
        <label class="f">Halle<input type="text" bind:value={hall} /></label>
        <div class="f"><span>Heim / Auswärts</span><div class="seg"><button type="button" class:on={home} onclick={() => (home = true)}>Heim</button><button type="button" class:on={!home} onclick={() => (home = false)}>Auswärts</button></div></div>
        <div class="f"><span>Erster Aufschlag</span><div class="seg"><button type="button" class:on={first_serve === 'us'} onclick={() => (first_serve = 'us')}>Wir</button><button type="button" class:on={first_serve === 'them'} onclick={() => (first_serve = 'them')}>Gegner</button></div></div>
      </div>
      <label class="f" style="margin-top:10px">Notizen<input type="text" bind:value={notes} placeholder="optional" /></label>
    </section>
  </div>
  <div>
    <section class="panel">
      <div class="panel-head"><h2>Startaufstellung Satz {set}</h2><span class="small muted">Position I schlägt auf</span></div>
      {#if fieldPlayers.length < 6}
        <p class="err">Mindestens sechs Feldspielerinnen im Kader nötig. <a href="{base}/kader">Zum Kader</a></p>
      {/if}
      <div class="court">
        {#each [[3, 2, 1], [4, 5, 0]] as row}
          <div class="rowp">
            {#each row as i}
              <div class="lslot">
                <span class="posn">{ROMAN[i]}{i === 0 ? ' · Aufschlag' : ''}</span>
                <select bind:value={pos[i]}>
                  <option value={0}>–</option>
                  {#each fieldPlayers as p (p.id)}<option value={p.id}>{p.number} {p.name} ({p.position})</option>{/each}
                </select>
              </div>
            {/each}
          </div>
        {/each}
      </div>
      <div class="row" style="margin-top:12px; align-items:end">
        <label class="f">Libera<select bind:value={libero}><option value={null}>keine</option>{#each liberos as p (p.id)}<option value={p.id}>{p.number} {p.name} ({POS_NAME[p.position]})</option>{/each}</select></label>
        <div style="flex:2; align-self:center" class:warn={check.warn} class:ok={check.ok && !check.warn} class:bad={!check.ok}>{check.text}</div>
      </div>
      <p class="small muted" style="margin:10px 0 0">Die Libera wird im Live-Feld automatisch für die Mittelblockerin in der Hinterzone (V, VI) angezeigt. Sätze ohne eigene Aufstellung übernehmen die vorige.</p>
    </section>
    {#if match && !tabOwner}
      <div class="scoutbar" role="status"><span class="txt">Dieses Spiel wird in einem anderen Tab dieses Browsers gescoutet. Aufstellung und Aufschlag lassen sich nur dort ändern.</span></div>
    {:else if mode === 'mine'}
      <p class="small muted" style="margin-top:10px">Aufstellung und Aufschlag gehören zur Aufzeichnung dieses Geräts und werden mit ihr hochgeladen.{#if resultIsOther} Als Ergebnis zählt derzeit eine andere Aufzeichnung; „Ergebnis als Kopie fortsetzen“ übernimmt deren Stand in eine neue eigene Aufzeichnung.{/if}</p>
    {:else if mode === 'foreign' && recLoaded}
      <div class="scoutbar" role="status">
        <span class="txt">Dieses Spiel wird auf einem anderen Gerät aufgezeichnet. Aufstellung und Aufschlag gehören zu dieser Aufzeichnung. „Rahmen speichern“ ändert nur Gegner, Datum und Notizen; eine geänderte Aufstellung setzt den angezeigten Stand als eigene Aufzeichnung (Kopie) fort.</span>
      </div>
    {/if}
    {#if error}<p class="err" style="margin-top:10px">{error}</p>{/if}
    <div class="row" style="margin-top:14px">
      <button class="btn primary big" type="submit" disabled={busy || fieldPlayers.length < 6 || (match && !tabOwner)}>{match ? (mode === 'foreign' ? 'Rahmen speichern' : 'Speichern & zum Live-Scouting') : 'Spiel anlegen & starten'}</button>
      {#if mode === 'foreign' && recLoaded}<button class="btn big" type="button" disabled={busy || !tabOwner} onclick={copyAndSave}>Als Kopie fortsetzen</button>
      {:else if resultIsOther}<button class="btn big" type="button" disabled={busy || !tabOwner} onclick={copyAndSave}>Ergebnis als Kopie fortsetzen</button>{/if}
      <a class="btn big" href={match ? `${base}/live/${match.id}` : '/spiele'}>Abbrechen</a>
    </div>
  </div>
</form>

<style>
  .court { position: relative; background: var(--court-soft); border: 2px solid var(--court-line); border-top: 6px solid var(--ink-2); border-radius: 4px; padding: 8px; display: grid; grid-template-rows: 1fr 1fr; gap: 8px; margin-top: 18px; }
  .court::before { content: 'Netz'; position: absolute; top: -20px; left: 50%; transform: translateX(-50%); font-size: 11px; color: var(--ink-3); }
  .rowp { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .lslot { background: var(--panel); border-radius: var(--r-m); padding: 6px; display: grid; gap: 4px; }
  .lslot .posn { font-size: 10px; color: var(--ink-3); }
  .lslot select { height: 36px; font-size: 13px; padding: 0 4px; }
  .scoutbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-top: 12px; padding: 8px 12px; border: 1px solid var(--accent); background: var(--accent-soft); border-radius: var(--r-m); font-size: 13px; }
  .scoutbar .txt { flex: 1 1 200px; min-width: 0; overflow-wrap: anywhere; line-height: 1.3; }
  @media (max-width: 760px) { .lslot select { height: 40px; font-size: 16px; padding: 0 2px; } }
  .warn { color: var(--g-neg); font-size: 13px; }
  .ok { color: var(--g-win); font-size: 13px; }
  .bad { color: var(--g-err); font-size: 13px; }
  .f > span { font-size: 12px; color: var(--ink-2); display: block; margin-bottom: 4px; }
</style>
