<script>
  import { base } from '$app/paths';
  // Team home: the running match, season tiles, matches, top lists, trends.
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations } from '$lib/stores.js';
  import { eff, fmtDM } from '$lib/engine.js';

  let season = $state(null);
  let matches = $state(null);
  let error = $state('');
  const canScout = $derived(['coach', 'assistant'].includes($me?.user?.role));

  async function load() {
    try {
      const [s, m] = await Promise.all([api('/season/stats'), api('/matches')]);
      season = s; matches = m.matches;
    } catch (e) { error = e.offline ? 'Keine Verbindung.' : e.message; }
  }
  $effect(() => { untrack(load); });
  $effect(() => { if ($mutations && ['match', 'action', 'player', 'resync'].includes($mutations.entity)) untrack(load); });

  const live = $derived((matches || []).find((m) => m.status === 'live'));
  const lastDone = $derived((matches || []).find((m) => m.status === 'done'));
  const next = $derived((matches || []).filter((m) => m.status === 'planned').sort((a, b) => a.date.localeCompare(b.date))[0]);
  const t = $derived(season?.team);
  const byId = $derived(Object.fromEntries((season?.players || []).map((p) => [p.id, p])));
  let players = $state([]);
  $effect(() => { api('/players').then((r) => (players = r.players)).catch(() => {}); });
  const pById = $derived(Object.fromEntries(players.map((p) => [p.id, p])));
  const name = (id) => pById[id]?.name || '?';
  const num = (id) => pById[id]?.number ?? '';
  const topPts = $derived((season?.players || []).slice().sort((a, b) => b.pts - a.pts).slice(0, 4));
  const topEff = $derived((season?.players || []).filter((x) => x.n >= 20).sort((a, b) => (b.k - b.e) / b.n - (a.k - a.e) / a.n).slice(0, 3));
  const topRec = $derived((season?.players || []).filter((x) => x.rn >= 15).sort((a, b) => b.rsum / b.rn - a.rsum / a.rn).slice(0, 3));
  const recent = $derived((season?.matches || []).slice(-4));
</script>

