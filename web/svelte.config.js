import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    // static SPA: the Axum binary serves build/ with index.html fallback
    adapter: adapter({ fallback: 'index.html' }),
    paths: { base: '' }
  }
};

export default config;
