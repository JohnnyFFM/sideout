<script>
  // Own half seen from behind the baseline: front row IV III II at the net,
  // back row V VI I. Tapping a slot selects that player.
  import { ROMAN, firstName } from '$lib/engine.js';
  let { court, byId, selected = null, serving = false, onselect } = $props();
  const byPos = $derived(Object.fromEntries(court.map((c) => [c.pos, c])));
</script>

<div class="court">
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
          onclick={() => onselect?.(c.id)}
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
    display: grid; grid-template-columns: 1fr auto; grid-template-rows: auto auto; align-content: space-between;
  }
  .slot .posn { font-size: 10px; color: var(--ink-3); grid-column: 1; }
  .slot .role { font-size: 10px; font-weight: 700; color: var(--ink-3); grid-column: 2; text-align: right; }
  .slot .jersey { font-family: var(--disp); font-size: 30px; font-weight: 700; line-height: 1; grid-column: 1; }
  .slot .nm { font-size: 12px; color: var(--ink-2); grid-column: 1 / -1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .slot.sel { border-color: var(--accent); background: var(--accent-soft); }
  .slot.srv::after { content: ''; position: absolute; right: 6px; bottom: 22px; width: 12px; height: 12px; border-radius: 50%; background: var(--g-neg); box-shadow: inset -2px -2px 0 #0003; }
  .slot.lib { border-style: dashed; border-color: var(--court-line); }
  .slot.lib .role { color: var(--court-line); }
  @media (max-width: 759px) {
    .court { padding: 6px; gap: 6px; }
    .rowp { gap: 6px; }
    .slot { min-height: 58px; padding: 4px 6px; }
    .slot .jersey { font-size: 24px; }
    .slot .nm { font-size: 11px; }
  }
</style>