<svelte:head><title>Sideout — {$me?.team?.name || 'Team'}</title></svelte:head>
<main class="page">
  <div class="toolbar">
    <div><h1>{$me?.team?.name}</h1><div class="small muted">{[$me?.team?.league, $me?.team?.season ? 'Saison ' + $me.team.season : ''].filter(Boolean).join(' · ')}</div></div>
    <span class="spacer"></span>
    {#if canScout}<a class="btn primary" href="{base}/spiele/neu">+ Spiel anlegen</a>{/if}
  </div>
  {#if error}<div class="panel empty">{error}</div>
  {:else if !season || !matches}<div class="panel empty">Lade…</div>
  {:else}
    <div class="hero">
      <section class="panel livecard">
        {#if live}
          <span class="pulse"><i></i> Läuft gerade · Satz {live.state.set}</span>
          <div class="sc">{live.state.us}:{live.state.them}<small>Sätze {live.state.sets_won}:{live.state.sets_lost}</small></div>
          <div class="opp">gegen {live.opponent}</div>
          <div class="small muted">{live.state.sets.map((s) => s.us + ':' + s.them).join('  ')}{live.hall ? ' · ' + live.hall : ''}</div>
          <div class="row" style="margin-top:12px"><a class="btn primary big" href="{base}/live/{live.id}">{canScout ? 'Weiter scouten' : 'Live verfolgen'}</a><a class="btn big" href="{base}/auswertung/{live.id}">Zwischenstand</a></div>
        {:else if next}
          <span class="pulse">Nächstes Spiel · {fmtDM(next.date)}{next.time ? ' ' + next.time : ''}</span>
          <div class="opp" style="font-size:24px;margin-top:8px">gegen {next.opponent}</div>
          <div class="small muted">{next.home ? 'Heim' : 'Auswärts'}{next.hall ? ' · ' + next.hall : ''}</div>
          <div class="row" style="margin-top:12px">{#if canScout}<a class="btn primary big" href="{base}/live/{next.id}">Scouting starten</a><a class="btn big" href="{base}/spiele/{next.id}">Aufstellung</a>{/if}</div>
        {:else if lastDone}
          <span class="pulse">Letztes Spiel</span>
          <div class="sc">{lastDone.state.sets_won}:{lastDone.state.sets_lost}</div>
          <div class="opp">gegen {lastDone.opponent}</div>
          <div class="small muted">{lastDone.state.sets.map((s) => s.us + ':' + s.them).join('  ')}</div>
          <div class="row" style="margin-top:12px"><a class="btn primary" href="{base}/auswertung/{lastDone.id}">Auswertung</a>{#if canScout}<a class="btn" href="{base}/spiele/neu">Nächstes Spiel anlegen</a>{/if}</div>
        {:else}
          <span class="pulse">Willkommen</span>
          <div class="opp" style="margin-top:8px">Noch kein Spiel gescoutet.</div>
          <p class="small muted">Erst den Kader anlegen, dann das erste Spiel mit Startaufstellung. Beim Scouten: Spielerin tippen, Aktion tippen. Alles andere rechnet Sideout.</p>
          <div class="row" style="margin-top:8px">{#if canScout}<a class="btn primary big" href="{base}/kader">Kader anlegen</a><a class="btn big" href="{base}/spiele/neu">Spiel anlegen</a>{/if}</div>
        {/if}
      </section>
      <section class="panel">
        <div class="panel-head"><h2>Saison</h2><span class="small muted">{season.matches.length} {season.matches.length === 1 ? 'Spiel' : 'Spiele'} gescoutet</span></div>
        <div class="tiles two">
          <div class="tile"><div class="v">{t.won}<small>:{t.lost}</small></div><div class="k">Bilanz</div><div class="d">Sätze {t.sets_won}:{t.sets_lost}</div></div>
          <div class="tile"><div class="v">{t.so_n ? Math.round((t.so_won / t.so_n) * 100) : '–'}<small>%</small></div><div class="k">Sideout</div><div class="d">Break {t.brk_n ? Math.round((t.brk_won / t.brk_n) * 100) + ' %' : '–'}</div></div>
          <div class="tile"><div class="v">{t.a_n ? eff((t.a_k - t.a_e) / t.a_n) : '–'}</div><div class="k">Angriff Eff.</div><div class="d">{t.a_k} Punkte aus {t.a_n} Versuchen</div></div>
          <div class="tile"><div class="v">{t.r_n ? (t.r_sum / t.r_n).toFixed(2) : '–'}</div><div class="k">Annahme Ø</div><div class="d">Skala 0–3</div></div>
        </div>
      </section>
    </div>

    <div class="grid3">
      <section class="panel">
        <div class="panel-head"><h2>Spiele</h2><a class="small" href="{base}/spiele">Alle</a></div>
        <ul class="matches">
          {#each matches.slice(0, 8) as m (m.id)}
            <li>
              <span class="d">{fmtDM(m.date)}</span>
              <span><a href={m.status === 'planned' ? `${base}/spiele/${m.id}` : `${base}/auswertung/${m.id}`}>{m.opponent}</a><div class="ha">{m.home ? 'Heim' : 'Auswärts'}{m.status === 'live' ? ' · läuft' : m.status === 'planned' ? ' · geplant' : ''}</div></span>
              <span class="sets">{m.state.sets.map((s) => s.us + ':' + s.them).join(' ')}</span>
              <span class="res" class:w={m.status === 'done' && m.state.sets_won > m.state.sets_lost} class:l={m.status === 'done' && m.state.sets_won < m.state.sets_lost}>{m.status === 'planned' ? '–' : `${m.state.sets_won}:${m.state.sets_lost}`}</span>
            </li>
          {:else}<li class="muted small">Noch keine Spiele.</li>{/each}
        </ul>
      </section>
      <section class="panel">
        <div class="panel-head"><h2>Beste Werte der Saison</h2><span class="small muted">alle gescouteten Spiele</span></div>
        <h3 style="margin:6px 0 4px">Punkte</h3>
        <ul class="top">{#each topPts as x (x.id)}<li><span class="jersey">{num(x.id)}</span><span>{name(x.id)}<div class="sub">{x.k} Angriff · {x.ace} Ass · {x.bpts} Block</div></span><span class="v">{x.pts}</span></li>{:else}<li class="muted small">–</li>{/each}</ul>
        <h3 style="margin:12px 0 4px">Angriff-Effizienz <span class="muted">(≥ 20 Versuche)</span></h3>
        <ul class="top">{#each topEff as x (x.id)}<li><span class="jersey">{num(x.id)}</span><span>{name(x.id)}<div class="sub">{x.k} Pkt · {x.e} Fehler · {x.n} Versuche</div></span><span class="v">{eff((x.k - x.e) / x.n)}</span></li>{:else}<li class="muted small">–</li>{/each}</ul>
        <h3 style="margin:12px 0 4px">Annahme Ø <span class="muted">(≥ 15 Annahmen)</span></h3>
        <ul class="top">{#each topRec as x (x.id)}<li><span class="jersey">{num(x.id)}</span><span>{name(x.id)}<div class="sub">{x.rn} Annahmen</div></span><span class="v">{(x.rsum / x.rn).toFixed(2)}</span></li>{:else}<li class="muted small">–</li>{/each}</ul>
      </section>
      <section class="panel">
        <div class="panel-head"><h2>Sideout-Trend</h2><span class="small muted">je Spiel</span></div>
        <div class="trend">{#each recent as m (m.id)}<div class="m"><div class="o">{m.opponent}</div><div class="v">{m.sideout.n ? Math.round((m.sideout.won / m.sideout.n) * 100) : '–'}<small>%</small></div></div>{:else}<div class="muted small">–</div>{/each}</div>
        <div class="panel-head" style="margin-top:14px"><h2>Angriff-Effizienz</h2><span class="small muted">je Spiel</span></div>
        <div class="trend">{#each recent as m (m.id)}<div class="m"><div class="o">{m.opponent}</div><div class="v">{m.attack.n ? eff((m.attack.k - m.attack.e) / m.attack.n) : '–'}</div></div>{:else}<div class="muted small">–</div>{/each}</div>
        <p class="small muted" style="margin:12px 0 0">Alle Kennzahlen werden aus den Aktionsprotokollen berechnet, keine Handeingaben.</p>
      </section>
    </div>
  {/if}
</main>

<style>
  .hero { display: grid; grid-template-columns: 1.2fr 1fr; gap: 14px; margin-bottom: 14px; }
  @media (max-width: 900px) { .hero { grid-template-columns: 1fr; } }
  .hero .panel + .panel { margin-top: 0; }
  .livecard { border-color: var(--accent); background: linear-gradient(180deg, var(--accent-soft), transparent 70%), var(--panel); }
  .livecard .sc { font-family: var(--disp); font-size: 56px; font-weight: 700; line-height: 1; display: flex; align-items: baseline; gap: 10px; }
  .livecard .sc small { font-size: 18px; color: var(--ink-2); font-weight: 600; }
  .livecard .opp { font-size: 18px; font-weight: 600; margin-top: 4px; }
  .pulse { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--accent-text); font-weight: 600; }
  .pulse i { width: 8px; height: 8px; border-radius: 50%; background: var(--g-err); }
  .tiles.two { grid-template-columns: repeat(2, 1fr); }
  .matches { list-style: none; margin: 0; padding: 0; }
  .matches li { display: grid; grid-template-columns: 52px minmax(0, 1fr) auto auto; gap: 10px; align-items: center; padding: 9px 0; border-bottom: 1px solid var(--line-soft); font-size: 14px; }
  .matches li:last-child { border-bottom: 0; }
  .matches a { color: var(--ink); text-decoration: none; font-weight: 600; }
  .d { color: var(--ink-3); font-size: 12px; }
  .ha { font-size: 11px; color: var(--ink-3); }
  .sets { color: var(--ink-3); font-size: 11px; text-align: right; max-width: 110px; }
  .res { font-family: var(--disp); font-size: 22px; font-weight: 700; min-width: 40px; text-align: right; }
  .res.w { color: var(--g-win); } .res.l { color: var(--g-err); }
  .grid3 { display: grid; grid-template-columns: repeat(3, 1fr); gap: 14px; }
  @media (max-width: 1000px) { .grid3 { grid-template-columns: 1fr; } }
  .grid3 .panel + .panel { margin-top: 0; }
  .top { list-style: none; margin: 0; padding: 0; }
  .top li { display: grid; grid-template-columns: 36px 1fr auto; gap: 8px; align-items: center; padding: 6px 0; border-bottom: 1px solid var(--line-soft); font-size: 14px; }
  .top li:last-child { border-bottom: 0; }
  .top .jersey { font-family: var(--disp); font-weight: 700; font-size: 18px; color: var(--ink-2); }
  .top .v { font-weight: 700; font-variant-numeric: tabular-nums; }
  .top .sub { color: var(--ink-3); font-size: 12px; }
  .trend { display: grid; grid-template-columns: repeat(auto-fit, minmax(90px, 1fr)); gap: 8px; }
  .trend .m { background: var(--raised); border-radius: 8px; padding: 8px; }
  .trend .m .o { font-size: 12px; color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .trend .m .v { font-family: var(--disp); font-size: 24px; font-weight: 700; }
  .trend .m .v small { font-size: 12px; color: var(--ink-3); }
</style>
