// Static SPA: no SSR, everything renders client-side against /api.
export const ssr = false;
export const prerender = false;

import { base } from '$app/paths';
import { api } from '$lib/api.js';
import { meCache } from '$lib/stores.js';

// /me is fetched once per session and cached; login, logout and the
// settings page call refreshMe() to drop the cache.
export async function load({ url }) {
  if (url.pathname === `${base}/login`) {
    meCache.value = null;
    return { me: null };
  }
  if (meCache.value) return { me: meCache.value };
  try {
    meCache.value = await api('/me');
    return { me: meCache.value };
  } catch (e) {
    if (e.offline) return { me: { offline: true, user: { display_name: '' }, team: { name: '' }, members: [] } };
    return { me: null };
  }
}
