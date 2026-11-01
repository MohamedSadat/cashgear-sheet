// Fetches the public test workbooks into tests/workbooks/public, which git ignores.
// Sources are pinned to exact commits, so the set can be rebuilt byte for byte, and
// the manifest records each file's URL and hash. Downloaded files are untrusted: they
// are only ever parsed, never opened in Excel. `--generated` adds the workbooks from
// scripts/generate-test-workbooks.mjs.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { root, run } from './lib.mjs';

const outRoot = join(root, 'tests/workbooks/public');
const maxBytes = 64 * 1024 * 1024;
const concurrency = 12;

/** Each `paths` entry is a directory prefix inside the repo at the pinned commit. */
export const sources = [
  {
    name: 'poi',
    repo: 'apache/poi',
    sha: 'ffc679c897644c2f1d12d62646594bc03d432ad2',
    paths: ['test-data/'],
  },
  {
    name: 'closedxml',
    repo: 'ClosedXML/ClosedXML',
    sha: '057f5976c788d97e8eac86d1a528bd5411d9386e',
    paths: ['ClosedXML.Tests/Resource/'],
  },
  {
    name: 'phpspreadsheet',
    repo: 'PHPOffice/PhpSpreadsheet',
    sha: 'ef440dd714c3fc390acf6a56228a6d42df18059d',
    paths: ['tests/data/', 'samples/'],
  },
  {
    name: 'xlcalculator',
    repo: 'bradbase/xlcalculator',
    sha: '0a187c97f0175bdee854d47d161a61c7cb0bd699',
    paths: ['tests/'],
  },
  {
    name: 'pycel',
    repo: 'dgorissen/pycel',
    sha: '8423a5119002bb0f6d413eea67137f9cbf1a4504',
    paths: ['tests/'],
  },
  {
    name: 'formulas',
    repo: 'vinci1it2000/formulas',
    sha: '0b14f8f61fcc42b3a3d9ec97ce36e87337e3498f',
    paths: ['test/'],
  },
  {
    name: 'epplus',
    repo: 'EPPlusSoftware/EPPlus',
    sha: '0ed726fde80cac598c023f5bb4824a6ad665d882',
    paths: ['src/EPPlusTest/'],
  },
  {
    name: 'exceljs',
    repo: 'exceljs/exceljs',
    sha: '5bed18b45e824f409b08456b59b87430ded023ab',
    paths: ['spec/'],
  },
  {
    name: 'excelize',
    repo: 'qax-os/excelize',
    sha: 'c5fd73bde8f611a2f9c30eaf7b0494f2189cb79a',
    paths: ['test/'],
  },
  {
    name: 'xlsxwriter',
    repo: 'jmcnamara/XlsxWriter',
    sha: 'af5927c110cf01571046a3138600f63b256ea89a',
    paths: ['xlsxwriter/test/comparison/xlsx_files/'],
  },
  {
    name: 'openxml-sdk',
    repo: 'dotnet/Open-XML-SDK',
    sha: '431ab05cf160248cc3885a4a766026d4f8243792',
    paths: ['test/'],
  },
  {
    name: 'npoi',
    repo: 'nissl-lab/npoi',
    sha: 'f217dfa796e68c210e1e53557c30920ab748bcaa',
    paths: ['testcases/'],
  },
  {
    name: 'koala',
    repo: 'vallettea/koala',
    sha: '3e7a654634c6b3856558cc1f7cd2e6ec58c74cd2',
    paths: ['tests/'],
  },
  {
    name: 'pandas',
    repo: 'pandas-dev/pandas',
    sha: '225bc0138c25b1dc0d691ddbb5f2798251c65c74',
    paths: ['pandas/tests/io/data/excel/'],
  },
  {
    name: 'calamine',
    repo: 'tafia/calamine',
    sha: 'b8ec4f09cba6aea7d8359ababc240c9ed064751b',
    paths: ['tests/'],
  },
  {
    name: 'libreoffice',
    repo: 'LibreOffice/core',
    sha: 'ffc1698dd1f88764852e548f561e237da72da2c4',
    paths: ['sc/qa/unit/data/xlsx/', 'sc/qa/unit/data/functions/', 'sc/qa/extras/testdocs/'],
  },
  // Both engines' own fixtures. The report shows results with and without these, since
  // an engine has been tuned on its own files.
  {
    name: 'ironcalc',
    repo: 'ironcalc/ironcalc',
    sha: '2128db12ab30b77fdebde9cba994273a26438ae3',
    paths: ['xlsx/tests/'],
    engineFixture: true,
  },
  {
    name: 'formualizer',
    repo: 'PSU3D0/formualizer',
    sha: 'dfd2f96457ff0929114a3e6b786c2761e6db5c5d',
    paths: ['crates/formualizer-workbook/tests/', 'benchmarks/'],
    engineFixture: true,
  },
];

function token() {
  if (process.env.GITHUB_TOKEN) return process.env.GITHUB_TOKEN;
  const gh = spawnSync('gh', ['auth', 'token'], { encoding: 'utf8' });
  return gh.status === 0 ? gh.stdout.trim() : undefined;
}

async function githubJson(path, auth) {
  const response = await fetch('https://api.github.com/' + path, {
    headers: {
      accept: 'application/vnd.github+json',
      'user-agent': 'cashgear-sheet-fetch',
      ...(auth ? { authorization: 'Bearer ' + auth } : {}),
    },
  });
  if (!response.ok) throw new Error('GitHub ' + path + ': ' + response.status);
  return response.json();
}

