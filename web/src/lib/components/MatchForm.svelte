<script>
  import { base } from '$app/paths';
  // Create / edit a match: opponent & frame, starting six per set, libero.
  import { goto } from '$app/navigation';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { showToast, online } from '$lib/stores.js';
  import { ROMAN, POS_NAME, todayIso } from '$lib/engine.js';
  import { loadOps, saveLease, newCid } from '$lib/offline.js';
  import { initialsOf } from '$lib/people.js';
  import { tabWriter } from '$lib/scoutlock.js';

  let { match = null, players = [], set = 1, onscout = null } = $props();

  // the editor is a writer too: it takes the same per-match writer lock as
  // the live page, so a second tab cannot change lineup or first serve
  // while another tab of this browser is scouting
  let tabOwner = $state(false);
  $effect(() => {
    const mid = match?.id;
    if (!mid) { tabOwner = true; return; }
    const lock = tabWriter(mid, (held) => (tabOwner = held));
    return () => lock.release();
  });

  // ----- scouting ownership: editing a lineup or the first serve of an
  // existing match is a protected write. The form never takes the match
  // from another device on its own: a current holder is shown with an
  // explicit "Scouting übernehmen"; a free or stale match is claimed
  // conditionally when saving. -----
  const scout = $derived(match?.scout || null);
  const holderElse = $derived(!!scout?.held && !scout.stale && !scout.mine);
  const pendingOps = $derived(match ? loadOps(match.id).length : 0);
  let acquiring = $state(false);
  const hhmm = (iso) => { const d = iso ? new Date(iso) : null; return d && !isNaN(d) ? d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' }) : '–'; };
  async function acquire(mode) {
    acquiring = true;
    try {
      const m = await api(`/matches/${match.id}/scout`, { method: 'POST', body: { mode, revision: scout?.revision ?? 0, request_id: newCid() } });
      onscout?.(m);
      if (m.scout?.mine && m.scout.lease) saveLease(match.id, { lease: m.scout.lease, rev: m.scout.revision });
      return m.scout?.lease || null;
    } catch (err) {
      if (err.code === 'scouted_elsewhere' || err.code === 'scout_lease_expired') {
        if (err.current?.scout) onscout?.({ ...match, scout: err.current.scout });
        error = 'Inzwischen scoutet jemand anderes. Stand aktualisiert.';
      } else error = err.message;
      return null;
    } finally {
      acquiring = false;
    }
  }
  async function takeover() { if (await acquire('takeover')) { error = ''; showToast('Du scoutest jetzt'); } }

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
  // a set without its own lineup inherits the closest lower one, exactly as the engine replays it
  const existing = $derived.by(() => {
    const l = match?.lineups || {};
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
        if (!tabOwner) { error = 'Dieses Spiel wird in einem anderen Tab dieses Browsers gescoutet.'; return; }
        // unsent actions of this device change the replay: they go first
        if (loadOps(match.id).length) { error = 'Dieses Gerät hat noch nicht gesendete Aktionen. Erst die Live-Seite öffnen und senden lassen.'; return; }
        // the lease: ours, or a conditional claim of a free/stale match; a
        // current holder elsewhere needs the explicit takeover above
        let lease = scout?.mine ? scout.lease : null;
        if (!lease) {
          if (holderElse) { error = `${scout.actor || 'Jemand'} scoutet gerade auf einem anderen Gerät. Zum Speichern erst übernehmen.`; return; }
          lease = await acquire('claim');
          if (!lease) return;
        }
        await api(`/matches/${match.id}`, { method: 'PATCH', body: { version: match.version, opponent, date, time, hall, home, first_serve, notes }, lease });
        await api(`/matches/${match.id}/lineups/${set}`, { method: 'PUT', body: lineup, lease });
        showToast('Gespeichert');
        goto(`${base}/live/${match.id}`);
      } else {
        const m = await api('/matches', { method: 'POST', body: { opponent, date, time, hall, home, first_serve, notes, lineup } });
        showToast('Spiel angelegt');
        goto(`${base}/live/${m.id}`);
      }
    } catch (err) {
      if (err.code === 'scouted_elsewhere' || err.code === 'scout_lease_expired') {
        if (err.current?.scout) onscout?.({ ...match, scout: err.current.scout });
        error = 'Inzwischen scoutet jemand anderes auf einem anderen Gerät. Stand aktualisiert, zum Speichern erst übernehmen.';
      } else error = err.status === 409 ? 'Jemand hat das Spiel zwischenzeitlich geändert. Seite neu laden.' : err.message;
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
    {#if match && holderElse}
      <div class="scoutbar" role="status">
        <span class="av">{initialsOf(scout.actor)}</span>
        <span class="txt"><b>{scout.actor || 'Jemand'}</b> scoutet auf einem anderen Gerät{scout.device ? ` (${scout.device})` : ''} · seit {hhmm(scout.since)}. Aufstellung und Aufschlag gehören zum Scouting.</span>
        <button class="btn primary" type="button" onclick={takeover} disabled={acquiring || !$online}>Scouting übernehmen</button>
      </div>
    {:else if match && !tabOwner}
      <div class="scoutbar" role="status"><span class="txt">Dieses Spiel wird in einem anderen Tab dieses Browsers gescoutet. Aufstellung und Aufschlag lassen sich nur dort ändern.</span></div>
    {:else if match && pendingOps}
      <p class="err" style="margin-top:10px">Dieses Gerät hat {pendingOps} nicht gesendete Aktionen. Erst die Live-Seite öffnen und senden lassen.</p>
    {/if}
    {#if error}<p class="err" style="margin-top:10px">{error}</p>{/if}
    <div class="row" style="margin-top:14px">
      <button class="btn primary big" type="submit" disabled={busy || acquiring || fieldPlayers.length < 6 || (match && (holderElse || pendingOps > 0 || !tabOwner))}>{match ? 'Speichern & zum Live-Scouting' : 'Spiel anlegen & starten'}</button>
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
  .scoutbar .av { width: 28px; height: 28px; border-radius: 50%; flex: none; display: grid; place-items: center; background: var(--panel); color: var(--accent-text); font-family: var(--disp); font-weight: 700; font-size: 12px; }
  .scoutbar .txt { flex: 1 1 200px; min-width: 0; overflow-wrap: anywhere; line-height: 1.3; }
  .scoutbar .btn { flex: 0 0 auto; height: 34px; padding: 0 12px; }
  @media (max-width: 760px) { .lslot select { height: 40px; font-size: 16px; padding: 0 2px; } }
  .warn { color: var(--g-neg); font-size: 13px; }
  .ok { color: var(--g-win); font-size: 13px; }
  .bad { color: var(--g-err); font-size: 13px; }
  .f > span { font-size: 12px; color: var(--ink-2); display: block; margin-bottom: 4px; }
</style>
