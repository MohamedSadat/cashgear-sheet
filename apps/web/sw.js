// The web app's service worker: the app works offline once it has loaded online.
// apps/web/pwa.ts builds it with the build's files in place of the lists below.
//
// - The app shell (the page, scripts, styles, WASM, fonts and icons) is cached when the worker
//   installs, under a name of its build.
// - Other build files (the third-party notices) are cached the first time they are used, and
//   dropped once a build no longer has them.
// - Pages come from the network first, so a new build shows at once, and from the cache offline.
//   Cached responses keep their cross-origin isolation headers, which the engine's shared memory
//   needs.
// - Workbooks never pass through it: they are the user's files, read from the device.

/** Cached when the worker installs. */
const shell = self.__SHELL__;
/** Every file of the build, to keep in the cache of files cached on use. */
const files = new Set(self.__FILES__);
const build = self.__BUILD__;
const shellCache = `cgs-shell-${build}`;
const filesCache = 'cgs-files';

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(shellCache)
      .then((cache) => cache.addAll(shell))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name.startsWith('cgs-shell-') && name !== shellCache) await caches.delete(name);
      }
      const cache = await caches.open(filesCache);
      for (const request of await cache.keys()) {
        if (!files.has(new URL(request.url).pathname)) await cache.delete(request);
      }
      await self.clients.claim();
    })(),
  );
});

async function fromCache(request) {
  const cached = await caches.match(request.url);
  if (cached) return cached;
  const response = await fetch(request);
  if (response.status === 200) {
    await (await caches.open(filesCache)).put(request.url, response.clone());
  }
  return response;
}

async function page(request) {
  try {
    return await fetch(request);
  } catch (error) {
    const cached = await caches.match('/', { cacheName: shellCache });
    if (cached) return cached;
    throw error;
  }
}

self.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (request.mode === 'navigate') event.respondWith(page(request));
  else if (files.has(url.pathname)) event.respondWith(fromCache(request));
});
