// Fuzz test: random patch sequences must never produce an `ok` result that contains
// grammar errors. The ERROR-node guard is the contract.

import fc from 'fast-check';
import { describe, it } from 'vitest';

import { applyPatch, parse, type LxNode, type PatchOp } from '../src/index.js';

const SAMPLE_VALUES = [
  '0',
  '1',
  '-1',
  '3.14',
  '"hello"',
  '"world"',
  'true',
  'false',
  'null',
  '[]',
  '{}',
  '[1, 2, 3]',
  '{"x": 1}',
  // Intentionally broken values; the guard must catch them.
  'oops',
  '{"a":',
  '[1,',
  '"unterminated',
];

describe('applyPatch fuzz — ERROR guard never lets a broken tree slip through', () => {
  it('JSON: random replace ops on any node either fail or yield a parseable tree', async () => {
    const baseSource = '{"a": 1, "b": [1, 2, 3], "c": {"nested": "value"}, "d": true}';
    const baseTree = await parse({ languageId: 'json', source: baseSource });
    const allPaths = collectPointers(baseTree.root).filter((p) => p !== '');

    await fc.assert(
      fc.asyncProperty(
        fc.array(fc.tuple(fc.constantFrom(...allPaths), fc.constantFrom(...SAMPLE_VALUES)), {
          minLength: 1,
          maxLength: 4,
        }),
        async (pairs) => {
          const ops: PatchOp[] = pairs.map(([path, value]) => ({
            op: 'replace' as const,
            path,
            value,
          }));
          const result = await applyPatch(baseTree, ops);
          if (result.status === 'ok') {
            // Must round-trip and must not have grammar errors.
            if (result.tree.serialize() !== result.tree.source) {
              throw new Error(
                `serialize() != source after ${ops.length} ops; src=${JSON.stringify(result.tree.source)}`,
              );
            }
            if (result.tree.hasError()) {
              throw new Error(
                `accepted patch yielded broken tree: ${JSON.stringify(result.tree.source)}`,
              );
            }
          }
          // status === 'error' or 'conflict' is always acceptable.
        },
      ),
      { numRuns: 50 },
    );
  });
});

function collectPointers(node: LxNode): string[] {
  const out: string[] = [node.pointer];
  for (const c of node.children) out.push(...collectPointers(c));
  return out;
}
