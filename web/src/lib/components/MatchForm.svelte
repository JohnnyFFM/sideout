<script>
  // Create / edit a match: opponent & frame, starting six per set, libero.
  import { goto } from '$app/navigation';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { showToast } from '$lib/stores.js';
  import { ROMAN, POS_NAME, todayIso } from '$lib/engine.js';

  let { match = null, players = [], set = 1 } = $props();

  let opponent = $state(match?.opponent || '');
  let date = $state(match?.date || todayIso());
  let time = $state(match?.time || '');
  let hall = $state(match?.hall || '');
  let home = $state(match?.home ?? true);
  let first_serve = $state(match?.first_serve || 'us');
  let notes = $state(match?.notes || '');
  let busy = $state(false);
  let error = $state('');

  const fieldPlayers = $derived(players.filter((p) => p.active && p.position !== 'L'));
  const liberos = $derived(players.filter((p) => p.active));
  const existing = $derived(match?.lineups?.[set] || match?.lineups?.[1] || null);
  let pos = $state([0, 0, 0, 0, 0, 0]);
  let libero = $state(null);
  $effect(() => {
    // initialize from the match (or a sensible default: first six by number);
    // reads of pos/players are untracked so the write never re-triggers this
    const ex = existing;
    untrack(() => {
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

  async function save(e) {
    e?.preventDefault();
    if (!check.ok) { error = check.text; return; }
    busy = true; error = '';
    const lineup = { pos: pos.slice(), libero: libero || null };
    try {
      if (match) {
        await api(`/matches/${match.id}`, { method: 'PATCH', body: { version: match.version, opponent, date, time, hall, home, first_serve, notes } });
        await api(`/matches/${match.id}/lineups/${set}`, { method: 'PUT', body: lineup });
        showToast('Gespeichert');
        goto(`/live/${match.id}`);
      } else {
        const m = await api('/matches', { method: 'POST', body: { opponent, date, time, hall, home, first_serve, notes, lineup } });
        showToast('Spiel angelegt');
        goto(`/live/${m.id}`);
      }
    } catch (err) {
      error = err.status === 409 ? 'Jemand hat das Spiel zwischenzeitlich geändert. Seite neu laden.' : err.message;
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
        <p class="err">Mindestens sechs Feldspielerinnen im Kader nötig. <a href="/kader">Zum Kader</a></p>
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
    {#if error}<p class="err" style="margin-top:10px">{error}</p>{/if}
    <div class="row" style="margin-top:14px">
      <button class="btn primary big" type="submit" disabled={busy || fieldPlayers.length < 6}>{match ? 'Speichern & zum Live-Scouting' : 'Spiel anlegen & starten'}</button>
      <a class="btn big" href={match ? `/live/${match.id}` : '/spiele'}>Abbrechen</a>
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
  .warn { color: var(--g-neg); font-size: 13px; }
  .ok { color: var(--g-win); font-size: 13px; }
  .bad { color: var(--g-err); font-size: 13px; }
  .f > span { font-size: 12px; color: var(--ink-2); display: block; margin-bottom: 4px; }
</style>
