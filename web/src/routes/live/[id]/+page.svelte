<script>
  import { base } from '$app/paths';
  // The capture screen. Renders from confirmed actions + a local op queue,
  // so every tap shows instantly and survives a dead connection.
  import { page } from '$app/stores';
  import { untrack, tick } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, mutations, online, showToast } from '$lib/stores.js';
  import { replay, courtView, expectedSkills, stats, SKILL, PAD, GRADE_CLASS, ROMAN, firstName, eff, fix } from '$lib/engine.js';
  import { loadOps, saveOps, applyOps, cacheMatch, cachedMatch, newCid, reconcileOps, keepDropped, settleAfterLoss, saveLease, loadLease } from '$lib/offline.js';
  import { tabWriter } from '$lib/scoutlock.js';
  import Court from '$lib/components/Court.svelte';
  import Pad from '$lib/components/Pad.svelte';
  import Voice from '$lib/components/Voice.svelte';

  const id = $derived(Number($page.params.id));
  let match = $state(null);
  let ops = $state([]);
  let selected = $state(null);
  let scope = $state('set');
  let flushing = false;
  let inflight = null;      // the op whose request is on the wire
  let inflightFor = null;   // ... and the match it belongs to
  let gen = 0;              // bumps on every local change; a refresh started before it is stale
  // a page that was left (or switched to another match) must not touch the
  // queue any more: its timers are cancelled and every await checks `stale()`
  let alive = true;
  const stale = (mid) => !alive || mid !== id;
  $effect(() => () => { alive = false; clearTimeout(retryTimer); });
  let retryTimer = null;
  let retryDelay = 5000;
  let loadError = $state('');

  // ----- scouting ownership (one active writer per match) -----
  // `scout` is the caller-relative block of the last server answer; `lease`
  // is our lease as the server confirmed it for this session (or, right
  // after an offline start, the one this device held last). Writes need the
  // lease AND this tab being the writer tab in this browser. `needVerify`
  // marks a doubt (page start, a scout event, a reconnect): the next flush
  // asks the server first and only continues if the same lease still holds.
  let scout = $state(null);
  let lease = $state(null);
  let leaseRev = 0;
  let needVerify = true;
  let doubtRev = 0;              // the ownership revision the doubt came from; older answers do not settle it
  let acquiring = $state(false);
  let tabOwner = $state(false);
  const holderElse = $derived(!!scout?.held && !scout.stale && !scout.mine);
  const canScout = $derived(['coach', 'assistant'].includes($me?.user?.role));
  const canWrite = $derived(canScout && !!lease && tabOwner);
  // may this device acquire the match right now (free, stale, or already ours)?
  const claimable = $derived(canScout && tabOwner && !!scout && (!scout.held || scout.stale || scout.mine));
  const hhmm = (iso) => { const d = iso ? new Date(iso) : null; return d && !isNaN(d) ? d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' }) : '–'; };
  // persisted state (queue, lease, set-aside ops) belongs to the writer tab:
  // a watching tab renders from storage but must never write it back, or a
  // refresh that was pending in the watcher could overwrite the writer's
  // newer queue with its older copy
  const persistOps = (mid, o) => { if (tabOwner) saveOps(mid, o); };
  const persistLease = (mid, l) => { if (tabOwner) saveLease(mid, l); };
  const persistDropped = (mid, o) => { if (tabOwner) keepDropped(mid, o); };
  const REQ_KEY = (mid) => `so_scout_req_${mid}`;
  const loadReq = (mid) => { try { return JSON.parse(localStorage.getItem(REQ_KEY(mid)) || 'null'); } catch { return null; } };
  const saveReq = (mid, v) => { try { if (v) localStorage.setItem(REQ_KEY(mid), JSON.stringify(v)); else localStorage.removeItem(REQ_KEY(mid)); } catch { /* ignore */ } };
  const byId = $derived(Object.fromEntries((match?.players || []).map((p) => [p.id, p])));
  const cfg = $derived(match ? { first_serve_us: match.first_serve === 'us', lineups: match.lineups } : null);
  const actionsAll = $derived(match ? applyOps(match.actions, ops) : []);
  const st = $derived(cfg ? replay(cfg, actionsAll) : null);
  const hasLineup = $derived(!!match && Object.keys(match.lineups || {}).length > 0);
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
  const stat = $derived(st ? stats(cfg, match.players, actionsAll, scope === 'set' ? st.set : 0) : null);
  // timeline: every action left to right, a score pill after each rally, a divider per set
  const timeline = $derived.by(() => {
    if (!st) return [];
    const out = []; let set = 0;
    for (const r of st.rows) {
      if (r.set !== set) { set = r.set; out.push({ t: 'set', n: set, key: 'set' + set }); }
      out.push({ t: 'act', r, key: 'a' + r.seq });
      if (r.outcome) out.push({ t: 'score', us: r.us + (r.outcome === 'us' ? 1 : 0), them: r.them + (r.outcome === 'them' ? 1 : 0), won: r.outcome === 'us', key: 's' + r.seq });
    }
    return out;
  });
  let tlEl = $state(null);
  // newest entry stays in view
  $effect(() => { void timeline.length; const el = tlEl; if (el) tick().then(() => { el.scrollLeft = el.scrollWidth; }); });

  // `entry`: the page was just opened (or this tab just became the writer
  // tab). Only then does a free match get claimed on its own; a watching
  // device never grabs a match that was merely released, that takes a tap
  async function load(entry = false) {
    if (inflight && inflightFor === id) { needVerify = true; return; } // the answer on the wire settles the queue; the next send verifies
    const saved = loadOps(id);
    if (JSON.stringify(saved) !== JSON.stringify(ops)) ops = saved;
    const g = gen; const mid = id;
    try {
      const m = await api(`/matches/${mid}`);
      if (stale(mid)) return;
      // something changed locally while this answer was on its way (a tap,
      // a save): it is older than what we show, so drop it and ask again
      if (g !== gen) { load(); return; }
      adopt(m);
      loadError = '';
    } catch (e) {
      const c = cachedMatch(id);
      if (c) { match = c; scout = c.scout || scout; showToast('Offline: Stand vom letzten Laden'); }
      else loadError = e.offline ? 'Keine Verbindung und kein lokaler Stand.' : e.message;
    }
    if (entry && !lease) await maybeClaim();
    flush();
  }
  // a fresh server log: first the ownership question (is our lease still the
  // match's lease?), then the queue is settled against the log (see
  // offline.js): ops the server already holds are dropped, the rest
  // renumbered; foreign actions with unsent ops of ours are a conflict and
  // those ops are kept for the record.
  function adopt(m) {
    const sc = m.scout;
    if (sc && sc.revision >= leaseRev) {
      const lostFlag = loadLease(id)?.lost;
      if ((lease && !(sc.mine && sc.lease === lease)) || lostFlag) { ownershipLost(m); return; }
    }
    const knownTop = (match || cachedMatch(id))?.actions?.slice(-1)[0]?.seq || 0;
    const r = reconcileOps(m.actions, ops, knownTop);
    if (r.conflict) {
      persistDropped(id, r.dropped);
      showToast(`Ein anderes Gerät hat gescoutet: ${r.dropped.length} eigene Aktionen verworfen`, true);
    }
    ops = r.ops;
    gen++;
    persistOps(id, ops);
    match = m;
    cacheMatch(id, m);
    if (sc && sc.revision >= (scout?.revision ?? -1)) setScout(sc);
  }
  function setScout(sc) {
    if (scout && sc.revision < scout.revision) return; // older than what we know
    scout = sc;
    if (sc.mine && sc.lease) {
      lease = sc.lease; leaseRev = sc.revision;
      if (sc.revision >= doubtRev) needVerify = false; // an answer from before the doubt does not settle it
      persistLease(id, { lease, rev: leaseRev });
    } else if (lease && sc.revision >= leaseRev) {
      lease = null; persistLease(id, null);
    }
  }
  // the lease is gone (takeover, expiry, refusal): whatever the server
  // already holds is not lost; the rest may not be replayed under a new
  // lease and is kept aside for the record
  function ownershipLost(m) {
    const rest = settleAfterLoss(m.actions, ops);
    persistDropped(id, rest);
    const who = m.scout?.actor ? `${m.scout.actor} scoutet jetzt` : 'Das Scouting ist beendet';
    showToast(rest.length ? `${who}: ${rest.length} nicht gesendete Aktionen beiseitegelegt` : who, true);
    ops = []; gen++; persistOps(id, ops);
    lease = null; persistLease(id, null); needVerify = true;
    match = m; cacheMatch(id, m); scout = m.scout;
  }
  // conditional acquisition on entry when the match is free or stale (never
  // a takeover). A lost answer is resolved first: the request id is kept
  // until the server confirmed or refused it, so a retry returns the same
  // lease instead of a second ownership period.
  async function maybeClaim() {
    if (!claimable || acquiring || !$online || lease) return false;
    const confirmed = match && cfg ? replay(cfg, match.actions) : null;
    if (confirmed?.finished && !ops.length) return false; // viewing a finished match does not claim it
    return acquireLease('claim');
  }
  async function acquireLease(mode, retried = false) {
    if (acquiring || !alive || !tabOwner) return false;
    const mid = id;
    const pending = loadReq(mid);
    const req = pending?.req || newCid();
    saveReq(mid, { req, mode });
    acquiring = true;
    try {
      const m = await api(`/matches/${mid}/scout`, { method: 'POST', body: { mode, revision: scout?.revision ?? 0, request_id: req } });
      if (stale(mid)) return false;
      saveReq(mid, null);
      adopt(m);
      return !!lease;
    } catch (e) {
      if (stale(mid) || e.offline) return false; // resolved on the next load with the same request id
      saveReq(mid, null);
      if (e.code === 'scouted_elsewhere' || e.code === 'scout_lease_expired') {
        const cur = e.current?.scout;
        if (cur) scout = cur;
        // a claim refused only because the revision moved (a release landed
        // in between) is repeated once from the fresh revision; it is still
        // conditional, so a current holder is never displaced by it
        if (mode === 'claim' && !retried && cur && (!cur.held || cur.stale)) { acquiring = false; return acquireLease('claim', true); }
        if (mode === 'takeover') showToast('Inzwischen scoutet jemand anderes, Stand aktualisiert', true);
        load();
        return false;
      }
      showToast(e.message, true);
      return false;
    } finally {
      acquiring = false;
    }
  }
  async function takeover() {
    if (!canScout || !tabOwner || acquiring) return;
    if (await acquireLease('takeover')) showToast('Du scoutest jetzt');
  }
  // release only our own lease, only when nothing is pending; a late or
  // repeated release is a server-side no-op
  function releaseLease(keepalive = false) {
    const l = lease; const mid = id;
    // only the writer tab lets go; a watching tab of the same session must
    // not release the lease the writer tab is using. Without a connection
    // nothing can be released: the device stays the holder of record, so a
    // reload in the hall keeps capturing (the server holds the lease anyway)
    if (!l || !tabOwner || ops.length || inflight || !navigator.onLine) return;
    lease = null; persistLease(mid, null);
    api(`/matches/${mid}/scout?lease=${encodeURIComponent(l)}`, { method: 'DELETE', keepalive }).catch(() => {});
  }
  // ask the server whether our lease still holds before anything is sent
  async function verifyLease(mid) {
    try {
      const m = await api(`/matches/${mid}`);
      if (stale(mid)) return false;
      adopt(m);
      return !!lease;
    } catch (e) {
      if (!stale(mid) && !e.offline) showToast(e.message, true);
      return false;
    }
  }

  // effects only track their trigger; everything they call runs untracked,
  // otherwise a reload that rewrites `ops` would re-trigger itself forever
  $effect(() => {
    const mid = id;
    untrack(() => {
      match = null; ops = []; scout = null; clearTimeout(retryTimer);
      const saved = loadLease(mid);
      // the holder of record may capture offline; sending waits for
      // verification, and the first server answer is authoritative (revision
      // 0), whatever revision the stored lease was confirmed at
      lease = saved?.lease && !saved.lost ? saved.lease : null;
      leaseRev = 0;
      needVerify = true;
      load(true);
    });
    // the writer tab for this match in this browser; the other tabs watch
    const lock = tabWriter(mid, (held, info) => {
      tabOwner = held;
      if (!held) return;
      needVerify = true;
      untrack(() => {
        // the persisted queue is the truth, not this tab's copy of it
        if (info?.handoff) { const saved = loadOps(mid); if (JSON.stringify(saved) !== JSON.stringify(ops)) { ops = saved; gen++; } }
        // a hand-off from another tab of this browser: that tab may have
        // released the lease we restored from storage, so acquire afresh
        // (the server starts a new period for the same session)
        if (info?.handoff && $online) { lease = null; persistLease(mid, null); }
        if (!lease) maybeClaim();
        flush();
      });
    });
    // a hard departure (tab closed, address bar): the same best-effort release
    const onHide = () => untrack(() => releaseLease(true));
    window.addEventListener('pagehide', onHide);
    return () => {
      // leaving this match: release our lease if nothing is pending
      // (best-effort, keepalive), and stop being the writer tab
      window.removeEventListener('pagehide', onHide);
      untrack(() => releaseLease(true));
      lock.release();
    };
  });

  // another device wrote to this match → refetch when we have nothing pending
  $effect(() => {
    const m = $mutations;
    if (!m) return;
    untrack(() => {
      if (!match) return;
      const mine = (m.entity === 'action' || m.entity === 'match') && m.id === id;
      if (mine && m.action === 'scout') {
        // ownership may have moved (even our own claim produces this): the
        // event is a notification, the server answer is the truth. With work
        // pending or a request on the wire, the flush loop verifies before
        // the next send (or right after the answer); otherwise ask right away.
        needVerify = true; doubtRev = Math.max(doubtRev, m.version || 0);
        if (!flushing && !inflight) load();
        return;
      }
      if ((mine || m.entity === 'resync') && ops.length === 0) load();
      else if (m.entity === 'resync' && ops.length) { needVerify = true; flush(); } // the stream is back: connectivity too
    });
  });
  $effect(() => { if ($online) untrack(() => { needVerify = true; flush(); }); });

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

  // the write gate, enforced here and in flush, not only on buttons
  function explainNoWrite() {
    if (!canScout) return;
    if (!tabOwner) showToast('Dieses Spiel wird in einem anderen Tab gescoutet', true);
    else if (holderElse) showToast(`${scout.actor || 'Jemand'} scoutet gerade, erst übernehmen`, true);
    else if (!$online) showToast('Ohne Verbindung kann nur der bisherige Scouter weitertippen', true);
    else showToast('Scouting noch nicht übernommen', true);
  }
  function queue(a) {
    if (!canScout) return;
    if (!canWrite) {
      // no lease yet but the match is free: acquire, then record (never a takeover)
      if (claimable && $online && !acquiring) { acquireLease('claim').then((ok) => { if (ok) queue(a); else explainNoWrite(); }); return; }
      explainNoWrite();
      return;
    }
    if (st.finished) { showToast('Das Spiel ist beendet'); return; }
    const before = st;
    const seq = (actionsAll[actionsAll.length - 1]?.seq || 0) + 1;
    ops = [...ops, { type: 'add', action: { id: -seq, seq, cid: newCid(), grade: null, player_id: null, sub_out: null, sub_in: null, ...a } }];
    gen++;
    persistOps(id, ops);
    selected = null;
    const after = replay(cfg, applyOps(match.actions, ops));
    if (after.set !== before.set) { const s = after.sets[after.sets.length - 1]; showToast(`Satz ${before.set} beendet ${s.us}:${s.them}` + (after.finished || match.lineups?.[after.set] ? '' : ' · Aufstellung übernommen, Wechsel per Drag & Drop')); }
    else if (after.finished) showToast(`Spiel beendet ${after.sets_won}:${after.sets_lost}`);
    flush();
  }

  // send the queue in order. One op is on the wire at a time (`inflight`);
  // undo never removes that one from the queue, it queues a compensating
  // undo instead, so a request that completes after the tap still gets
  // undone. Errors: offline → wait; 5xx → retry with backoff; 409 → reload
  // and reconcile; 4xx → this op is refused for good, the rest is renumbered.
  async function flush() {
    if (flushing || !match || !alive || !tabOwner || !lease) return;
    flushing = true;
    clearTimeout(retryTimer);
    const mid = id;
    try {
      let conflicts = 0;
      while (ops.length && !stale(mid)) {
        if (needVerify) {
          // a doubt about ownership: settle it before sending anything
          const ok = await verifyLease(mid);
          if (stale(mid)) return;
          if (!ok) { if (lease) { retryTimer = setTimeout(flush, retryDelay); retryDelay = Math.min(retryDelay * 2, 60000); } break; }
          if (!ops.length) break; // the fresh log settled everything
        }
        const op = ops[0];
        inflight = op; inflightFor = mid;
        if (op.type === 'add' && !op.sent) { op.sent = true; persistOps(id, ops); } // from here on the server may hold it
        try {
          if (op.type === 'add') {
            const res = await api(`/matches/${mid}/actions`, { method: 'POST', body: op.action, lease });
            if (stale(mid)) return;
            match = { ...match, actions: [...match.actions.filter((x) => x.seq !== res.action.seq), res.action] };
            if (res.scout) setScout(res.scout);
          } else {
            const q = op.cid ? `cid=${encodeURIComponent(op.cid)}` : `seq=${op.seq}`;
            const res = await api(`/matches/${mid}/actions/last?${q}`, { method: 'DELETE', lease });
            if (stale(mid)) return;
            if (res.scout) setScout(res.scout);
            // a retry whose target is already gone answers removed:null; the
            // local log still holds the action, so take it out by target either way
            const gone = res.removed ? (x) => x.seq !== res.removed.seq : (x) => (op.cid ? x.cid !== op.cid : x.seq !== op.seq);
            match = { ...match, actions: match.actions.filter(gone) };
          }
          ops = ops.filter((o) => o !== op);
          gen++;
          persistOps(mid, ops);
          cacheMatch(id, match);
          retryDelay = 5000;
          conflicts = 0;
        } catch (e) {
          if (stale(mid)) return;
          if (e.offline) {
            // no answer: the online event flushes again, but the browser may
            // stay "online" while the server is unreachable, so retry anyway
            retryTimer = setTimeout(flush, retryDelay);
            retryDelay = Math.min(retryDelay * 2, 60000);
            break;
          }
          if (e.code === 'scouted_elsewhere' || e.code === 'scout_lease_expired') {
            // our lease is over: stop this writer, settle what the server
            // already holds, keep the rest aside — before any sequence logic
            const m = await api(`/matches/${mid}`).catch(() => null);
            if (stale(mid)) return;
            if (m) ownershipLost(m);
            else { persistLease(mid, { lost: true }); lease = null; scout = e.current?.scout || scout; } // settled on the next successful load
            break;
          }
          if (e.status === 409) {
            if (++conflicts > 2) { persistDropped(id, ops); ops = []; persistOps(id, ops); showToast('Konflikt mit dem Server, eigene Aktionen verworfen', true); break; }
            const m = await api(`/matches/${mid}`).catch(() => null);
            if (stale(mid)) return;
            if (!m) { // the recovery reload failed too: same backoff as any other error
              retryTimer = setTimeout(flush, retryDelay);
              retryDelay = Math.min(retryDelay * 2, 60000);
              break;
            }
            adopt(m);
            continue;
          }
          if (e.status >= 500) {
            showToast('Server antwortet nicht, neuer Versuch gleich', true);
            retryTimer = setTimeout(flush, retryDelay);
            retryDelay = Math.min(retryDelay * 2, 60000);
            break;
          }
          // refused for good (e.g. the match is finished): drop it and the undo aimed at it
          showToast(e.message, true);
          const rest = ops.filter((o) => o !== op && !(op.type === 'add' && o.type === 'undo' && o.cid === op.action.cid));
          ops = reconcileOps(match.actions, rest, match.actions.slice(-1)[0]?.seq || 0).ops;
          persistOps(id, ops);
        } finally {
          inflight = null;
        }
      }
    } finally {
      flushing = false;
      // a doubt raised while a request was on the wire and nothing left to
      // send: ask now, so a lost lease shows as read-only right away
      if (alive && mid === id && needVerify && lease && !ops.length) { verifyLease(mid).catch(() => {}); }
      // this flush belonged to a match the page has left: the flag it held
      // kept the new match's queue waiting, so hand over now
      if (alive && mid !== id && ops.length) flush();
      // the final point is confirmed and nothing is pending: let go of the match
      if (alive && mid === id && lease && !ops.length && !inflight && match && cfg && replay(cfg, match.actions).finished) releaseLease(false);
    }
  }

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
    if (!canWrite) {
      // after a finished match the lease was released: undoing the final point acquires anew
      if (claimable && $online && !acquiring) { acquireLease('claim').then((ok) => { if (ok) undo(); else explainNoWrite(); }); return; }
      explainNoWrite();
      return;
    }
    const a = last;
    const lastOp = ops[ops.length - 1];
    // an add that never went out is simply taken back; anything else
    // (confirmed, on the wire, or sent with the answer lost) gets a targeted
    // undo, because the server may hold it
    if (lastOp?.type === 'add' && !lastOp.sent && lastOp !== inflight) ops = ops.slice(0, -1);
    else ops = [...ops, { type: 'undo', cid: a.cid ?? null, seq: a.seq }];
    gen++;
    persistOps(id, ops);
    selected = null;
    showToast('Rückgängig: ' + describe(a));
    flush();
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
        {#if holderElse}
          <div class="panel hint scoutbar" role="status">
            <span class="hint-txt"><b>{scout.actor || 'Jemand'}</b> scoutet auf einem anderen Gerät{scout.device ? ` (${scout.device})` : ''} · seit {hhmm(scout.since)}</span>
            {#if canScout}<button class="btn primary" onclick={takeover} disabled={acquiring || !$online || !tabOwner}>Scouting übernehmen</button>{/if}
          </div>
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
          <div class="txt">{last ? 'Zuletzt: ' + describe(last) : 'Noch keine Aktion.'}{#if ops.length}<span class="pending"> · {ops.length} ausstehend</span>{/if}</div>
          <button class="btn" onclick={undo} disabled={!last || !canScout || (!canWrite && !claimable)}><svg class="ico-undo" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M12.5 8c-2.65 0-5.05.99-6.9 2.6L2 7v9h9l-3.62-3.62c1.39-1.16 3.16-1.88 5.12-1.88 3.54 0 6.55 2.31 7.6 5.5l2.37-.78C21.08 11.03 17.15 8 12.5 8z"/></svg> Rückgängig</button>
        </section>
      </div>

      <div class="col mid">
        <Pad {expected} idle={!selected} disabled={!canScout || !(canWrite || claimable) || st.finished || tossPending} ontap={tapCell} />
        <div class="pad-foot" class:has-mic={canScout}>
          <button class="btn big us" disabled={!canScout || !(canWrite || claimable) || st.finished || tossPending} onclick={() => queue({ skill: 'opp', grade: '=' })}>Fehler Gegner <span class="muted">+1 wir</span></button>
          {#if canScout}
            <button class="btn big mic" class:on={voiceMode} onclick={toggleVoiceMode} title={voiceMode ? 'Zurück zum Pad' : 'Sprache'} aria-label={voiceMode ? 'Zurück zum Pad' : 'Sprache'}>
              {#if voiceMode}<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><path fill="currentColor" d="M3 3h8v8H3V3zm10 0h8v8h-8V3zM3 13h8v8H3v-8zm10 0h8v8h-8v-8z"/></svg>
              {:else}<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><path fill="currentColor" d="M12 14a3 3 0 0 0 3-3V5a3 3 0 0 0-6 0v6a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.92V21h2v-3.08A7 7 0 0 0 19 11h-2z"/></svg>{/if}
            </button>
          {/if}
          <button class="btn big them" disabled={!canScout || !(canWrite || claimable) || st.finished || tossPending} onclick={() => queue({ skill: 'opp', grade: '#' })}>Punkt Gegner</button>
        </div>
        {#if canScout}
          <Voice bind:this={voice} players={match.players} onCourt={court.map((c) => c.id)} disabled={!(canWrite || claimable) || st.finished || tossPending} onaction={queue} />
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
        <div class="tl-head"><h2>Verlauf</h2><span class="small muted">{actionsAll.length} Aktionen</span><span class="spacer"></span><a class="small" href="{base}/auswertung/{id}">Auswertung →</a></div>
        <button class="tl-undo" onclick={undo} disabled={!last || !canScout} title={last ? 'Rückgängig: ' + describe(last) : 'Nichts zum Rückgängigmachen'}><svg class="ico-undo" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M12.5 8c-2.65 0-5.05.99-6.9 2.6L2 7v9h9l-3.62-3.62c1.39-1.16 3.16-1.88 5.12-1.88 3.54 0 6.55 2.31 7.6 5.5l2.37-.78C21.08 11.03 17.15 8 12.5 8z"/></svg>{#if ops.length}<i>{ops.length}</i>{/if}</button>
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
