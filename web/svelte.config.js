import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
// deploy under a path prefix (e.g. /sideout on a shared host): set SO_BASE at build time
const base = (process.env.SO_BASE || '').replace(/\/$/, '');
// output directory (default build/); lets a prefixed build live next to a plain one
const out = process.env.SO_OUTDIR || 'build';

const config = {
  kit: {
    // static SPA: the Axum binary serves build/ with index.html fallback
    adapter: adapter({ fallback: 'index.html', pages: out, assets: out }),
    paths: { base },
    // an open tab notices a new deploy within a minute and turns its next
    // navigation into a full reload, instead of dead-ending on a chunk that
    // no longer exists (looks like "the app is locked" until F5)
    version: { pollInterval: 60000 }
  }
};

export default config;
