// Lossless round-trip: parse(source) → reconstruct === source.
// This is the project's primary green bar. Every file under tests/corpus/<lang>/
// is exercised; new corpus files are picked up automatically.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { LANGUAGES, type LanguageId, parse } from '../src/index.js';

const here = dirname(fileURLToPath(import.meta.url));
const corpusRoot = resolve(here, 'corpus');

function listFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isFile()) out.push(full);
  }
  return out;
}

const corpus: { lang: LanguageId; files: string[] }[] = [];
for (const id of Object.keys(LANGUAGES) as LanguageId[]) {
  const dir = join(corpusRoot, id);
  let files: string[] = [];
  try {
    files = listFiles(dir);
  } catch {
    files = [];
  }
  corpus.push({ lang: id, files });
}

describe('round-trip (lossless serialize == source)', () => {
  for (const { lang, files } of corpus) {
    describe(lang, () => {
      if (files.length === 0) {
        it.skip('no corpus files', () => {});
        return;
      }
      for (const file of files) {
        it(`round-trips ${file.replace(corpusRoot, 'corpus')}`, async () => {
          const source = readFileSync(file, 'utf8');
          const tree = await parse({ languageId: lang, source });
          expect(tree.serialize()).toBe(source);
        });
      }
    });
  }
});
