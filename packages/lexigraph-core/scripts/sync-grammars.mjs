#!/usr/bin/env node
// Sync tree-sitter WASM grammars from the `tree-sitter-wasms` npm package
// into ./grammars/ and write MANIFEST.json (version + sha256 per file).
// Run from packages/lexigraph-core: `node scripts/sync-grammars.mjs`.

import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = resolve(here, '..');
const grammarsDir = join(pkgRoot, 'grammars');
mkdirSync(grammarsDir, { recursive: true });

const require = createRequire(import.meta.url);
const wasmsPkgPath = require.resolve('tree-sitter-wasms/package.json');
const wasmsPkgRoot = dirname(wasmsPkgPath);
const wasmsPkg = JSON.parse(readFileSync(wasmsPkgPath, 'utf8'));
const sourceOut = join(wasmsPkgRoot, 'out');

// Files we want, mapped to canonical names under ./grammars/.
const WANT = [
  'tree-sitter-javascript.wasm',
  'tree-sitter-typescript.wasm',
  'tree-sitter-tsx.wasm',
  'tree-sitter-json.wasm',
  'tree-sitter-css.wasm',
  'tree-sitter-html.wasm',
  'tree-sitter-rust.wasm',
  'tree-sitter-python.wasm',
  'tree-sitter-yaml.wasm',
  'tree-sitter-toml.wasm',
];

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

const manifest = {
  source: {
    name: wasmsPkg.name,
    version: wasmsPkg.version,
    note: 'WASM grammars bundled by tree-sitter-wasms; see https://github.com/Gregoor/tree-sitter-wasms',
  },
  files: {},
};

for (const file of WANT) {
  const src = join(sourceOut, file);
  const dst = join(grammarsDir, file);
  const bytes = readFileSync(src);
  copyFileSync(src, dst);
  manifest.files[file] = {
    sha256: sha256(bytes),
    bytes: bytes.length,
  };
  console.error(`copied ${file} (${bytes.length} bytes)`);
}

writeFileSync(join(grammarsDir, 'MANIFEST.json'), JSON.stringify(manifest, null, 2) + '\n');
console.error(`wrote MANIFEST.json with ${Object.keys(manifest.files).length} entries`);
