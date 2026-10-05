<script>
  import { base } from '$app/paths';
  // The capture screen. Renders this device's own recording (base + local
  // edits from IndexedDB) when it has one, otherwise the match result the
  // server shows. Every tap is an edit saved on the device before anything
  // is sent; the uploader (uploader.js) carries it to the server whenever
  // it can. There is no lease: viewing never writes, a second device simply
  // records on its own, and the coach picks the result.
  import { page } from '$app/stores';
  import { untrack, tick } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations, online, showToast, logDiag } from '$lib/stores.js';
  import { replay, courtView, expectedSkills, stats, SKILL, PAD, GRADE_CLASS, ROMAN, firstName, eff, fix } from '$lib/engine.js';
  import { cacheMatch, cachedMatch } from '$lib/offline.js';
  import { fold, baseFromMatch, newAction, playersOf, recordingLabel } from '$lib/recording.js';
  import { myRecording, getRecording, editsFrom, appendEdit, createRecording, deviceId, onChange } from '$lib/recstore.js';
  import { sync, kick } from '$lib/uploader.js';
  import { tabWriter } from '$lib/scoutlock.js';
  import Court from '$lib/components/Court.svelte';
  import Pad from '$lib/components/Pad.svelte';
  import Voice from '$lib/components/Voice.svelte';

  const id = $derived(Number($page.params.id));
  let match = $state(null);      // the server payload (or the cached one)
  let rec = $state(null);        // this device's recording of the match, or null
  let edits = $state([]);        // its edits, in order
  let selected = $state(null);
  let scope = $state('set');
  let loadError = $state('');
  let tabOwner = $state(false);
  // every capture step (create, add, undo) runs in one chain: a tap never
  // sees the recording of the tap before it half-made, and sequence numbers
  // are computed after the previous edit is saved
  let chain = Promise.resolve();
  const serial = (fn) => { const p = chain.then(fn, fn); chain = p.catch(() => {}); return p; };
  let myDevice = $state('');
  // a page that was left (or switched to another match) must not touch
  // anything any more: every await checks `stale()`
  let alive = true;
  const stale = (mid) => !alive || mid !== id;
  $effect(() => () => { alive = false; });

  const canScout = $derived(['coach', 'assistant'].includes($me?.user?.role));
  const canWrite = $derived(canScout && tabOwner && $sync.supported);
  const hhmm = (iso) => { const d = iso ? new Date(iso.endsWith('Z') || iso.includes('T') ? iso : iso.replace(' ', 'T') + 'Z') : null; return d && !isNaN(d) ? d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' }) : '–'; };

  // what is shown: my recording, else the match result (the selected recording, or the planning)
  const snap = $derived.by(() => {
    if (rec) return fold(rec.base, edits);
    if (!match) return null;
    return { cfg: { first_serve_us: match.first_serve === 'us', lineups: match.lineups || {} }, roster: match.roster || [], actions: match.actions || [] };
  });
  const players = $derived(match && snap ? playersOf(match.players, snap) : []);
  const byId = $derived(Object.fromEntries(players.map((p) => [p.id, p])));
  const cfg = $derived(snap?.cfg || null);
  const actionsAll = $derived(snap?.actions || []);
  const st = $derived(cfg ? replay(cfg, actionsAll) : null);
  const hasLineup = $derived(!!cfg && Object.keys(cfg.lineups || {}).length > 0);
  const court = $derived(st ? courtView(st.lineup, st.libero, byId, st.libero_for, st.libero_off, st.serving) : []);
  const expected = $derived(st ? expectedSkills(st) : []);
  // who the next action most likely belongs to: the server on I, the setter for a set
  const setters = $derived(court.filter((c) => byId[c.id]?.position === 'Z' && !c.libero).map((c) => c.id));
  const suggested = $derived.by(() => {
    if (!st) return [];
    if (expected.includes('S') && st.serving) return [st.lineup[0]];
    if (expected.includes('E')) return setters;
    return [];
  });
  const last = $derived(actionsAll[actionsAll.length - 1] || null);
  // set five is a new toss: until the coach says who serves, nothing scores
  // (substitutions and libero changes are fine). A rally already recorded
  // in set five ends the question, the choice would be moot.
  const tossPending = $derived(!!st && canScout && st.set === 5 && !st.finished && !st.rows.some((r) => r.set === 5 && (r.skill === 'srv' || r.outcome)));
  const stat = $derived(st ? stats(cfg, players, actionsAll, scope === 'set' ? st.set : 0) : null);
  // timeline: every action left to right, a score pill after each rally, a divider per set
  const timeline = $derived.by(() => {
    if (!st) return [];
    const out = []; let set = 0;
    for (const r of st.rows) {
      if (r.set !== set) { set = r.set; out.push({ t: 'set', n: set, key: 'set' + set }); }
      out.push({ t: 'act', r, key: 'a' + r.id });
      if (r.outcome) out.push({ t: 'score', us: r.us + (r.outcome === 'us' ? 1 : 0), them: r.them + (r.outcome === 'them' ? 1 : 0), won: r.outcome === 'us', key: 's' + r.id });
    }
    return out;
  });
  let tlEl = $state(null);
  // newest entry stays in view
  $effect(() => { void timeline.length; const el = tlEl; if (el) tick().then(() => { el.scrollLeft = el.scrollWidth; }); });

  // ----- sync and the other recordings -----
  const pendingHere = $derived($sync.byMatch[id] || 0);
  const recError = $derived(rec?.error || null);
  // an upload the server refused: said as a toast, the export lives in Einstellungen
  let toldError = null;
  $effect(() => {
    const e = recError;
    if (!e || e === toldError) return;
    toldError = e;
    untrack(() => showToast(`Upload abgelehnt: ${e} · die Aufzeichnung bleibt auf dem Gerät, Export unter Einstellungen`, true));
  });
  const recordings = $derived(match?.recordings || []);
  const others = $derived(recordings.filter((r) => r.device_id !== myDevice));
  const activeOther = $derived(others.find((r) => r.active) || null);
  // someone else is scouting right now: said once as a toast, not as a bar in the way
  let toldAbout = null;
  $effect(() => {
    const o = activeOther;
    if (!o || o.id === toldAbout) return;
    toldAbout = o.id;
    untrack(() => showToast(o.user_id != null && o.user_id === $me?.user?.id ? 'Du scoutest gerade auf einem anderen Gerät' : `${o.user || 'Jemand'} scoutet gerade`));
  });
  const selectedRec = $derived(recordings.find((r) => r.selected) || null);
  // my recording is a copy of the result with nothing in between: the server
  // makes it the result on its first upload, so no word about it meanwhile
  const cleanContinuation = $derived(!!rec && !!match?.selected && rec.origin_id === match.selected && rec.origin_n != null && rec.origin_n === selectedRec?.n);
  // my recording exists but another one is the result: said once as a toast;
  // the recordings and the choice live in the Spiele overview
  const resultIsOther = $derived(!!rec && !!match?.selected && match.selected !== rec.id && !cleanContinuation);
  let toldResult = null;
  $effect(() => {
    const sel = resultIsOther ? match.selected : null;
    if (!sel || sel === toldResult) return;
    toldResult = sel;
    const who = selectedRec ? recordingLabel(selectedRec) : 'jemand anderem';
    setTimeout(() => showToast(`Als Ergebnis zählt die Aufzeichnung von ${who}, nicht die dieses Geräts · Auswahl unter Spiele`), activeOther ? 2600 : 0);
  });

  async function loadServer(mid) {
    try {
      const m = await api(`/matches/${mid}`);
      if (stale(mid)) return;
      match = m;
      cacheMatch(mid, m);
      loadError = '';
    } catch (e) {
      if (stale(mid)) return;
      const c = cachedMatch(mid);
      if (c) { if (!match) match = c; if (!e.offline) showToast(e.message, true); }
      else if (!match) loadError = e.offline ? 'Keine Verbindung und kein lokaler Stand.' : e.message;
    }
  }
  async function refreshRec(mid) {
    if (!rec) return;
    try { const r = await getRecording(rec.id); if (!stale(mid) && r && rec && r.id === rec.id) rec = r; } catch { /* next change */ }
  }
  async function loadLocal(mid) {
    try {
      const r = await myRecording(mid, $me?.user?.id ?? null);
      const es = r ? (await editsFrom(r.id, 1)).map((e) => e.body) : [];
      if (stale(mid)) return;
      rec = r; edits = es;
    } catch (e) {
      if (!stale(mid)) showToast('Lokaler Speicher: ' + (e?.message || e), true);
    }
  }

  $effect(() => {
    const mid = id;
    untrack(() => {
      match = null; rec = null; edits = []; loadError = '';
      deviceId().then((d) => { if (!stale(mid)) myDevice = d; }).catch(() => {});
      loadLocal(mid).then(() => loadServer(mid));
    });
    // the writer tab for this match in this browser; the other tabs watch
    const lock = tabWriter(mid, (held) => {
      tabOwner = held;
      // another tab of this browser may have added edits meanwhile
      if (held) untrack(() => loadLocal(mid));
    });
    // edits written by another tab show up here too; the writer tab only
    // refreshes its recording row (upload state), never its edit list
    const off = onChange(() => untrack(() => { if (!tabOwner) loadLocal(mid); else refreshRec(mid); }));
    return () => { lock.release(); off(); };
  });
  // the identity may land after the first effect (layout order, offline start)
  // or change while the page is open (another tab signed in): the local
  // recording of the signed-in account is looked up again
  $effect(() => { const uid = $me?.user?.id ?? null; const mid = id; if (uid != null) untrack(() => loadLocal(mid)); });

  // another device wrote to this match, or the coach chose: refetch the server side only
  $effect(() => {
    const m = $mutations;
    if (!m) return;
    untrack(() => {
      if (!match) return;
      const mine = (m.entity === 'action' || m.entity === 'match') && m.id === id;
      if (mine || m.entity === 'resync') loadServer(id);
    });
  });
  $effect(() => { if ($online) untrack(() => { kick(); if (match) loadServer(id); }); });

  // phone: the live screen has no top bar. Scoped via a body class, because a
  // :global rule in this component would stay loaded after leaving the page.
  $effect(() => {
    document.body.classList.add('live-phone');
    return () => document.body.classList.remove('live-phone');
  });

  // keep the screen on while scouting
  $effect(() => {
    let lock = null;
    const req = () => navigator.wakeLock?.request?.('screen').then((l) => (lock = l)).catch(() => {});
    req();
    const vis = () => { if (document.visibilityState === 'visible') req(); };
    document.addEventListener('visibilitychange', vis);
    return () => { document.removeEventListener('visibilitychange', vis); lock?.release?.(); };
  });

  function describe(a) {
    if (!a) return '';
    if (a.skill === 'adj') return a.grade === '#' ? 'Nachtrag: +1 wir' : 'Nachtrag: +1 Gegner';
    if (a.skill === 'rot') return 'Nachtrag: rotiert';
    if (a.skill === 'srv') return a.grade === '#' ? 'Nachtrag: Aufschlag wir' : 'Nachtrag: Aufschlag Gegner';
    if (a.skill === 'lib') return a.sub_in == null ? 'Libera raus' : a.sub_out ? `Libera für ${byId[a.sub_out]?.number} ${firstName(byId[a.sub_out])}` : 'Libera automatisch (Mitte)';
    if (a.skill === 'sub') return `Wechsel ${byId[a.sub_out]?.number} → ${byId[a.sub_in]?.number}`;
    if (a.skill === 'opp') return a.grade === '=' ? 'Fehler Gegner' : 'Punkt Gegner';
    const p = byId[a.player_id];
    return `${p?.number} ${firstName(p)} ${SKILL[a.skill].name} ${a.grade} ${PAD[a.skill][a.grade] || ''}`;
  }

  // the write gate, enforced here and in the pad, not only on buttons
  function explainNoWrite() {
    if (!canScout) return;
    if (!$sync.supported) showToast($sync.unsupportedReason, true);
    else if (!tabOwner) showToast('Dieses Spiel wird in einem anderen Tab gescoutet', true);
    else showToast('Scouten nicht möglich', true);
  }

  // ----- capture context: a tap belongs to the match and recording it was
  // made on. The chain may run it after the page moved to another match (or
  // was left): the edit is still saved to that recording, only the screen is
  // updated when it still shows it. -----
  function ctxNow() {
    return { mid: id, m: match, r: rec, uid: $me?.user?.id ?? null };
  }
  /** the recording and edits of the context: from memory while the page still shows it, else from the store */
  async function resolve(ctx) {
    const live = id === ctx.mid; // the page still shows the match the tap was made on
    if (live && rec && (!ctx.r || rec.id === ctx.r.id)) return { r: rec, es: edits, live };
    const r = ctx.r ? await getRecording(ctx.r.id) : await myRecording(ctx.mid, ctx.uid);
    const es = r ? (await editsFrom(r.id, 1)).map((e) => e.body) : [];
    return { r, es, live };
  }
  /** save one edit of a recording; the screen follows only while it shows that recording */
  async function commit(ctx, r, es, edit, note) {
    const cfg0 = fold(r.base, es).cfg;
    const before = replay(cfg0, fold(r.base, es).actions);
    let n;
    try {
      n = await appendEdit(r.id, edit);
    } catch (e) {
      logDiag('store', e?.message || e);
      showToast('Speichern auf dem Gerät fehlgeschlagen: ' + (e?.message || e), true);
      return false;
    }
    kick();
    if (id !== ctx.mid || !rec || rec.id !== r.id) return true; // saved for that match; the page shows another one
    if (edits.length === n - 1) edits = [...edits, edit];
    else loadLocal(ctx.mid); // another writer got in between: the store is the truth
    selected = null;
    const after = replay(cfg0, fold(r.base, [...es, edit]).actions);
    if (note) showToast(note);
    else if (after.set !== before.set) { const s = after.sets[after.sets.length - 1]; showToast(`Satz ${before.set} beendet ${s.us}:${s.them}` + (after.finished || cfg0.lineups?.[after.set] ? '' : ' · Aufstellung übernommen, Wechsel per Drag & Drop')); }
    else if (after.finished) showToast(`Spiel beendet ${after.sets_won}:${after.sets_lost}`);
    return true;
  }

  function queue(a) {
    if (!canScout) return;
    if (!canWrite) { explainNoWrite(); return; }
    const ctx = ctxNow();
    serial(async () => {
      const { r, es, live } = await resolve(ctx);
      if (!r) { await startRecording(ctx, { add: a }); return; }
      const acts = fold(r.base, es).actions;
      if (replay(fold(r.base, es).cfg, acts).finished) { if (live) showToast('Das Spiel ist beendet'); return; }
      await commit(ctx, r, es, { op: 'add', action: newAction(acts, a) });
    });
  }

  // the first tap on this device continues what is loaded, without asking:
  // a fresh recording from the planning when nothing was recorded yet, else
  // a copy of the shown result (another scout's or an import) as the own
  // recording, with its origin noted; the server makes that copy the result
  // when nothing came in between. Saved from the captured context even when
  // the page has already moved on.
  async function startRecording(ctx, pending) {
    if (ctx.uid == null) return;
    await createAndApply(ctx, (ctx.m?.recordings || []).length ? 'copy' : 'fresh', { first: pending });
  }
  /** a pending tap → the edit it means on the given effective log */
  function editFor(pending, acts) {
    if (pending?.add) return { op: 'add', action: newAction(acts, pending.add) };
    if (pending?.undo) { const top = acts[acts.length - 1]; return top ? { op: 'undo', seq: top.seq } : null; }
    if (pending?.edit) return pending.edit;
    return null;
  }
  async function createAndApply(ctx, mode, ask) {
    const m = ctx.m;
    if (!m || ctx.uid == null) return;
    const b = mode === 'copy' ? baseFromMatch(m) : baseFromMatch(m, { planning: true });
    if (!Object.keys(b.lineups).length) { if (id === ctx.mid) showToast('Erst die Aufstellung eintragen', true); return; }
    const acts = b.actions.map((a) => ({ ...a }));
    const first = editFor(ask?.first, acts);
    if (ask?.first && !first) return;
    const sel = (m.recordings || []).find((x) => x.selected) || null;
    let r;
    try {
      r = await createRecording({
        match_id: ctx.mid, team_id: $me?.team?.id ?? null, user_id: ctx.uid, base: b,
        origin_id: mode === 'copy' ? m.selected || null : null,
        origin_n: mode === 'copy' ? sel?.n ?? null : null,
        firstEdit: first
      });
    } catch (e) {
      logDiag('store', e?.message || e);
      showToast('Speichern auf dem Gerät fehlgeschlagen: ' + (e?.message || e), true);
      return;
    }
    kick();
    if (id === ctx.mid) {
      rec = r; edits = first ? [first] : []; selected = null;
      showToast(mode === 'copy' ? `Ergebnis von ${sel ? recordingLabel(sel) : 'jemand anderem'} wird auf diesem Gerät fortgesetzt` : 'Eigene Aufzeichnung begonnen');
    }
  }

  const syncText = $derived(!rec ? '' : pendingHere ? `${pendingHere} nicht hochgeladen` : rec.created ? 'auf Server gespeichert' : 'lokal gespeichert');

  function tapCell(skill, grade) {
    let who = selected;
    if (!who && skill === 'S' && st.serving) who = st.lineup[0];
    if (!who && skill === 'E' && setters.length === 1) who = setters[0];
    if (!who) { showToast('Erst Spielerin auf dem Feld tippen'); return; }
    queue({ skill, grade, player_id: who });
  }

  // ----- bench strip: libero pinned first, then the bench. A chip is
  // dragged onto a court slot (pointer events, works with touch) or tapped
  // to arm it, then the target slot is tapped. -----
  let phoneView = $state('pad');   // phone: 'pad' | 'stats'
  // below 1000 px the mic button swaps the pad for the voice card; the card
  // starts listening inside that tap and only shuts its gate while the pad is back
  let voiceMode = $state(false);
  let voice = $state(null);
  function toggleVoiceMode() { voiceMode = !voiceMode; if (voiceMode) voice?.open(); else voice?.pause(); }
  let extrasOpen = $state(false);  // phone: bench + catch-up shown
  let drag = $state(null);    // { id, x, y, moved }
  let over = $state(null);    // court position under the dragged chip
  // the bench = everyone not on the court right now. The libero leads it
  // while she sits; while she stands in for someone, that someone leads it.
  const liberoCard = $derived(court.find((c) => c.libero) || null);
  const benchChips = $derived.by(() => {
    if (!match || !st) return [];
    const rest = match.players.filter((p) => p.active && !st.lineup.includes(p.id) && p.id !== st.libero).sort((a, b) => a.number - b.number);
    const first = [];
    if (st.libero && !liberoCard && byId[st.libero]) first.push(byId[st.libero]);
    if (liberoCard && byId[liberoCard.replaced]) first.push(byId[liberoCard.replaced]);
    return first.concat(rest);
  });
  const chipNote = (p) => (p.id === st?.libero ? 'Libera' : liberoCard?.replaced === p.id ? `${p.position} · draußen für L` : p.position);

  function slotUnder(x, y) {
    const el = document.elementFromPoint(x, y)?.closest('.slot');
    return el ? Number(el.dataset.pos) : null;
  }
  function chipDown(e, pid) {
    if (!canWrite || st.finished) { if (canScout && !st.finished) explainNoWrite(); return; }
    // every touch on a chip is a drag (touch-action: none, no gesture guessing);
    // the strip scrolls with the ‹ › buttons instead
    e.preventDefault();
    try { e.currentTarget.setPointerCapture?.(e.pointerId); } catch { /* synthetic or already-released pointer */ }
    drag = { id: pid, x: e.clientX, y: e.clientY, moved: false };
  }
  function chipCancel() { drag = null; over = null; } // pointer taken by the browser
  // the bench is paged: as many whole chips as fit between ‹ and ›, the
  // next press shows the next set. Pages are measured from the rendered
  // chips (names differ in width) and recomputed on resize or roster change.
  let benchEl = $state(null);
  let pages = $state([0]);      // index of the first chip of each bpage
  let bpage = $state(0);
  const pageRange = $derived({ first: pages[bpage] ?? 0, last: pages[bpage + 1] ?? Infinity });
  function layoutBench() {
    const el = benchEl; if (!el) return;
    const chips = [...el.querySelectorAll('.chipb')];
    const W = el.clientWidth;
    const starts = []; let i = 0;
    while (i < chips.length) {
      starts.push(i);
      const left = chips[i].offsetLeft; let j = i;
      while (j < chips.length && chips[j].offsetLeft + chips[j].offsetWidth - left <= W + 1) j++;
      i = j > i ? j : i + 1;
    }
    pages = starts.length ? starts : [0];
    if (bpage > pages.length - 1) bpage = Math.max(0, pages.length - 1);
  }
  $effect(() => {
    void benchChips; const el = benchEl; if (!el) return;
    let frame = null;
    let disposed = false;
    // Pagination can show/hide the arrows, which changes the observed width.
    // Apply it next frame, outside ResizeObserver's notification delivery,
    // and coalesce resize bursts (for example while rotating the phone).
    const schedule = () => {
      if (disposed || frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        if (!disposed) layoutBench();
      });
    };
    tick().then(schedule);
    const ro = new ResizeObserver(schedule); ro.observe(el);
    return () => {
      disposed = true;
      ro.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    };
  });
  // show the page: shift the row so the page's first chip sits at the left
  // edge (a transform, not scrollLeft, which cannot go past the end)
  let benchShift = $state(0);
  $effect(() => {
    const el = benchEl; const first = pages[bpage] ?? 0; if (!el) return;
    const chips = el.querySelectorAll('.chipb');
    benchShift = chips[first] ? chips[first].offsetLeft - chips[0].offsetLeft : 0;
  });
  // grabbing cursor and no text selection while a chip is in flight
  $effect(() => {
    document.body.classList.toggle('dragging', !!drag?.moved);
    return () => document.body.classList.remove('dragging');
  });

  // the court positions a chip may be dropped on
  function validTargets(chipId) {
    if (!st) return [];
    if (chipId === st.libero) {
      return [5, 6, 1].filter((pos) => !(pos === 1 && st.serving) && court[pos - 1].id !== st.libero);
    }
    if (liberoCard && chipId === liberoCard.replaced) return [liberoCard.pos];
    return [1, 2, 3, 4, 5, 6];
  }
  // targets light up the moment a finger lands on a chip, not only once it moves
  const targets = $derived(drag ? validTargets(drag.id) : []);
  function chipMove(e) {
    if (!drag) return;
    const moved = drag.moved || Math.hypot(e.clientX - drag.x, e.clientY - drag.y) > 6;
    drag = { ...drag, x: e.clientX, y: e.clientY, moved };
    const pos = moved ? slotUnder(e.clientX, e.clientY) : null;
    over = pos && validTargets(drag.id).includes(pos) ? pos : null;
  }
  function chipUp(e) {
    if (!drag) return;
    const d = drag; drag = null; over = null;
    if (!d.moved) { showToast('Zum Wechseln den Chip auf eine Karte ziehen'); return; }
    const pos = slotUnder(e.clientX, e.clientY);
    if (!pos) return;
    if (!validTargets(d.id).includes(pos)) {
      showToast(d.id === st.libero ? 'Die Libera kann nur hinten stehen: V, VI und I bei gegnerischem Aufschlag' : liberoCard && d.id === liberoCard.replaced ? `${byId[d.id]?.number} gehört zu Position ${ROMAN[liberoCard.pos - 1]}` : 'Hier nicht ablegbar');
      return;
    }
    dropOn(d.id, pos);
  }
  function slotTap(pid) {
    selected = selected === pid ? null : pid;
  }
  function dropOn(chipId, pos) {
    extrasOpen = false;
    const slot = court[pos - 1];
    const target = slot.replaced || slot.id;   // the player who actually holds the position
    if (chipId === st.libero) {
      if (pos === 1 && st.serving) { showToast('Auf I schlägt gerade unsere Spielerin auf, die Libera kann dort erst bei gegnerischem Aufschlag stehen'); return; }
      if (pos !== 1 && pos !== 5 && pos !== 6) { showToast('Die Libera kann nur hinten stehen: I, V oder VI'); return; }
      queue({ skill: 'lib', sub_out: target, sub_in: st.libero });
      showToast(`Libera für ${byId[target]?.number} ${firstName(byId[target])}`);
      return;
    }
    if (liberoCard && chipId === liberoCard.replaced) {
      // the player the libero stands in for comes back onto her own card
      if (slot.replaced === chipId) { queue({ skill: 'lib', sub_out: null, sub_in: null }); showToast(`Libera raus, ${byId[chipId]?.number} ${firstName(byId[chipId])} wieder auf ${ROMAN[pos - 1]}`); }
      else showToast(`${byId[chipId]?.number} gehört zu Position ${ROMAN[liberoCard.pos - 1]}, dort ablegen`);
      return;
    }
    if (target === chipId) return;
    queue({ skill: 'sub', sub_out: target, sub_in: chipId });
    showToast(`Wechsel ${byId[target]?.number} → ${byId[chipId]?.number}`);
  }
  function undo() {
    if (!actionsAll.length || !canScout) return;
    if (!canWrite) { explainNoWrite(); return; }
    const ctx = ctxNow();
    serial(async () => {
      const { r, es, live } = await resolve(ctx);
      // undoing on a device without its own recording starts one (a copy or fresh)
      if (!r) { await startRecording(ctx, { undo: true }); return; }
      const acts = fold(r.base, es).actions;
      const a = acts[acts.length - 1];
      if (!a) return;
      await commit(ctx, r, es, { op: 'undo', seq: a.seq }, live ? 'Rückgängig: ' + describe(a) : null);
    });
  }

  function keys(e) {
    if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT') return;
    if (e.key >= '1' && e.key <= '6' && court.length) { selected = court[+e.key - 1].id; }
    if (e.key === 'z' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); undo(); }
    if (e.key === 'Escape') { selected = null; }
  }
