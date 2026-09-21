<script>
  import { base } from '$app/paths';
  // The guide inside the app shell (top bar, tab bar, theme): the static
  // page under /hilfe/ stays for logged-out readers and deep links; here its
  // article and its own styles are loaded and shown as a normal page.
  let html = $state('');
  let css = $state('');
  let error = $state('');
  $effect(() => {
    fetch(`${base}/hilfe/index.html`)
      .then((r) => (r.ok ? r.text() : Promise.reject(new Error('HTTP ' + r.status))))
      .then((text) => {
        const doc = new DOMParser().parseFromString(text, 'text/html');
        css = [...doc.querySelectorAll('style')].map((s) => s.textContent).join('\n');
        const article = doc.querySelector('article');
        const toc = doc.querySelector('.toc-inline');
        // relative links of the standalone page point at the app root
        for (const a of [...(article?.querySelectorAll('a[href]') || []), ...(toc?.querySelectorAll('a[href]') || [])]) {
          const h = a.getAttribute('href');
          if (h.startsWith('../')) a.setAttribute('href', `${base}/${h.slice(3)}`);
          else if (h === './') a.setAttribute('href', `${base}/anleitung`);
          else if (h.startsWith('/') && !h.startsWith(base + '/')) a.setAttribute('href', base + h);
        }
        html = article ? article.outerHTML : ''; // the inline table of contents already sits inside the article
        if (!article) error = 'Anleitung nicht gefunden.';
        // an anchor in the URL: scroll to it once the article is in the DOM
        if (location.hash) setTimeout(() => document.getElementById(location.hash.slice(1))?.scrollIntoView(), 50);
      })
      .catch((e) => (error = e.message));
  });
</script>

<svelte:head><title>Sideout — Anleitung</title></svelte:head>
<main class="page guide">
  {#if error}<div class="panel empty">{error}</div>
  {:else if !html}<div class="panel empty">Lade…</div>
  {:else}
    {@html '<style>' + css + '</style>'}
    {@html html}
  {/if}
</main>

<style>
  /* one column at every width: the standalone page's sidebar grid (.doc) is
     not used here, and its inline table of contents stays visible on
     desktop too (the embedded stylesheet hides it from 1000 px) */
  .guide { max-width: 800px; }
  .guide :global(.toc-inline) { display: block !important; margin-bottom: 16px; }
  .guide :global(article) { min-width: 0; }
  .guide :global(article h1) { margin-top: 0; }
</style>
