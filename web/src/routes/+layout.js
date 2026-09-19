// Static SPA: no SSR, everything renders client-side against /api.
export const ssr = false;
export const prerender = false;

import { api } from '$lib/api.js';

export async function load({ url }) {
  if (url.pathname === '/login') return { me: null };
  try {
    return { me: await api('/me') };
  } catch (e) {
    // offline with a cached shell: keep the app usable, the live page has its own cache
    if (e.offline) return { me: { offline: true, user: { display_name: '' }, team: { name: '' }, members: [] } };
    return { me: null };
  }
}
