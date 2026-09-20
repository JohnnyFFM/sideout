<script>
  // Rows = skills, columns = Data-Volley grades. Cells that don't exist for a
  // skill stay blank. Expected rows (from the rally phase) are highlighted.
  import { SKILLS, GRADES, GRADE_CLASS, GRADE_NAME, PAD } from '$lib/engine.js';
  let { expected = [], idle = true, disabled = false, ontap } = $props();
</script>

<section class="pad" class:idle>
  <div class="pad-head">
    <span></span>
    {#each GRADES as g}<span>{GRADE_NAME[g].split('/')[0]}</span>{/each}
  </div>
  {#each SKILLS as s}
    <div class="prow" class:expect={expected.includes(s.key)}>
      <div class="lbl">{s.name}{#if expected.includes(s.key)}<small>erwartet</small>{/if}</div>
      {#each GRADES as g}
        {#if PAD[s.key][g]}
          <button class="cell {GRADE_CLASS[g]}" {disabled} onclick={() => ontap?.(s.key, g)}>
            <b>{g}</b><span>{PAD[s.key][g]}</span>
          </button>
        {:else}
          <div class="cell none"></div>
        {/if}
      {/each}
    </div>
  {/each}
</section>

<style>
  .pad { background: var(--panel); border: 1px solid var(--line-soft); border-radius: var(--r-l); padding: 10px; display: grid; gap: 5px; }
  .pad-head, .prow { display: grid; grid-template-columns: 62px repeat(6, 1fr); gap: 5px; align-items: stretch; }
  .pad-head span { font-size: 10px; color: var(--ink-3); text-align: center; align-self: end; }
  .prow .lbl { display: flex; flex-direction: column; justify-content: center; font-size: 13px; font-weight: 600; color: var(--ink-2); padding-left: 2px; }
  .prow .lbl small { font-size: 10px; font-weight: 500; color: var(--ink-3); }
  .prow.expect .lbl, .prow.expect .lbl small { color: var(--accent-text); }
  .cell {
    min-height: var(--tap); border: 0; border-radius: var(--r-m); cursor: pointer;
    display: grid; align-content: center; justify-items: center; gap: 1px; padding: 2px;
  }
  .cell b { font-family: var(--disp); font-size: 20px; line-height: 1; font-weight: 700; }
  .cell span { font-size: 10px; line-height: 1; letter-spacing: -0.01em; }
  .cell.none { background: var(--raised); cursor: default; opacity: 0.35; }
  .cell:active { transform: scale(0.96); }
  .cell:disabled { opacity: 0.3; cursor: default; }
  .pad.idle .cell:not(.none) { opacity: 0.6; }
  .pad.idle .prow.expect .cell:not(.none) { opacity: 1; }
  @media (max-width: 400px) {
    .pad-head, .prow { grid-template-columns: 52px repeat(6, 1fr); }
    .prow .lbl { font-size: 12px; }
    .cell span { font-size: 9px; }
  }
  @media (max-width: 759px) and (max-height: 700px) {
    .pad-head { display: none; }
    .cell { min-height: 35px !important; }
    .cell b { font-size: 15px !important; }
    .cell span { display: none; }
    .prow .lbl small { display: none; }
  }
  @media (max-width: 759px) {
    .pad { padding: 6px; gap: 3px; }
    .pad-head, .prow { gap: 3px; }
    .cell { min-height: 42px; }
    .cell b { font-size: 17px; }
    .cell span { font-size: 9px; }
    .prow .lbl { font-size: 12px; }
  }
</style>
