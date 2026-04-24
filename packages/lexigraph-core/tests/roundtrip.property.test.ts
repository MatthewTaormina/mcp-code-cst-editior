// Property-based round-trip: random-but-valid source from each language's corpus
// (concatenated with newline padding) must still round-trip when re-parsed.

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import fc from 'fast-check';
import { describe, it } from 'vitest';

import { LANGUAGES, type LanguageId, parse } from '../src/index.js';

const here = dirname(fileURLToPath(import.meta.url));
const corpusRoot = resolve(here, 'corpus');

function loadCorpus(lang: string): string[] {
  try {
    return readdirSync(join(corpusRoot, lang)).map((f) =>
      readFileSync(join(corpusRoot, lang, f), 'utf8'),
    );
  } catch {
    return [];
  }
}

describe('round-trip property tests', () => {
  for (const id of Object.keys(LANGUAGES) as LanguageId[]) {
    const samples = loadCorpus(id);
    if (samples.length === 0) continue;

    it(`${id}: random concatenations of corpus snippets round-trip`, async () => {
      await fc.assert(
        fc.asyncProperty(
          fc.array(fc.integer({ min: 0, max: samples.length - 1 }), {
            minLength: 1,
            maxLength: 4,
          }),
          fc.string({ unit: fc.constantFrom('\n', '\n\n', '\n\n\n'), minLength: 1, maxLength: 3 }),
          async (indices, sep) => {
            const source = indices.map((i) => samples[i]!).join(sep);
            const tree = await parse({ languageId: id, source });
            const round = tree.serialize();
            if (round !== source) {
              throw new Error(
                `round-trip mismatch for ${id}; lengths ${source.length} vs ${round.length}`,
              );
            }
          },
        ),
        { numRuns: 25 },
      );
    });
  }
});
