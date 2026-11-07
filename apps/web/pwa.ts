import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import type { Plugin } from 'vite';

// Builds the service worker (sw.js beside this file) with the build's files: the app shell it
// caches when it installs, and the files it caches the first time they are used. The notices are
// written after Vite's build (scripts/third-party-notices.mjs) and only About reads them.

const onUse = /^\/third-party-notices\.txt$/;
const notServed = /^\/(_headers|staticwebapp\.config\.json)$/;

export function serviceWorker(): Plugin {
  let publicDir = '';
  return {
    name: 'cgs-service-worker',
    apply: 'build',
    configResolved(config) {
      publicDir = config.publicDir;
    },
    generateBundle(_, bundle) {
      const built = Object.keys(bundle).map((name) => '/' + name);
      const listPublic = (dir: string): string[] =>
        readdirSync(dir).flatMap((name) => {
          const path = join(dir, name);
          return statSync(path).isDirectory() ? listPublic(path) : [path];
        });
      const extras = publicDir
        ? listPublic(publicDir)
            .map((path) => '/' + relative(publicDir, path).replace(/\\/g, '/'))
            .filter((path) => !notServed.test(path))
        : [];
      const files = [
        '/',
        ...built.filter((name) => name !== '/index.html'),
        ...extras,
        '/third-party-notices.txt',
      ].sort();
      const shell = files.filter((name) => !onUse.test(name) && name !== '/sw.js');
      const build = createHash('sha256').update(files.join('\n')).digest('hex').slice(0, 12);
      const template = readFileSync(new URL('./sw.js', import.meta.url), 'utf8');
      const source = template
        .replace('self.__SHELL__', JSON.stringify(shell))
        .replace('self.__FILES__', JSON.stringify(files))
        .replace('self.__BUILD__', JSON.stringify(build));
      this.emitFile({ type: 'asset', fileName: 'sw.js', source });
    },
  };
}