</script>

<svelte:window onkeydown={keys} />
<svelte:head><title>Sideout — Live{match ? ` · ${match.opponent}` : ''}</title></svelte:head>

<main class="page live-page">
  {#if loadError}
    <div class="panel empty">{loadError}</div>
  {:else if !match}
    <div class="panel empty">Lade Spiel…</div>
  {:else if !hasLineup}
    <div class="panel">
      <h2>Aufstellung fehlt</h2>
      <p class="muted">Ohne Startaufstellung kann nicht gescoutet werden.</p>
      <a class="btn primary big" href="{base}/spiele/{id}">Aufstellung eintragen</a>
    </div>
  {:else}
    <div class="live" class:pv-stats={phoneView === 'stats'} class:extras={extrasOpen} class:vm={voiceMode}>
      <div class="col left">
        <section class="score">
          <button class="board-btn left" class:on={phoneView === 'stats'} onclick={() => (phoneView = phoneView === 'stats' ? 'pad' : 'stats')} title="Live-Werte"><svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M4 20h16v-2H4v2zm2-4h3V7H6v9zm5 0h3V4h-3v12zm5 0h3v-6h-3v6z"/></svg></button>
          <button class="board-btn right" class:on={extrasOpen} onclick={() => (extrasOpen = !extrasOpen)} title="Nachtragen: Spielstand, Rotation, Aufschlag"><svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zm17.71-10.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z"/></svg></button>
          <div class="side us"><img class="serve" class:on={st.serving} src="{base}/icon.svg" alt="" /><span class="team">{$me?.team?.short || 'Wir'}</span><span class="pts">{st.us}</span></div>
          <div class="colon">:</div>
          <div class="side them"><span class="pts">{st.them}</span><span class="team">{match.opponent.split(' ')[0]}</span><img class="serve" class:on={!st.serving} src="{base}/icon.svg" alt="" /></div>
          <div class="meta">
            <span>Satz <b>{st.set}</b></span><span>Sätze <b>{st.sets_won}:{st.sets_lost}</b></span>
            {#if st.sets.length}<span><b>{st.sets.map((s) => s.us + ':' + s.them).join('  ')}</b></span>{/if}
            <span>{st.serving ? 'Aufschlag' : 'Annahme'} · Rally <b>{st.rally}</b></span>
          </div>
          {#if canScout && !st.finished}
            <div class="catch" title="Nachtragen: Spielstand, Rotation und Aufschlag ohne Aktionen angleichen">
              <button disabled={tossPending || !canWrite} onclick={() => queue({ skill: 'adj', grade: '#' })}>+1 wir</button>
              <button disabled={tossPending || !canWrite} onclick={() => queue({ skill: 'adj', grade: '=' })}>+1 Gegner</button>
              <button disabled={!canWrite} onclick={() => queue({ skill: 'rot' })}>⟳ Rotieren</button>
              <button disabled={!canWrite} onclick={() => queue({ skill: 'srv', grade: st.serving ? '=' : '#' })} title="Aufschlagrecht wechseln">⇄ Aufschlag</button>
            </div>
          {/if}
        </section>
        {#if canScout && !$sync.supported}
          <div class="panel hint scoutbar" role="status"><span class="hint-txt">{$sync.unsupportedReason}</span></div>
        {:else if canScout && !tabOwner}
          <div class="panel hint scoutbar" role="status"><span class="hint-txt">Dieses Spiel wird in einem anderen Tab dieses Browsers gescoutet.</span></div>
        {/if}

        {#if tossPending}
          <div class="panel hint toss">Satz 5, neue Auslosung. Wer schlägt auf?
            <button class="btn" disabled={!canWrite} onclick={() => queue({ skill: 'srv', grade: '#' })}>Wir</button>
            <button class="btn" disabled={!canWrite} onclick={() => queue({ skill: 'srv', grade: '=' })}>Gegner</button>
          </div>
        {/if}
        <section class="court-wrap">
          <Court {court} {byId} {selected} serving={st.serving} {suggested} {over} dragging={!!drag} {targets} onselect={slotTap} />
          {#if benchChips.length}
            <div class="benchwrap">
            {#if pages.length > 1}<button class="bscroll l" disabled={bpage === 0} onclick={() => (bpage = Math.max(0, bpage - 1))} title="Bank: vorherige">‹</button>{/if}
            <div class="bench" bind:this={benchEl} onpointermove={chipMove} onpointerup={chipUp} onpointercancel={chipCancel}>
              <div class="benchrow" style="transform: translateX(-{benchShift}px)">
              {#each benchChips as p, k (p.id)}
                <button class="chipb" class:offpage={k < pageRange.first || k >= pageRange.last} class:libero={p.id === st.libero} class:out={liberoCard?.replaced === p.id} class:dragging={drag?.id === p.id} onpointerdown={(e) => chipDown(e, p.id)} disabled={!canScout || st.finished}>
                  <b>{p.number}</b><span>{firstName(p)}</span><small>{chipNote(p)}</small>
                </button>
              {/each}
              </div>
            </div>
            {#if pages.length > 1}<button class="bscroll r" disabled={bpage >= pages.length - 1} onclick={() => (bpage = Math.min(pages.length - 1, bpage + 1))} title="Bank: nächste">›</button>{/if}
            </div>
          {/if}
          {#if drag?.moved}
            {@const gp = byId[drag.id]}
            <div class="dragghost" class:libero={drag.id === st.libero} style="left:{drag.x}px; top:{drag.y}px"><b>{gp?.number}</b><span>{firstName(gp)}</span></div>
          {/if}
          {#if selected && !drag}
            <div class="court-foot">
              <span class="chip pos-{byId[selected]?.position}">{byId[selected]?.number} {firstName(byId[selected])}</span><span>ausgewählt, jetzt Aktion tippen</span>
            </div>
          {/if}
        </section>
        <section class="panel last" id="lastBox">
          <div class="txt">{last ? 'Zuletzt: ' + describe(last) : 'Noch keine Aktion.'}{#if syncText}<span class:pending={pendingHere > 0} class:okt={!pendingHere}> · {syncText}</span>{/if}</div>
          <button class="btn" onclick={undo} disabled={!last || !canScout || !canWrite}><svg class="ico-undo" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M12.5 8c-2.65 0-5.05.99-6.9 2.6L2 7v9h9l-3.62-3.62c1.39-1.16 3.16-1.88 5.12-1.88 3.54 0 6.55 2.31 7.6 5.5l2.37-.78C21.08 11.03 17.15 8 12.5 8z"/></svg> Rückgängig</button>
        </section>
      </div>

      <div class="col mid">
        <Pad {expected} idle={!selected} disabled={!canScout || !canWrite || st.finished || tossPending} ontap={tapCell} />
        <div class="pad-foot" class:has-mic={canScout}>
          <button class="btn big us" disabled={!canScout || !canWrite || st.finished || tossPending} onclick={() => queue({ skill: 'opp', grade: '=' })}>Fehler Gegner <span class="muted">+1 wir</span></button>
          {#if canScout}
            <button class="btn big mic" class:on={voiceMode} onclick={toggleVoiceMode} title={voiceMode ? 'Zurück zum Pad' : 'Sprache'} aria-label={voiceMode ? 'Zurück zum Pad' : 'Sprache'}>
              {#if voiceMode}<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><path fill="currentColor" d="M3 3h8v8H3V3zm10 0h8v8h-8V3zM3 13h8v8H3v-8zm10 0h8v8h-8v-8z"/></svg>
              {:else}<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><path fill="currentColor" d="M12 14a3 3 0 0 0 3-3V5a3 3 0 0 0-6 0v6a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.92V21h2v-3.08A7 7 0 0 0 19 11h-2z"/></svg>{/if}
            </button>
          {/if}
          <button class="btn big them" disabled={!canScout || !canWrite || st.finished || tossPending} onclick={() => queue({ skill: 'opp', grade: '#' })}>Punkt Gegner</button>
        </div>
        {#if canScout}
          <Voice bind:this={voice} players={match.players} onCourt={court.map((c) => c.id)} disabled={!canWrite || st.finished || tossPending} onaction={queue} />
        {/if}
        {#if st.finished}
          <div class="panel done">
            <h2>Spiel beendet {st.sets_won}:{st.sets_lost}</h2>
            <p class="muted">{st.sets.map((s) => s.us + ':' + s.them).join('   ')}</p>
            <a class="btn primary" href="{base}/auswertung/{id}">Zur Auswertung</a>
          </div>
        {/if}
      </div>

      <div class="col right">
        <section class="panel stats-panel">
          <div class="panel-head"><h2>{scope === 'set' ? 'Dieser Satz' : 'Ganzes Spiel'}</h2>
            <span class="tabs"><button class:active={scope === 'set'} onclick={() => (scope = 'set')}>Satz</button><button class:active={scope === 'match'} onclick={() => (scope = 'match')}>Spiel</button></span></div>
          {#if stat}
            {@const t = stat.team}
            {@const A = stat.players.reduce((a, p) => ({ k: a.k + p.A.k, e: a.e + p.A.e + p.A.blk, n: a.n + p.A.n }), { k: 0, e: 0, n: 0 })}
            <div class="tiles three">
              <div class="tile"><div class="v">{t.sideout.n ? Math.round((t.sideout.won / t.sideout.n) * 100) : '–'}<small>%</small></div><div class="k">Sideout</div><div class="d">{t.sideout.won}/{t.sideout.n} Annahme-Rallys</div></div>
              <div class="tile"><div class="v">{t.brk.n ? Math.round((t.brk.won / t.brk.n) * 100) : '–'}<small>%</small></div><div class="k">Break</div><div class="d">{t.brk.won}/{t.brk.n} Aufschlag-Rallys</div></div>
              <div class="tile"><div class="v">{A.n ? eff((A.k - A.e) / A.n) : '–'}</div><div class="k">Angriff Eff.</div><div class="d">{A.k} Pkt · {A.e} Fehler · {A.n} Vers.</div></div>
            </div>
            <table class="mini">
              <thead><tr><th class="l">Spielerin</th><th>Pkt</th><th>Ang</th><th>Eff</th><th>Ann Ø</th><th>Blo</th><th>Abw</th></tr></thead>
              <tbody>
                {#each stat.players as p (p.id)}
                  <tr><td class="l">{p.number}<small>{firstName(p)}</small></td><td>{p.pts}</td><td>{p.A.k}/{p.A.n}</td><td>{eff(p.A.pct)}</td><td>{fix(p.R.avg)}</td><td>{p.B.pts}</td><td>{p.D.good}/{p.D.n}</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </section>
      </div>
      <section class="panel timeline">
        <div class="tl-head"><h2>Verlauf</h2><span class="small muted">{actionsAll.length} Aktionen{#if syncText} · {syncText}{/if}</span><span class="spacer"></span><a class="small" href="{base}/auswertung/{id}">Auswertung →</a></div>
        <button class="tl-undo" onclick={undo} disabled={!last || !canScout || !canWrite} title={last ? 'Rückgängig: ' + describe(last) : 'Nichts zum Rückgängigmachen'}><svg class="ico-undo" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M12.5 8c-2.65 0-5.05.99-6.9 2.6L2 7v9h9l-3.62-3.62c1.39-1.16 3.16-1.88 5.12-1.88 3.54 0 6.55 2.31 7.6 5.5l2.37-.78C21.08 11.03 17.15 8 12.5 8z"/></svg>{#if pendingHere}<i>{pendingHere}</i>{/if}</button>
        <div class="tl" bind:this={tlEl}>
          {#each timeline as e (e.key)}
            {#if e.t === 'set'}
              <div class="tl-set">Satz {e.n}</div>
            {:else if e.t === 'score'}
              <div class="tl-score" class:us={e.won} class:them={!e.won}>{e.us}:{e.them}</div>
            {:else if e.r.skill === 'opp'}
              <div class="tl-item opp"><b>{e.r.grade === '=' ? '✕' : '●'}</b><small>Gegner</small></div>
            {:else if e.r.skill === 'adj'}
              <div class="tl-item sub"><b>+1</b><small>{e.r.grade === '#' ? 'wir' : 'Gegner'}</small></div>
            {:else if e.r.skill === 'rot'}
              <div class="tl-item sub"><b>⟳</b><small>rotiert</small></div>
            {:else if e.r.skill === 'srv'}
              <div class="tl-item sub"><b>S</b><small>{e.r.grade === '#' ? 'wir' : 'Gegner'}</small></div>
            {:else if e.r.skill === 'sub'}
              <div class="tl-item sub"><b>⇄</b><small>{byId[e.r.sub_out]?.number}→{byId[e.r.sub_in]?.number}</small></div>
            {:else if e.r.skill === 'lib'}
              <div class="tl-item sub"><b>L</b><small>{e.r.sub_in == null ? 'raus' : e.r.sub_out ? 'für ' + byId[e.r.sub_out]?.number : 'auto'}</small></div>
            {:else}
              <div class="tl-item" style="background: var(--{GRADE_CLASS[e.r.grade]}); color: var(--{GRADE_CLASS[e.r.grade]}-ink)"><b>{byId[e.r.player_id]?.number}</b><small>{SKILL[e.r.skill].short} {e.r.grade}</small></div>
            {/if}
          {:else}
            <div class="muted small">Noch keine Aktion. Der Verlauf wächst nach rechts.</div>
          {/each}
        </div>
      </section>
    </div>
  {/if}
</main>



<style>
  .live { display: grid; gap: 12px; grid-template-columns: 1fr; }
  /* tablet, portrait or landscape (760–1099px): left column = score, court,
     undo, then the live stats; the pad spans the right side; the timeline
     runs across the bottom. Natural heights, page scrolls if it must. */
  .col { display: grid; gap: 12px; align-content: start; min-width: 0; }
  .timeline { min-width: 0; }
  :global(.score .catch) { grid-column: 1 / -1; display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; margin-top: 4px; }
  :global(.score .catch button) { height: 30px; border-radius: 6px; border: 1px dashed var(--line); background: transparent; color: var(--ink-2); font-size: 12px; font-weight: 600; cursor: pointer; }
  :global(.score .catch button:hover) { background: var(--raised); color: var(--ink); }
  /* desktop: one screen. Row 1 = three cards of equal height (each scrolls
     inside if it must), row 2 = the timeline across the full width. */
  /* desktop: the row takes its natural height (the taller of the left and
     middle columns), the three cards stretch to that height and no further;
     the stats card scrolls inside. Wide screens get a wider page, not
     taller cards. */
  @media (min-width: 1000px) {
    .live-page { padding-bottom: 16px; max-width: 1500px; }
    .live { grid-template-columns: clamp(300px, 25%, 360px) minmax(0, 1fr) clamp(300px, 27%, 380px); grid-template-rows: auto auto; align-items: stretch; }
    .col { display: flex; flex-direction: column; min-height: 0; }
    .col > :global(*) { flex: 0 0 auto; }
    .col.left .court-wrap { flex: 1 1 auto; display: flex; flex-direction: column; min-height: 0; }
    .col.left :global(.catch) { grid-column: 1 / -1; }
    .col.left .court-wrap :global(.court) { flex: 1 1 auto; min-height: 250px; }
    .col.mid :global(.pad) { flex: 1 1 auto; grid-template-rows: auto repeat(6, minmax(64px, 1fr)); }
    .col.mid .pad-foot.has-mic { grid-template-columns: 1fr 1fr; }
    .col.mid .pad-foot .btn.mic { display: none; }
    .col.right .stats-panel { flex: 1 1 0; min-height: 0; overflow: auto; }
    .timeline { grid-column: 1 / -1; }
  }
  /* narrow three-column range: the four catch-up buttons as 2×2 */
  @media (min-width: 1000px) and (max-width: 1199px) {
    :global(.score .catch) { grid-template-columns: 1fr 1fr; }
  }
  /* desktop on a short screen (laptops at 700–760px, iPad landscape): tighter */
  @media (min-width: 1000px) and (max-height: 780px) {
    .live-page { padding-top: 10px; }
    .live { gap: 10px; } .col { gap: 8px; }
    :global(.score .pts) { font-size: 38px; }
    .col.left .court-wrap :global(.court) { min-height: 190px; }
    .col.left .court-wrap :global(.slot) { min-height: 0; }
    #lastBox { padding: 8px 12px; }
    .col.mid :global(.pad) { grid-template-rows: auto repeat(6, minmax(50px, 1fr)); }
    .col.mid :global(.pad .cell) { min-height: 0; }
    .col.mid :global(.pad .cell b) { font-size: 17px; }
    .timeline { padding: 4px 10px 4px; }
    .tl-item { width: 42px; padding: 3px 0 2px; } .tl-item b { font-size: 15px; }
    .tl-head h2 { font-size: 13px; } .tl-head { margin-bottom: 2px; }
  }
  /* timeline */
  .timeline { padding: 8px 12px 6px; }
  .tl-head { display: flex; align-items: baseline; gap: 10px; margin-bottom: 6px; }
  .tl-head h2 { font-size: 15px; }
  .tl { display: flex; align-items: center; gap: 4px; overflow-x: auto; padding: 6px 2px 6px; scrollbar-width: thin; }
  .tl-item { flex: 0 0 auto; display: grid; justify-items: center; gap: 2px; width: 50px; padding: 5px 0 4px; border-radius: 7px; background: var(--raised); color: var(--ink-2); }
  .tl-item b { font-family: var(--disp); font-size: 19px; font-weight: 700; line-height: 1; }
  .tl-item small { font-size: 9px; line-height: 1; white-space: nowrap; opacity: 0.85; font-weight: 600; }
  .tl-item.opp { background: var(--overlay); color: var(--ink-2); }
  .tl-item.sub { background: var(--accent-soft); color: var(--accent-text); }
  .tl-score { flex: 0 0 auto; font-family: var(--disp); font-weight: 700; font-size: 13px; padding: 2px 7px; border-radius: 10px; margin: 0 3px; }
  .tl-score.us { background: var(--g-win); color: var(--g-win-ink); }
  .tl-score.them { background: var(--g-err); color: #fff; }
  .tl-set { flex: 0 0 auto; align-self: stretch; display: grid; align-items: center; padding: 0 8px 0 10px; margin: 0 4px; border-left: 2px solid var(--line); font-size: 11px; font-weight: 600; color: var(--ink-3); white-space: nowrap; }
  .court-wrap { background: var(--panel); border: 1px solid var(--line-soft); border-radius: var(--r-l); padding: 10px; }
  .court-foot { display: flex; align-items: center; gap: 8px; margin-top: 8px; font-size: 12px; color: var(--ink-2); min-height: 22px; }
  .benchwrap { display: flex; align-items: center; gap: 6px; margin-top: 8px; }
  .bench { position: relative; flex: 1 1 auto; min-width: 0; overflow: hidden; padding: 2px 0 4px; touch-action: none; }
  .benchrow { position: relative; display: flex; gap: 6px; width: max-content; }
  .chipb.offpage { visibility: hidden; }
  .bscroll { flex: 0 0 auto; width: 30px; height: 32px; border-radius: var(--r-m); border: 1px solid var(--line); background: var(--raised); color: var(--ink); font-size: 22px; line-height: 1; padding: 0 0 3px; cursor: pointer; user-select: none; -webkit-user-select: none; }
  .bscroll:disabled { opacity: 0.3; cursor: default; }
  .bscroll:not(:disabled):active { background: var(--accent); color: #fff; }
  .bench::-webkit-scrollbar { display: none; }
  .chipb {
    flex: 0 0 auto; display: grid; grid-template-columns: auto auto; align-items: baseline; column-gap: 5px;
    height: 40px; padding: 0 10px; border-radius: 20px; border: 1px solid var(--line); background: var(--raised);
    cursor: grab; touch-action: none; user-select: none; -webkit-user-select: none;
  }
  .chipb b { font-family: var(--disp); font-size: 18px; font-weight: 700; }
  .chipb span { font-size: 12px; color: var(--ink-2); }
  .chipb small { grid-column: 1 / -1; font-size: 10px; color: var(--ink-3); line-height: 1; margin-top: -2px; }
  .chipb.libero { border-color: var(--court-line); background: var(--court-soft); }
  .chipb.libero small { color: var(--court-line); }
  .chipb.out { border-style: dashed; }
  .chipb.dragging { opacity: 0.35; }
  .dragghost {
    position: fixed; z-index: 50; transform: translate(-50%, -50%) scale(1.1); pointer-events: none;
    display: grid; grid-template-columns: auto auto; align-items: baseline; column-gap: 6px;
    height: 42px; padding: 0 14px; border-radius: 21px; border: 1px solid var(--accent);
    background: var(--panel); color: var(--ink); box-shadow: 0 12px 32px #000b, 0 0 0 3px var(--accent-soft);
  }
  .dragghost b { font-family: var(--disp); font-size: 19px; font-weight: 700; }
  .dragghost span { font-size: 13px; color: var(--ink-2); }
  .dragghost.libero { border-color: var(--court-line); box-shadow: 0 12px 32px #000b, 0 0 0 3px var(--court-soft); }
  .dragghost.libero span { color: var(--court-line); }
  .chipb:disabled { opacity: 0.4; cursor: default; }
  .btn.sm { height: 28px; padding: 0 10px; font-size: 12px; }
  .hint { padding: 8px 12px; font-size: 13px; color: var(--ink-2); }
  .scoutbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; border-color: var(--accent); background: var(--accent-soft); color: var(--ink); }
  .scoutbar .hint-txt { flex: 1 1 180px; min-width: 0; white-space: normal; overflow-wrap: anywhere; line-height: 1.3; }
  .scoutbar .btn { flex: 0 0 auto; height: 34px; padding: 0 12px; }
  .pad-foot { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .pad-foot .btn { justify-content: center; }
  .pad-foot .btn.us { border-color: var(--g-win); color: var(--g-win); }
  .pad-foot .btn.them { border-color: var(--g-err); color: var(--g-err); }
  .pad-foot.has-mic { grid-template-columns: 1fr auto 1fr; }
  .pad-foot .btn.mic { width: 56px; padding: 0; color: var(--ink-2); }
  .pad-foot .btn.mic.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
  .done { text-align: center; display: grid; gap: 8px; justify-items: center; }
  .last { display: flex; align-items: center; gap: 10px; }
  .last .txt { flex: 1; font-size: 13px; color: var(--ink-2); min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pending { color: var(--g-neg); }
  .okt { color: var(--ink-3); }


  .tiles.three { grid-template-columns: repeat(3, 1fr); }
  .mini { width: 100%; border-collapse: collapse; font-size: 13px; margin-top: 12px; }
  .mini th, .mini td { padding: 5px 4px; text-align: right; white-space: nowrap; }
  .mini th { color: var(--ink-3); font-weight: 600; font-size: 11px; border-bottom: 1px solid var(--line-soft); }
  .mini td.l, .mini th.l { text-align: left; }
  .mini td.l { font-weight: 600; }
  .mini td.l small { color: var(--ink-3); font-weight: 500; margin-left: 4px; }
  .board-btn { display: none; }
  .tl-undo { display: none; }
  .ico-undo { display: inline-block; vertical-align: -3px; }
  /* phone: one screen. Top bar hidden (tab bar navigates), score strip with
     Feld/Werte toggle, court, pad, opponent buttons; the timeline is docked
     above the tab bar with the undo button; bench and catch-up fold away. */
  @media (max-width: 759px) {
    /* the live view owns the whole viewport between the top inset and the
       tab bar: a flex column where the pad absorbs the remaining height, so
       it fits any display without scrolling; the two opponent buttons are
       pinned to the bottom edge */
    .live-page { position: fixed; top: 0; left: 0; right: 0; bottom: var(--tabbar-h); padding: calc(6px + env(safe-area-inset-top)) 8px 58px; overflow: hidden; display: flex; flex-direction: column; }
    .live { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; gap: 6px; }
    .col { display: contents; }
    .col.left > :global(.score), .timeline, .col.left > .hint, .col.left > .court-wrap { flex: 0 0 auto; }
    /* court and pad share the remaining height 3:4; everything else scales
       its type with the viewport height */
    .col.left > .court-wrap { flex: 3 1 0; min-height: 0; overflow: hidden; display: flex; flex-direction: column; }
    .col.left > .court-wrap :global(.court) { flex: 1 1 auto; min-height: 0; }
    .col.left > .court-wrap :global(.slot) { min-height: 0; }
    .col.left > .court-wrap :global(.slot .jersey) { font-size: clamp(18px, 3.4dvh, 34px); }
    .col.left > .court-wrap :global(.slot .nm) { font-size: clamp(9px, 1.4dvh, 12px); }
    .col.left > .court-wrap .benchwrap { flex: 0 0 auto; }
    .chipb { height: clamp(30px, 4.6dvh, 44px); }
    .chipb b { font-size: clamp(14px, 2.2dvh, 19px); }
    .col.mid > :global(.pad) { flex: 4 1 0; min-height: 0; overflow: hidden; grid-template-rows: auto repeat(6, minmax(0, 1fr)); }
    .col.mid > :global(.pad .cell) { min-height: 0; }
    .col.mid > :global(.pad .cell b) { font-size: clamp(12px, 2.2dvh, 18px); }
    .col.mid > :global(.pad .cell span) { font-size: clamp(8px, 1.15dvh, 10px); }
    .col.mid > :global(.pad .prow .lbl) { font-size: clamp(10px, 1.5dvh, 12px); }
    .col.mid > .pad-foot { position: absolute; left: 8px; right: 8px; bottom: 6px; }
    .col.right > .stats-panel { flex: 1 1 0; min-height: 0; overflow: auto; }
    .col.left > :global(.score) { order: 1; }
    .timeline { order: 2; }
    .col.left > .hint { order: 3; }
    .col.left > .court-wrap { order: 4; }
    .col.mid > :global(.pad) { order: 5; }
    /* same vertical padding as the pad: with flex-basis 0 the padding is added on top of the shared height, so a thicker card would steal from the court */
    .col.mid > :global(.voice) { order: 5; flex: 4 1 0; min-height: 0; overflow: auto; display: none; padding: 6px 10px; }
    .live.vm .col.mid > :global(.voice) { display: grid; }
    .live.vm .col.mid > :global(.pad) { display: none; }
    .col.mid > .pad-foot { order: 6; }
    .col.mid > .done { order: 7; }
    .col.right > .stats-panel { order: 8; }
    /* the board carries two icon buttons: live stats (left), catch-up (right) */
    .board-btn { display: grid; place-items: center; position: absolute; top: 8px; width: 32px; height: 32px; border-radius: 8px; border: 1px solid var(--line-soft); background: var(--raised); color: var(--ink-2); z-index: 1; }
    .board-btn.left { left: 8px; }
    .board-btn.right { right: 8px; }
    .board-btn.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
    .hint-txt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    :global(.score .catch) { display: none; grid-template-columns: 1fr 1fr 1fr 1.25fr; margin-top: 8px; }
    :global(.score .catch button) { font-size: 11px; padding: 0 2px; white-space: nowrap; }
    .live.extras :global(.score .catch) { display: grid; }
    .benchwrap { margin-top: 6px; }
    .chipb { height: 38px; }
    .court-wrap { padding: 6px; }
    .court-foot { margin-top: 4px; min-height: 18px; font-size: 11px; }
    .pad-foot .btn { height: 44px; font-size: 15px; }
    .pad-foot.has-mic .btn .muted { display: none; }
    #lastBox { display: none; }
    .col.right > .stats-panel { display: none; }
    .live.pv-stats .col.right > .stats-panel { display: block; }
    .live.pv-stats .col.left > .court-wrap, .live.pv-stats .col.mid > :global(.pad), .live.pv-stats .col.mid > :global(.voice), .live.pv-stats .col.mid > .pad-foot { display: none; }
    .timeline { display: flex; align-items: center; gap: 6px; padding: 6px 8px; }
    .tl-head { display: none; }
    /* undo sits at the right, next to the newest entry, and stays small */
    .tl-undo { display: grid; place-items: center; position: relative; flex: 0 0 auto; order: 2; width: 36px; height: 36px; border-radius: 8px; border: 1px solid var(--line); background: var(--raised); }
    .tl-undo .ico-undo { width: 16px; height: 16px; }
    .tl { order: 1; }
    .tl-undo:disabled { opacity: 0.4; }
    .tl-undo i { position: absolute; top: -5px; right: -5px; min-width: 16px; height: 16px; border-radius: 8px; background: var(--g-neg); color: var(--g-neg-ink); font-size: 10px; font-style: normal; font-weight: 700; display: grid; place-items: center; padding: 0 3px; }
    .tl { flex: 1 1 auto; min-width: 0; padding: 2px 0; gap: 4px; }
    .tl-item { width: clamp(40px, 6dvh, 56px); padding: clamp(3px, 0.5dvh, 6px) 0; }
    .tl-item b { font-size: clamp(14px, 2.3dvh, 21px); }
    .tl-item small { font-size: clamp(8px, 1.1dvh, 10px); }
    .tl-score { font-size: clamp(11px, 1.6dvh, 14px); padding: 1px 6px; }
    .tl-undo { width: clamp(30px, 4.6dvh, 42px); height: clamp(30px, 4.6dvh, 42px); }
  }
  @media (max-width: 759px) and (max-height: 700px) {
    :global(.score .meta) { display: none; }
    .board-btn { top: 6px; }
    .pad-foot .btn { height: 36px; font-size: 14px; }
    .live-page { padding-top: calc(4px + env(safe-area-inset-top)); }
    .col.mid > :global(.pad) { grid-template-rows: repeat(6, minmax(0, 1fr)); }
  }
  @media (max-width: 759px) {
    :global(.score) { position: relative; padding: clamp(4px, 1dvh, 10px) 46px clamp(4px, 1dvh, 10px); row-gap: 2px; }
    :global(.score .pts) { font-size: clamp(30px, 5.2dvh, 52px); }
    :global(.score .colon) { font-size: clamp(20px, 3.4dvh, 32px); }
    :global(.score .meta) { gap: 10px; font-size: clamp(11px, 1.5dvh, 13px); }
    :global(.score .team) { font-size: clamp(12px, 1.6dvh, 14px); }
    .board-btn { width: clamp(28px, 4dvh, 36px); height: clamp(28px, 4dvh, 36px); }
  }
  /* wide but short (a phone held sideways, up to 1099px wide and 540px
     tall): two columns. Left: score, timeline, court + bench. Right: the pad
     with the two buttons under it. Stats replace the pad on demand. */
  @media (max-width: 1099px) and (max-height: 540px) and (orientation: landscape) {
    .live-page { position: fixed; top: 0; left: 0; right: 0; bottom: var(--tabbar-h); padding: calc(4px + env(safe-area-inset-top)) calc(8px + env(safe-area-inset-right)) 4px calc(8px + env(safe-area-inset-left)); overflow: hidden; display: flex; flex-direction: column; max-width: none; }
    .live { flex: 1 1 auto; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr); grid-template-rows: auto auto minmax(0, 1fr) auto; gap: 6px; height: auto; align-items: stretch; }
    .col { display: contents; }
    .col.left > :global(.score) { grid-column: 1; grid-row: 1; position: relative; padding: 4px 42px; row-gap: 0; }
    :global(.score .pts) { font-size: clamp(24px, 7dvh, 36px); }
    :global(.score .colon) { font-size: clamp(16px, 5dvh, 26px); }
    :global(.score .meta) { display: none; }
    .board-btn { display: grid; place-items: center; position: absolute; top: 5px; width: 28px; height: 28px; border-radius: 8px; border: 1px solid var(--line-soft); background: var(--raised); color: var(--ink-2); z-index: 1; }
    .board-btn.left { left: 6px; } .board-btn.right { right: 6px; }
    .board-btn.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
    :global(.score .catch) { display: none; margin-top: 4px; grid-template-columns: 1fr 1fr 1fr 1.25fr; }
    :global(.score .catch button) { height: 26px; font-size: 11px; padding: 0 2px; white-space: nowrap; }
    .live.extras :global(.score .catch) { display: grid; }
    .timeline { grid-column: 1; grid-row: 2; display: flex; align-items: center; gap: 6px; padding: 4px 8px; min-width: 0; }
    .tl-head { display: none; }
    .tl { flex: 1 1 auto; min-width: 0; padding: 2px 0; gap: 3px; order: 1; }
    .tl-item { width: 38px; padding: 3px 0 2px; } .tl-item b { font-size: 13px; } .tl-item small { font-size: 8px; }
    .tl-score { font-size: 11px; padding: 1px 5px; }
    .tl-undo { display: grid; place-items: center; position: relative; order: 2; flex: 0 0 auto; width: 30px; height: 30px; border-radius: 8px; border: 1px solid var(--line); background: var(--raised); }
    .tl-undo .ico-undo { width: 14px; height: 14px; }
    .col.left > .hint { grid-column: 1; grid-row: 3; }
    .col.left > .court-wrap { grid-column: 1; grid-row: 3 / span 2; min-height: 0; overflow: hidden; display: flex; flex-direction: column; padding: 6px; }
    .col.left > .court-wrap :global(.court) { flex: 1 1 auto; min-height: 0; padding: 4px; gap: 4px; border-top-width: 4px; }
    .col.left > .court-wrap :global(.court::before) { display: none; }
    .col.left > .court-wrap :global(.rowp) { gap: 4px; }
    .col.left > .court-wrap :global(.slot) { min-height: 0; padding: 2px 5px; }
    .col.left > .court-wrap :global(.slot .jersey) { font-size: clamp(14px, 5dvh, 24px); }
    .col.left > .court-wrap :global(.slot .nm) { display: none; }
    .benchwrap { flex: 0 0 auto; margin-top: 4px; }
    .chipb { height: 30px; } .chipb b { font-size: 14px; } .chipb small { display: none; }
    #lastBox { display: none; }
    .col.mid > :global(.pad) { grid-column: 2; grid-row: 1 / span 3; min-height: 0; overflow: hidden; padding: 6px; gap: 3px; grid-template-rows: repeat(6, minmax(0, 1fr)); }
    .col.mid > :global(.pad .pad-head) { display: none; }
    .col.mid > :global(.pad .prow) { gap: 3px; }
    .col.mid > :global(.pad .cell) { min-height: 0; }
    .col.mid > :global(.pad .cell b) { font-size: clamp(12px, 4dvh, 18px); }
    .col.mid > :global(.pad .cell span) { display: none; }
    .col.mid > :global(.pad .prow .lbl) { font-size: 11px; }
    .col.mid > :global(.pad .prow .lbl small) { display: none; }
    .col.mid > .pad-foot { grid-column: 2; grid-row: 4; position: static; }
    .col.mid > :global(.voice) { grid-column: 2; grid-row: 1 / span 3; min-height: 0; overflow: auto; display: none; }
    .live.vm .col.mid > :global(.voice) { display: grid; }
    .live.vm .col.mid > :global(.pad) { display: none; }
    .pad-foot .btn { height: 34px; font-size: 13px; }
    .col.mid > .done { grid-column: 2; grid-row: 4; }
    .col.right > .stats-panel { display: none; }
    .live.pv-stats .col.right > .stats-panel { display: block; grid-column: 2; grid-row: 1 / span 4; min-height: 0; overflow: auto; }
    .live.pv-stats .col.mid > :global(.pad), .live.pv-stats .col.mid > :global(.voice), .live.pv-stats .col.mid > .pad-foot { display: none; }
  }
  /* tablet portrait (760–999px): like the phone, but two columns. Left:
     score with the two board icons, timeline, court + bench, undo. Right: the
     pad with its buttons, or the stats when the chart icon is on. */
  @media (min-width: 760px) and (max-width: 999px) and (min-height: 541px) {
    .live { display: grid; grid-template-columns: minmax(300px, 400px) minmax(0, 1fr); grid-template-rows: auto auto auto auto; align-items: start; gap: 12px; }
    /* rows: 1 score · 2 court · 3 undo · 4 timeline across both columns; the pad spans 1–2, its buttons sit in row 3 */
    .col { display: contents; }
    .col.left > :global(.score) { grid-column: 1; grid-row: 1; position: relative; padding: 8px 46px; }
    .board-btn { display: grid; place-items: center; position: absolute; top: 8px; width: 32px; height: 32px; border-radius: 8px; border: 1px solid var(--line-soft); background: var(--raised); color: var(--ink-2); z-index: 1; }
    .board-btn.left { left: 8px; } .board-btn.right { right: 8px; }
    .board-btn.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
    :global(.score .catch) { display: none; grid-template-columns: 1fr 1fr; margin-top: 8px; }
    .live.extras :global(.score .catch) { display: grid; }
    .timeline { grid-column: 1 / -1; grid-row: 4; display: flex; align-items: center; gap: 6px; padding: 6px 10px; min-width: 0; }
    .tl-head { display: none; }
    .tl { flex: 1 1 auto; min-width: 0; order: 1; padding: 2px 0; gap: 4px; }
    .tl-undo { display: none; }
    .col.left > .hint { grid-column: 1; grid-row: 2; }
    .col.left > .court-wrap { grid-column: 1; grid-row: 2; }
    #lastBox { grid-column: 1; grid-row: 3; }
    .col.mid > :global(.pad) { grid-column: 2; grid-row: 1 / span 2; grid-template-rows: auto repeat(6, minmax(58px, auto)); }
    .col.mid > .pad-foot { grid-column: 2; grid-row: 3; }
    .col.mid > :global(.voice) { grid-column: 2; grid-row: 1 / span 2; min-height: 0; overflow: auto; display: none; }
    .live.vm .col.mid > :global(.voice) { display: grid; }
    .live.vm .col.mid > :global(.pad) { display: none; }
    .col.mid > .done { grid-column: 2; grid-row: 3; }
    .col.right > .stats-panel { display: none; }
    .live.pv-stats .col.right > .stats-panel { display: block; grid-column: 2; grid-row: 1 / span 3; }
    .live.pv-stats .col.mid > :global(.pad), .live.pv-stats .col.mid > :global(.voice), .live.pv-stats .col.mid > .pad-foot { display: none; }
  }
</style>
