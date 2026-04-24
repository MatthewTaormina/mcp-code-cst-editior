// Resolve absolute paths to bundled grammar files and the web-tree-sitter runtime WASM.

import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { LanguageDef } from './languages.js';

const here = dirname(fileURLToPath(import.meta.url));

// Grammars sit at packages/lexigraph-core/grammars/. Search a few candidates so this
// works both from src/ (during vitest) and dist/ (after build).
const grammarCandidates = [resolve(here, '..', 'grammars'), resolve(here, '..', '..', 'grammars')];

function grammarsRoot(): string {
  for (const dir of grammarCandidates) {
    if (existsSync(dir)) return dir;
  }
  throw new Error(
    `lexigraph: grammars directory not found; expected one of: ${grammarCandidates.join(', ')}`,
  );
}

export function grammarPath(lang: LanguageDef): string {
  const path = resolve(grammarsRoot(), lang.wasm);
  if (!existsSync(path)) {
    throw new Error(`lexigraph: missing grammar ${lang.wasm} at ${path}`);
  }
  return path;
}

const require = createRequire(import.meta.url);

/** Absolute path to the web-tree-sitter runtime WASM file. */
export function runtimeWasmPath(): string {
  // package.json exports './tree-sitter.wasm' as a subpath; resolve returns the file path.
  return require.resolve('web-tree-sitter/tree-sitter.wasm');
}
