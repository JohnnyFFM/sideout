/// <reference types="@sveltejs/kit" />
/* Sideout service worker.
   1. The API is NEVER cached by the worker — the live page keeps its own
      match log in localStorage and syncs through a queue (lib/offline.js).
   2. Navigations are NETWORK-FIRST with the fresh shell stored as the
      offline fallback, so an old build is never pinned but a coach in a
      hall without reception still gets the app.
   3. Hashed build assets are cache-first, keyed per version. */
import { build, files, version } from '$service-worker';

const CACHE = `sideout-${version}`;
const ASSETS = [...build, ...files];
const SHELL = '/__shell';

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(ASSETS))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== location.origin) return;
  if (url.pathname.startsWith('/api/')) return;

  if (request.mode === 'navigate') {
    event.respondWith(
      fetch(request)
        .then((res) => {
          const copy = res.clone();
          caches.open(CACHE).then((cache) => cache.put(SHELL, copy));
          return res;
        })
        .catch(() => caches.match(SHELL))
    );
    return;
  }

  if (ASSETS.includes(url.pathname)) {
    event.respondWith(caches.match(url.pathname).then((hit) => hit || fetch(request)));
  }
});
