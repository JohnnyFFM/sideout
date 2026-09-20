import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    // static SPA: the Axum binary serves build/ with index.html fallback
    adapter: adapter({ fallback: 'index.html' }),
    paths: { base: '' },
    // an open tab notices a new deploy within a minute and turns its next
    // navigation into a full reload, instead of dead-ending on a chunk that
    // no longer exists (looks like "the app is locked" until F5)
    version: { pollInterval: 60000 }
  }
};

export default config;