/** Lists blobs under `prefix`, walking down one level at a time when a whole tree is too big. */
async function listPrefix(source, prefix, auth) {
  let tree = source.sha;
  let base = '';
  for (const part of prefix.split('/').filter(Boolean)) {
    const level = await githubJson(`repos/${source.repo}/git/trees/${tree}`, auth);
    const entry = level.tree.find((item) => item.path === part && item.type === 'tree');
    if (!entry) return [];
    tree = entry.sha;
    base += part + '/';
  }
  const listed = await githubJson(`repos/${source.repo}/git/trees/${tree}?recursive=1`, auth);
  if (listed.truncated) throw new Error(source.repo + ' ' + prefix + ' listing was truncated');
  return listed.tree
    .filter((item) => item.type === 'blob')
    .map((item) => ({ path: base + item.path, size: item.size }));
}

async function listSource(source, auth) {
  const seen = new Set();
  const files = [];
  for (const prefix of source.paths) {
    for (const file of await listPrefix(source, prefix, auth)) {
      if (!/\.(xlsx|xlsm)$/i.test(file.path) || seen.has(file.path)) continue;
      seen.add(file.path);
      files.push(file);
    }
  }
  return files;
}

async function download(url) {
  for (let attempt = 0; ; attempt++) {
    try {
      const response = await fetch(url, { headers: { 'user-agent': 'cashgear-sheet-fetch' } });
      if (!response.ok) throw new Error('HTTP ' + response.status);
      return Buffer.from(await response.arrayBuffer());
    } catch (error) {
      if (attempt >= 3) throw error;
      await new Promise((done) => setTimeout(done, 1000 * (attempt + 1)));
    }
  }
}

async function fetchPublic() {
  const auth = token();
  const manifestPath = join(outRoot, 'manifest.json');
  const known = existsSync(manifestPath)
    ? JSON.parse(await readFile(manifestPath, 'utf8')).files
    : [];
  const byUrl = new Map(known.map((file) => [file.url, file]));
  const hashes = new Map();
  const files = [];
  const skipped = [];

  const queue = [];
  for (const source of sources) {
    const listed = await listSource(source, auth);
    console.log(source.name + ': ' + listed.length + ' files at ' + source.sha.slice(0, 8));
    for (const file of listed) queue.push({ source, file });
  }

  let done = 0;
  // Sources run in table order so that duplicates always keep the same owner.
  const results = new Array(queue.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: concurrency }, async () => {
      while (next < queue.length) {
        const index = next++;
        const { source, file } = queue[index];
        const url = `https://raw.githubusercontent.com/${source.repo}/${source.sha}/${file.path
          .split('/')
          .map(encodeURIComponent)
          .join('/')}`;
        const local = join(source.name, file.path.replace(/[\\/]/g, '__'));
        const absolute = join(outRoot, local);
        const prior = byUrl.get(url);
        if (file.size > maxBytes) {
          results[index] = { skip: 'over size limit', url };
        } else if (prior && existsSync(absolute)) {
          const bytes = await readFile(absolute);
          const sha256 = createHash('sha256').update(bytes).digest('hex');
          results[index] =
            sha256 === prior.sha256
              ? { entry: prior, sha256 }
              : { refetch: true, source, file, url, local, absolute };
        } else {
          results[index] = { refetch: true, source, file, url, local, absolute };
        }
        const pending = results[index];
        if (pending.refetch) {
          try {
            const bytes = await download(url);
            // Encrypted packages and LFS pointers aren't zips, so nothing can read them.
            if (bytes[0] !== 0x50 || bytes[1] !== 0x4b) {
              results[index] = { skip: 'not a zip package', url };
            } else {
              await mkdir(dirname(absolute), { recursive: true });
              await writeFile(absolute, bytes);
              const sha256 = createHash('sha256').update(bytes).digest('hex');
              results[index] = {
                entry: {
                  source: source.name,
                  engineFixture: source.engineFixture === true,
                  path: local.replace(/\\/g, '/'),
                  url,
                  bytes: bytes.length,
                  sha256,
                },
                sha256,
              };
            }
          } catch (error) {
            results[index] = { skip: String(error.message ?? error), url };
          }
        }
        done++;
        if (done % 200 === 0) console.log(done + ' of ' + queue.length);
      }
    }),
  );

  for (const result of results) {
    if (result.skip) {
      skipped.push({ url: result.url, reason: result.skip });
    } else if (hashes.has(result.sha256)) {
      skipped.push({ url: result.entry.url, reason: 'same bytes as ' + hashes.get(result.sha256) });
    } else {
      hashes.set(result.sha256, result.entry.path);
      files.push(result.entry);
    }
  }
  await mkdir(outRoot, { recursive: true });
  await writeFile(manifestPath, JSON.stringify({ files, skipped }, null, 1) + '\n');
  const total = files.reduce((sum, file) => sum + file.bytes, 0);
  console.log(
    files.length +
      ' unique files, ' +
      (total / 1048576).toFixed(1) +
      ' MB; skipped ' +
      skipped.length,
  );
}

const onlyGenerated = process.argv.includes('--generated-only');
if (!onlyGenerated) await fetchPublic();
if (onlyGenerated || process.argv.includes('--generated')) {
  await run(process.execPath, ['scripts/generate-test-workbooks.mjs']);
}
