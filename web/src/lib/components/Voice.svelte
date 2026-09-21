<script>
  // Voice input panel (prototype): on-device recognition, strict parsing,
  // every understood utterance echoed so a wrong one is one "Rückgängig" away.
  // Two ways to talk: hold (a big button, or a key such as Space or whatever
  // a Bluetooth clicker sends — learnable) or continuous listening.
  import { onDestroy } from 'svelte';
  import { base } from '$app/paths';
  import { grammar, parse, start } from '$lib/voice.js';
  let { players = [], onCourt = null, disabled = false, onaction } = $props();

  const MODEL = `${base}/models/vosk-model-small-de-0.15.tar.gz`;
  const saved = (() => { try { return JSON.parse(localStorage.getItem('so_voice') || '{}'); } catch { return {}; } })();
  let mode = $state(saved.mode === 'cont' ? 'cont' : 'hold');
  let key = $state(saved.key || 'Space');
  let learning = $state(false);
  let ctl = $state(null);       // recognizer controller while on
  let busy = $state(false);     // starting up
  let status = $state('aus');
  let partial = $state('');
  let held = $state(false);
  let log = $state([]);         // last few { ok, text }
  const on = $derived(!!ctl);
  const roster = $derived(players.filter((p) => p.active !== false));

  function persist() { try { localStorage.setItem('so_voice', JSON.stringify({ mode, key })); } catch {} }

  async function toggle() {
    if (ctl) { stop(); return; }
    busy = true; log = [];
    try {
      ctl = await start({
        modelUrl: MODEL, grammar: grammar(roster),
        onState: (s) => (status = s),
        onPartial: (p) => (partial = p),
        onResult: heard
      });
      if (mode === 'cont') ctl.gate(true);
    } catch (e) {
      status = 'Start fehlgeschlagen: ' + (e?.message || e);
      ctl = null;
    } finally { busy = false; }
  }
  function stop() { ctl?.stop(); ctl = null; held = false; partial = ''; status = 'aus'; }
  onDestroy(stop);

  function heard(text) {
    partial = '';
    const r = parse(text, roster, onCourt);
    console.debug('[voice]', JSON.stringify(text), r.ok ? r.label : r.reason);
    if (r.ok && !disabled) onaction?.(r.action);
    log = [{ ok: r.ok && !disabled, text: r.ok ? (disabled ? r.label + ' · gerade nicht möglich' : r.label) : r.reason, raw: text }, ...log].slice(0, 3);
  }
  function setMode(m) { mode = m; persist(); if (ctl) { ctl.gate(m === 'cont'); held = false; } }

  // hold: on-screen button
  function down(e) { if (!ctl || mode !== 'hold') return; e.preventDefault(); held = true; ctl.gate(true); }
  function up() { if (!ctl || mode !== 'hold' || !held) return; held = false; ctl.gate(false); }
  // hold: keyboard (or a Bluetooth clicker that types a key)
  const typing = (e) => ['INPUT', 'TEXTAREA', 'SELECT'].includes(e.target?.tagName) || e.target?.isContentEditable;
  function keydown(e) {
    if (learning) { e.preventDefault(); key = e.code || e.key; learning = false; persist(); return; }
    if (e.repeat || typing(e) || e.code !== key || mode !== 'hold' || !ctl) return;
    e.preventDefault(); if (!held) { held = true; ctl.gate(true); }
  }
  function keyup(e) { if (e.code === key && held && mode === 'hold') { e.preventDefault(); up(); } }
  const keyLabel = (k) => ({ Space: 'Leertaste', PageDown: 'Bild ↓', PageUp: 'Bild ↑', ArrowRight: 'Pfeil →', ArrowLeft: 'Pfeil ←', ArrowDown: 'Pfeil ↓', ArrowUp: 'Pfeil ↑', Enter: 'Enter' }[k] || k.replace(/^Key|^Digit/, ''));
</script>

