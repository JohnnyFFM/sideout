<script>
  import { base } from '$app/paths';
  // The recordings of one match: who recorded on which device, how far,
  // which one is the result. The coach picks the result here; a device whose
  // own recording is not the result can continue the result as a new copy,
  // which becomes the result itself when nothing came in between. Recordings
  // the result fully contains (`superseded`) are not listed.
  import { goto } from '$app/navigation';
  import { untrack } from 'svelte';
  import { api } from '$lib/api.js';
  import { me, showToast, online } from '$lib/stores.js';
  import { recordingLabel, recordingMeta, isHere, baseFromMatch } from '$lib/recording.js';
  import { myRecording, deviceId, createRecording, onChange, setDeleted } from '$lib/recstore.js';
  import { kick, sync } from '$lib/uploader.js';

  let { matchId, onchange = null } = $props();
  let m = $state(null);
  let rec = $state(null);
  let myDevice = $state('');
  let busy = $state(false);
  let error = $state('');
  const canSelect = $derived($me?.user?.role === 'coach');
  const canScout = $derived(['coach', 'assistant'].includes($me?.user?.role));
  // a coach deletes any recording that is not the result, everyone else only their own
  const canDelete = (r) => !r.selected && (canSelect || (r.user_id != null && r.user_id === $me?.user?.id));
  const all = $derived(m?.recordings || []);
  const recordings = $derived(all.filter((r) => !r.superseded));
  const selectedRec = $derived(recordings.find((r) => r.selected) || null);
  // a copy of the result with nothing in between becomes the result on its first upload: not "another" recording meanwhile
  const mineNotResult = $derived(!!rec && !!m?.selected && m.selected !== rec.id && !(rec.origin_id === m.selected && rec.origin_n != null && rec.origin_n === selectedRec?.n));
  const mineNotUploaded = $derived(!!rec && !all.some((r) => r.id === rec.id));
  const pendingHere = $derived($sync.byMatch[matchId] || 0);

  async function load() {
    try {
      m = await api(`/matches/${matchId}`);
      error = '';
    } catch (e) { error = e.offline ? 'Keine Verbindung.' : e.message; }
    try { rec = await myRecording(matchId, $me?.user?.id ?? null); myDevice = await deviceId(); } catch { /* no local store */ }
  }
  $effect(() => { void matchId; void $me?.user?.id; untrack(load); });
  $effect(() => onChange(() => untrack(load)));

  async function select(rid) {
    if (!canSelect || !m || busy) return;
    busy = true;
    try {
      m = await api(`/matches/${matchId}/select`, { method: 'POST', body: { recording_id: rid, rev: m.selection_rev ?? 0 } });
      showToast('Als Ergebnis übernommen');
      onchange?.();
    } catch (e) {
      if (e.code === 'selection_moved') { showToast('Die Auswahl wurde inzwischen geändert, Stand aktualisiert', true); load(); }
      else showToast(e.message, true);
    } finally { busy = false; }
  }
  async function remove(r) {
    if (!canDelete(r) || busy) return;
    if (!confirm(`Aufzeichnung von ${recordingLabel(r)} (${recordingMeta(r, selectedRec)}) löschen? Sie verschwindet aus den Listen; die Daten bleiben auf dem Server.`)) return;
    busy = true;
    try {
      await api(`/matches/${matchId}/recordings/${r.id}`, { method: 'DELETE' });
      if (r.device_id === myDevice) await setDeleted(r.id).catch(() => {});
      showToast('Aufzeichnung gelöscht');
      await load();
      onchange?.();
    } catch (e) { showToast(e.message, true); }
    finally { busy = false; }
  }
  /** A → B → A: the result's state becomes a new own recording on this device; the older own one stays */
  async function continueCopy() {
    if (!canScout || !m?.selected || busy || !$sync.supported) return;
    busy = true;
    try {
      const b = baseFromMatch(m);
      await createRecording({ match_id: matchId, team_id: $me?.team?.id ?? null, user_id: $me?.user?.id ?? null, base: b, origin_id: m.selected, origin_n: selectedRec?.n ?? null, firstEdit: null });
      kick();
      showToast('Eigene Aufzeichnung als Kopie begonnen');
      goto(`${base}/live/${matchId}`);
    } catch (e) { showToast(e.message, true); }
    finally { busy = false; }
  }
</script>

<div class="recpanel">
  {#if error}<div class="small muted">{error}</div>
  {:else if !m}<div class="small muted">Lade…</div>
  {:else}
    <ul>
      {#if mineNotUploaded}
        <li><b>{$me?.user?.display_name || 'Ich'}</b><span class="here" title="auf diesem Gerät">📱</span> <span class="muted pend">{pendingHere ? `${pendingHere} nicht hochgeladen` : 'noch nicht hochgeladen'}</span></li>
      {/if}
      {#each recordings as r (r.id)}
        <li class:sel={r.selected}>
          <b>{recordingLabel(r)}</b>{#if isHere(r, myDevice)}<span class="here" title="auf diesem Gerät">📱</span>{/if}
          <span class="muted">{recordingMeta(r, selectedRec)}{isHere(r, myDevice) && pendingHere ? ` · ` : ''}{#if isHere(r, myDevice) && pendingHere}<span class="pend">{pendingHere} nicht hochgeladen</span>{/if}</span>
          {#if r.selected}<span class="chip ok">Ergebnis</span>{:else if canSelect}<button class="btn sm" onclick={() => select(r.id)} disabled={busy || !$online}>Als Ergebnis verwenden</button>{/if}
          {#if canDelete(r)}<button class="icon-btn del" title="Löschen" aria-label="Aufzeichnung löschen" onclick={() => remove(r)} disabled={busy || !$online}>🗑</button>{/if}
        </li>
      {/each}
    </ul>
    {#if mineNotResult && canScout}
      <div class="row" style="margin-top:8px; align-items:center; gap:8px; flex-wrap:wrap">
        <span class="small muted">Als Ergebnis zählt die Aufzeichnung von {selectedRec ? recordingLabel(selectedRec) : 'jemand anderem'}, nicht die dieses Geräts.</span>
        <button class="btn sm" onclick={continueCopy} disabled={busy} title="Den Stand des Ergebnisses übernehmen und auf diesem Gerät weiter tippen; kommt nichts dazwischen, zählt die Fortsetzung als Ergebnis. Die bisherige eigene Aufzeichnung bleibt erhalten">Ergebnis fortsetzen</button>
      </div>
    {/if}
  {/if}
</div>

<style>
  .recpanel { padding: 6px 0 10px; font-size: 13px; }
  .recpanel ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .recpanel li { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; padding: 6px 8px; border-radius: var(--r-m); background: var(--raised); }
  .recpanel li.sel { outline: 1px solid var(--accent); }
  .recpanel li .muted { flex: 1 1 160px; font-size: 12px; }
  .chip.ok { border-color: var(--ok); color: var(--ok); }
  .here { margin-left: 2px; font-size: 13px; }
  .pend { color: var(--g-neg); }
  .btn.sm { height: 28px; padding: 0 10px; font-size: 12px; }
  .icon-btn.del { width: 28px; height: 28px; font-size: 12px; }
</style>
