<script>
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations } from '$lib/stores.js';
  import { fmtDate } from '$lib/engine.js';
  let matches = $state(null);
  let error = $state('');
  const canScout = $derived(['coach', 'assistant'].includes($me?.user?.role));
  async function load() { try { matches = (await api('/matches')).matches; } catch (e) { error = e.offline ? 'Keine Verbindung.' : e.message; } }
  $effect(() => { untrack(load); });
  $effect(() => { if ($mutations?.entity === 'match' || $mutations?.entity === 'action' || $mutations?.entity === 'resync') untrack(load); });
  const live = $derived((matches || []).filter((m) => m.status === 'live'));
  const planned = $derived((matches || []).filter((m) => m.status === 'planned'));
  const done = $derived((matches || []).filter((m) => m.status === 'done'));
</script>

<svelte:head><title>Sideout — Spiele</title></svelte:head>
<main class="page">
  <div class="toolbar"><h1>Spiele</h1><span class="spacer"></span>{#if canScout}<a class="btn primary" href="/spiele/neu">+ Spiel anlegen</a>{/if}</div>
  {#if error}<div class="panel empty">{error}</div>
  {:else if !matches}<div class="panel empty">Lade…</div>
  {:else if !matches.length}
    <div class="panel empty">Noch kein Spiel. {#if canScout}<a href="/spiele/neu">Erstes Spiel anlegen</a>{/if}</div>
  {:else}
    {#each [['Läuft gerade', live], ['Geplant', planned], ['Gespielt', done]] as [title, list]}
      {#if list.length}
        <section class="panel">
          <div class="panel-head"><h2>{title}</h2></div>
          <ul class="matches">
            {#each list as m (m.id)}
              <li>
                <span class="d">{fmtDate(m.date)}{#if m.time}<br />{m.time}{/if}</span>
                <span><a class="opp" href={m.status === 'planned' ? `/spiele/${m.id}` : `/live/${m.id}`}>{m.opponent}</a><div class="ha">{m.home ? 'Heim' : 'Auswärts'}{m.hall ? ' · ' + m.hall : ''}</div></span>
                <span class="sets">{m.state.sets.map((s) => s.us + ':' + s.them).join(' ')}{#if m.status === 'live'} <em>({m.state.us}:{m.state.them})</em>{/if}</span>
                <span class="res" class:w={m.status === 'done' && m.state.sets_won > m.state.sets_lost} class:l={m.status === 'done' && m.state.sets_won < m.state.sets_lost}>{m.status === 'planned' ? '–' : `${m.state.sets_won}:${m.state.sets_lost}`}</span>
                <span class="acts">
                  {#if m.status === 'live' && canScout}<a class="btn primary" href="/live/{m.id}">Scouten</a>
                  {:else if m.status === 'planned' && canScout}<a class="btn" href="/live/{m.id}">Starten</a>
                  {:else}<a class="btn" href="/auswertung/{m.id}">Auswertung</a>{/if}
                </span>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/each}
  {/if}
</main>

<style>
  .matches { list-style: none; margin: 0; padding: 0; }
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
