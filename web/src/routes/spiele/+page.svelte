<script>
  import { base } from '$app/paths';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations } from '$lib/stores.js';
  import { fmtDate } from '$lib/engine.js';
  import { cacheList, cachedList, cacheMatch, offlineReady } from '$lib/offline.js';
  import { sync } from '$lib/uploader.js';
  import RecordingsPanel from '$lib/components/RecordingsPanel.svelte';
  let open = $state({}); // match id → recordings panel shown
  let matches = $state(null);
  let error = $state('');
  let stale = $state(false);
  let ready = $state({});
  const canScout = $derived(['coach', 'assistant'].includes($me?.user?.role));
  const listKey = () => 'matches_' + ($me?.team?.id ?? 0);
  async function load() {
    try { matches = (await api('/matches')).matches; stale = false; cacheList(listKey(), matches); mark(); prefetch(); }
    catch (e) {
      const c = cachedList(listKey());
      if (c) { matches = c; stale = true; mark(); } else error = e.offline ? 'Keine Verbindung.' : e.message;
    }
  }
  // running and upcoming matches are stored on this device, so the live
  // screen opens and scouting works without reception
  async function prefetch() {
    for (const m of (matches || []).filter((m) => m.status !== 'done')) {
      if (offlineReady(m)) continue;
      try { cacheMatch(m.id, await api(`/matches/${m.id}`)); } catch { /* next visit */ }
    }
    mark();
  }
  function mark() { ready = Object.fromEntries((matches || []).map((m) => [m.id, offlineReady(m)])); }
  $effect(() => { untrack(load); });
  $effect(() => { if ($mutations?.entity === 'match' || $mutations?.entity === 'action' || $mutations?.entity === 'resync') untrack(load); });
  const live = $derived((matches || []).filter((m) => m.status === 'live'));
  const planned = $derived((matches || []).filter((m) => m.status === 'planned'));
  const done = $derived((matches || []).filter((m) => m.status === 'done'));
</script>

<svelte:head><title>Sideout — Spiele</title></svelte:head>
<main class="page">
  <div class="toolbar"><h1>Spiele</h1>{#if stale}<span class="small muted">Offline: Liste vom letzten Laden</span>{/if}<span class="spacer"></span>{#if canScout}<a class="btn primary" href="{base}/spiele/neu">+ Spiel anlegen</a>{/if}</div>
  {#if error}<div class="panel empty">{error}</div>
  {:else if !matches}<div class="panel empty">Lade…</div>
  {:else if stale && !matches.length}<div class="panel empty">Offline, keine gespeicherte Liste.</div>
  {:else if !matches.length}
    <div class="panel empty">Noch kein Spiel. {#if canScout}<a href="{base}/spiele/neu">Erstes Spiel anlegen</a>{/if}</div>
  {:else}
    {#each [['Läuft gerade', live], ['Geplant', planned], ['Gespielt', done]] as [title, list]}
      {#if list.length}
        <section class="panel">
          <div class="panel-head"><h2>{title}</h2></div>
          <ul class="matches">
            {#each list as m (m.id)}
              <li>
                <span class="d">{fmtDate(m.date)}{#if m.time}<br />{m.time}{/if}</span>
                <span><a class="opp" href={m.status === 'planned' ? `${base}/spiele/${m.id}` : `${base}/live/${m.id}`}>{m.opponent}</a><div class="ha">{m.home ? 'Heim' : 'Auswärts'}{m.hall ? ' · ' + m.hall : ''}{#if m.status !== 'done' && ready[m.id]}<span class="offl" title="Auf diesem Gerät gespeichert: Scouten geht auch ohne Empfang">offline bereit</span>{/if}{#if $sync.byMatch[m.id]}<span class="offl warn" title="Änderungen dieses Geräts, die noch nicht auf dem Server sind">{$sync.byMatch[m.id]} nicht hochgeladen</span>{/if}{#if m.scouting}<span class="who">scoutet: {m.scouting.actor || 'jemand'}{m.scouting.device ? ` (${m.scouting.device})` : ''}</span>{/if}{#if m.recordings > 1 || m.recordings_contained > 0 || (m.recordings > 0 && $sync.byMatch[m.id])}<button class="lnk" onclick={() => (open = { ...open, [m.id]: !open[m.id] })}>{m.recordings} {m.recordings === 1 ? 'Aufzeichnung' : 'Aufzeichnungen'} {open[m.id] ? '▴' : '▾'}</button>{/if}</div></span>
                <span class="sets">{m.state.sets.map((s) => s.us + ':' + s.them).join(' ')}{#if m.status === 'live'} <em>({m.state.us}:{m.state.them})</em>{/if}</span>
                <span class="res" class:w={m.status === 'done' && m.state.sets_won > m.state.sets_lost} class:l={m.status === 'done' && m.state.sets_won < m.state.sets_lost}>{m.status === 'planned' ? '–' : `${m.state.sets_won}:${m.state.sets_lost}`}</span>
                <span class="acts">
                  {#if m.status === 'live' && canScout}<a class="btn primary" href="{base}/live/{m.id}">Scouten</a>
                  {:else if m.status === 'planned' && canScout}<a class="btn" href="{base}/live/{m.id}">Starten</a>
                  {:else}<a class="btn" href="{base}/auswertung/{m.id}">Auswertung</a>{/if}
                </span>
              </li>
              {#if open[m.id]}
                <li class="recs-row"><RecordingsPanel matchId={m.id} onchange={load} /></li>
              {/if}
            {/each}
          </ul>
        </section>
      {/if}
    {/each}
  {/if}
</main>

<style>
  .matches { list-style: none; margin: 0; padding: 0; }
  .who { margin-left: 6px; font-size: 11px; color: var(--accent-text); white-space: nowrap; }
  .offl.warn { color: var(--g-neg); border-color: var(--g-neg); }
  .lnk { margin-left: 6px; font-size: 11px; color: var(--accent-text); background: none; border: 0; padding: 0; cursor: pointer; font: inherit; font-size: 11px; white-space: nowrap; }
  .matches li.recs-row { display: block; padding: 0 0 4px; }
  .offl { margin-left: 6px; font-size: 11px; padding: 1px 6px; border-radius: 9px; background: var(--raised); color: var(--ok); border: 1px solid var(--line-soft); white-space: nowrap; }
  .matches li { display: grid; grid-template-columns: 64px minmax(0, 1fr) auto auto auto; gap: 10px; align-items: center; padding: 9px 0; border-bottom: 1px solid var(--line-soft); font-size: 14px; }
  .matches li:last-child { border-bottom: 0; }
  .d { color: var(--ink-3); font-size: 12px; }
  .opp { color: var(--ink); font-weight: 600; text-decoration: none; }
  .ha { font-size: 11px; color: var(--ink-3); }
  .sets { color: var(--ink-3); font-size: 11px; text-align: right; max-width: 120px; }
  .res { font-family: var(--disp); font-size: 22px; font-weight: 700; min-width: 40px; text-align: right; }
  .res.w { color: var(--g-win); } .res.l { color: var(--g-err); }
  @media (max-width: 600px) { .sets { display: none; } .matches li { grid-template-columns: 52px minmax(0, 1fr) auto auto; gap: 8px; } }
</style>
