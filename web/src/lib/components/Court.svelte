<script>
  // Own half seen from behind the baseline: front row IV III II at the net,
  // back row V VI I. Tapping a slot selects that player. `suggested` ids get
  // a hint ring (server on I, setter when a set is expected); `over` is the
  // position a dragged chip is hovering; while `dragging`, only `targets`
  // (positions) accept the chip, the rest fade out.
  import { ROMAN, firstName } from '$lib/engine.js';
  let { court, byId, selected = null, serving = false, suggested = [], over = null, dragging = false, targets = [], onselect } = $props();
  const byPos = $derived(Object.fromEntries(court.map((c) => [c.pos, c])));
</script>

<div class="court" class:dragging>
  {#each [[4, 3, 2], [5, 6, 1]] as row}
    <div class="rowp">
      {#each row as pos}
        {@const c = byPos[pos]}
        {@const p = byId[c?.id]}
        <button
          class="slot"
          class:sel={selected === c?.id}
          class:lib={c?.libero}
          class:srv={pos === 1 && serving}
          class:hint={!dragging && suggested.includes(c?.id)}
          class:target={dragging && targets.includes(pos)}
          class:blocked={dragging && !targets.includes(pos)}
          class:over={over === pos}
          data-pos={pos}
          onclick={() => onselect?.(c.id, pos)}
          disabled={!p}
        >
          <span class="posn">{ROMAN[pos - 1]}</span>
          <span class="role">{p?.position || ''}</span>
          <span class="jersey">{p?.number ?? '–'}</span>
          <span class="nm">{firstName(p)}{#if c?.libero}<small> für {byId[c.replaced]?.number}</small>{/if}</span>
        </button>
      {/each}
    </div>
  {/each}
</div>

<style>
  .court {
    position: relative; background: var(--court-soft);
    border: 2px solid var(--court-line); border-top: 6px solid var(--ink-2);
    border-radius: 4px; padding: 8px; display: grid; grid-template-rows: 1fr 1fr; gap: 8px;
  }
  .court::before { content: 'Netz'; position: absolute; top: -20px; left: 50%; transform: translateX(-50%); font-size: 11px; color: var(--ink-3); letter-spacing: 0.04em; }
  .rowp { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .slot {
    min-height: 76px; border-radius: var(--r-m); border: 2px solid transparent;
    background: var(--panel); cursor: pointer; padding: 6px 6px 5px; text-align: left; position: relative;
    display: grid; grid-template-columns: 1fr auto; grid-template-rows: auto 1fr auto; align-content: stretch;
    transition: border-color 0.12s, box-shadow 0.12s;
  }
  .slot .posn { font-size: 10px; color: var(--ink-3); grid-column: 1; }
  .slot .role { font-size: 10px; font-weight: 700; color: var(--ink-3); grid-column: 2; text-align: right; }
  .slot .jersey { font-family: var(--disp); font-size: 30px; font-weight: 700; line-height: 1; grid-column: 1; align-self: center; }
  .slot .nm { font-size: 12px; color: var(--ink-2); grid-column: 1 / -1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .slot.hint { border-color: var(--accent); border-style: dashed; }
  .slot.hint::before { content: 'erwartet'; position: absolute; top: 4px; right: 6px; font-size: 9px; color: var(--accent-text); font-weight: 600; }
  .slot.hint .role { visibility: hidden; }
  .slot.sel { border-color: var(--accent); border-style: solid; background: var(--accent-soft); }
  .slot.srv::after { content: ''; position: absolute; right: 6px; bottom: 22px; width: 14px; height: 14px; background: url(/icon.svg) center / contain no-repeat; }
  .slot.lib { border-style: dashed; border-color: var(--court-line); }
  .slot.lib.hint { border-color: var(--accent); }
  .slot.lib .role { color: var(--court-line); }
  .slot.target { border-style: dashed; border-color: var(--court-line); box-shadow: 0 0 0 2px var(--court-soft); }
  .slot.blocked { opacity: 0.3; filter: grayscale(1); }
  .slot.over { border-color: var(--accent); border-style: solid; background: var(--accent-soft); box-shadow: 0 0 0 3px var(--accent-soft); transform: scale(1.03); }
  @media (max-width: 759px) and (max-height: 700px) {
    .slot .nm { display: none; }
    .slot.srv::after { bottom: 6px; right: 8px; }
    .slot.hint::before { font-size: 8px; right: 4px; }
  }
  @media (max-width: 759px) {
    .court { padding: 5px; gap: 5px; border-top-width: 4px; }
    .court::before { top: -17px; }
    .rowp { gap: 5px; }
    .slot { min-height: 54px; padding: 3px 6px; }
    .slot .jersey { font-size: 22px; }
    .slot .nm { font-size: 11px; }
    .slot .posn, .slot .role { font-size: 9px; }
  }
</style>