<svelte:window onkeydown={keydown} onkeyup={keyup} />

<section class="panel voice" class:on class:held>
  <div class="head">
    <span class="title">Sprache <small class="proto">Prototyp</small></span>
    <span class="status" class:live={on}>{busy ? status : on ? (mode === 'cont' ? 'hört zu' : held ? 'spricht …' : 'bereit') : 'aus'}</span>
    <button class="btn" onclick={toggle} disabled={busy}>{busy ? '…' : on ? 'Aus' : 'Einschalten'}</button>
  </div>
  {#if on || busy}
    <div class="modes">
      <button class="seg" class:active={mode === 'hold'} onclick={() => setMode('hold')}>Halten</button>
      <button class="seg" class:active={mode === 'cont'} onclick={() => setMode('cont')}>Dauernd</button>
      {#if mode === 'hold'}
        <button class="seg key" class:active={learning} onclick={() => (learning = !learning)} title="Taste zum Halten: drücken, dann die gewünschte Taste">{learning ? 'Taste drücken …' : 'Taste: ' + keyLabel(key)}</button>
      {/if}
    </div>
    {#if mode === 'hold'}
      <button class="ptt" onpointerdown={down} onpointerup={up} onpointercancel={up} onpointerleave={up} oncontextmenu={(e) => e.preventDefault()} disabled={!on}>
        {held ? '● Sprechen …' : 'Halten und sprechen'}
      </button>
    {/if}
    <div class="echo">
      {#if partial}<div class="partial">{partial} …</div>{/if}
      {#each log as l, i (i)}
        <div class="line" class:ok={l.ok} class:bad={!l.ok}><b>{l.ok ? '✓' : '✗'}</b> {l.text}{#if !l.ok}<small> gehört: „{l.raw}“</small>{/if}</div>
      {/each}
      {#if !log.length && !partial}<div class="hint">Sag „Nummer Aktion Note“: <em>zwölf Block Punkt</em> · <em>Lena Annahme gut</em> · <em>Fehler Gegner</em></div>{/if}
    </div>
  {/if}
</section>

<style>
  .voice { display: grid; gap: 8px; }
  .head { display: flex; align-items: center; gap: 10px; }
  .title { font-weight: 600; color: var(--ink-2); flex: 1; }
  .proto { font-size: 10px; font-weight: 500; color: var(--ink-3); border: 1px solid var(--line-soft); border-radius: 999px; padding: 1px 6px; margin-left: 4px; vertical-align: middle; }
  .status { font-size: 12px; color: var(--ink-3); }
  .status.live { color: var(--ok); }
  .modes { display: flex; gap: 6px; flex-wrap: wrap; }
  .seg { border: 1px solid var(--line-soft); background: transparent; color: var(--ink-2); border-radius: 999px; padding: 4px 12px; font-size: 13px; cursor: pointer; }
  .seg.active { border-color: var(--accent-text); color: var(--accent-text); }
  .ptt {
    height: calc(var(--tap) * 1.4); border-radius: var(--r-m); border: 2px solid var(--accent-text); background: transparent;
    color: var(--accent-text); font-size: 17px; font-weight: 600; cursor: pointer; touch-action: none; user-select: none; -webkit-user-select: none;
  }
  .held .ptt { background: var(--accent-text); color: #fff; }
  .ptt:disabled { opacity: .5; }
  .echo { display: grid; gap: 3px; font-size: 13px; min-height: 20px; }
  .partial { color: var(--ink-3); font-style: italic; }
  .line b { font-weight: 700; margin-right: 4px; }
  .line.ok { color: var(--ink-1, inherit); }
  .line.ok b { color: var(--ok); }
  .line.bad { color: var(--ink-3); }
  .line.bad b { color: var(--danger); }
  .line small { color: var(--ink-3); margin-left: 6px; }
  .hint { color: var(--ink-3); font-size: 12px; }
  .hint em { font-style: normal; color: var(--ink-2); }
</style>
