import { describe, expect, it } from 'vitest';

import { applyPatch, parse, type LxTree } from '../src/index.js';

async function jsonTree(source: string): Promise<LxTree> {
  return parse({ languageId: 'json', source });
}

describe('applyPatch — replace', () => {
  it('replaces a value node', async () => {
    const tree = await jsonTree('{"a": 1, "b": 2}');
    const valuePath = findFirstValuePath(tree);
    const result = await applyPatch(tree, [{ op: 'replace', path: valuePath, value: '99' }]);
    expect(result.status).toBe('ok');
    if (result.status !== 'ok') return;
    expect(result.tree.source).toBe('{"a": 99, "b": 2}');
    expect(result.tree.version).toBe(1);
    expect(result.tree.serialize()).toBe('{"a": 99, "b": 2}');
  });

  it('rejects a replace that breaks the grammar', async () => {
    const tree = await jsonTree('{"a": 1}');
    const valuePath = findFirstValuePath(tree);
    const result = await applyPatch(tree, [
      { op: 'replace', path: valuePath, value: 'oops not json' },
    ]);
    expect(result.status).toBe('error');
    if (result.status !== 'error') return;
    expect(result.reason).toMatch(/grammar errors/);
  });

  it('returns error when path does not exist', async () => {
    const tree = await jsonTree('{"a": 1}');
    const result = await applyPatch(tree, [{ op: 'replace', path: '/children/99', value: '0' }]);
    expect(result.status).toBe('error');
  });
});

describe('applyPatch — remove', () => {
  it('refuses to remove the root', async () => {
    const tree = await jsonTree('{}');
    const result = await applyPatch(tree, [{ op: 'remove', path: '' }]);
    expect(result.status).toBe('error');
  });
});

describe('applyPatch — add', () => {
  it('appends to an array by inserting before the closing bracket', async () => {
    const tree = await jsonTree('[1, 2, 3]');
    // Root → array; array's last child is "]". Insert before it with a leading ", ".
    const arr = tree.root.children[0]!;
    expect(arr.type).toBe('array');
    const closeIdx = arr.children.findIndex((c) => c.type === ']');
    const path = `${arr.pointer}/children/${closeIdx}`;
    const result = await applyPatch(tree, [{ op: 'add', path, value: ', 4' }]);
    expect(result.status).toBe('ok');
    if (result.status !== 'ok') return;
    expect(result.tree.source).toBe('[1, 2, 3, 4]');
  });

  it('rejects add path that does not address /children/N', async () => {
    const tree = await jsonTree('{}');
    const result = await applyPatch(tree, [{ op: 'add', path: '/foo/0', value: 'x' }]);
    expect(result.status).toBe('error');
  });

  it('rejects add that breaks the grammar', async () => {
    const tree = await jsonTree('[1, 2, 3]');
    const arr = tree.root.children[0]!;
    const closeIdx = arr.children.findIndex((c) => c.type === ']');
    const path = `${arr.pointer}/children/${closeIdx}`;
    const result = await applyPatch(tree, [{ op: 'add', path, value: ', not json' }]);
    expect(result.status).toBe('error');
  });
});

describe('applyPatch — test', () => {
  it('passes when node text matches', async () => {
    const tree = await jsonTree('{"a": 1}');
    const valuePath = findFirstValuePath(tree);
    const result = await applyPatch(tree, [{ op: 'test', path: valuePath, value: '1' }]);
    expect(result.status).toBe('ok');
    if (result.status !== 'ok') return;
    expect(result.tree.source).toBe('{"a": 1}');
  });

  it('rejects when node text differs', async () => {
    const tree = await jsonTree('{"a": 1}');
    const valuePath = findFirstValuePath(tree);
    const result = await applyPatch(tree, [{ op: 'test', path: valuePath, value: '2' }]);
    expect(result.status).toBe('error');
    if (result.status !== 'error') return;
    expect(result.reason).toMatch(/test failed/);
  });
});

describe('applyPatch — atomic batches', () => {
  it('applies multiple ops sequentially', async () => {
    const tree = await jsonTree('{"a": 1, "b": 2}');
    const aValue = findFirstValuePath(tree);
    const bValue = findValuePath(tree, 1);
    const result = await applyPatch(tree, [
      { op: 'replace', path: aValue, value: '10' },
      { op: 'replace', path: bValue, value: '20' },
    ]);
    expect(result.status).toBe('ok');
    if (result.status !== 'ok') return;
    expect(result.tree.source).toBe('{"a": 10, "b": 20}');
    expect(result.tree.version).toBe(2);
  });

  it('rolls back the original tree on failure', async () => {
    const tree = await jsonTree('{"a": 1, "b": 2}');
    const aValue = findFirstValuePath(tree);
    const bValue = findValuePath(tree, 1);
    const result = await applyPatch(tree, [
      { op: 'replace', path: aValue, value: '10' },
      { op: 'replace', path: bValue, value: 'BROKEN' },
    ]);
    expect(result.status).toBe('error');
    // Original tree is untouched.
    expect(tree.source).toBe('{"a": 1, "b": 2}');
    expect(tree.version).toBe(0);
  });
});

describe('applyPatch — optimistic locking', () => {
  it('returns conflict on version mismatch', async () => {
    const tree = await jsonTree('{}');
    const result = await applyPatch(tree, [{ op: 'replace', path: '', value: '{"a": 1}' }], {
      expectedVersion: 5,
    });
    expect(result.status).toBe('conflict');
    if (result.status !== 'conflict') return;
    expect(result.expectedVersion).toBe(5);
    expect(result.actualVersion).toBe(0);
  });

  it('proceeds when version matches', async () => {
    const tree = await jsonTree('{}');
    const result = await applyPatch(tree, [{ op: 'replace', path: '', value: '{"a": 1}' }], {
      expectedVersion: 0,
    });
    expect(result.status).toBe('ok');
    if (result.status !== 'ok') return;
    expect(result.tree.version).toBe(1);
  });
});

describe('applyPatch — move', () => {
  it('rejects move of the root', async () => {
    const tree = await jsonTree('[42]');
    const result = await applyPatch(tree, [{ op: 'move', from: '', path: '/children/0' }]);
    expect(result.status).toBe('error');
  });

  it('rejects a move that breaks the grammar', async () => {
    // Move the literal `42` from the array to a parent slot — the result is invalid JSON
    // and the ERROR guard must reject.
    const tree = await jsonTree('[42, 0]');
    const arr = tree.root.children[0]!;
    const fortyTwo = arr.children.find((c) => c.text === '42')!;
    const closeIdx = arr.children.findIndex((c) => c.type === ']');
    const result = await applyPatch(tree, [
      { op: 'move', from: fortyTwo.pointer, path: `${arr.pointer}/children/${closeIdx}` },
    ]);
    expect(result.status).toBe('error');
  });
});

// ---------- helpers ----------

function findFirstValuePath(tree: LxTree): string {
  return findValuePath(tree, 0);
}

function findValuePath(tree: LxTree, pairIndex: number): string {
  const obj = tree.root.children[0]!;
  let seen = 0;
  for (const c of obj.children) {
    if (c.type !== 'pair') continue;
    if (seen === pairIndex) {
      // value is the last named child of pair
      const valueChild = c.children[c.children.length - 1]!;
      return valueChild.pointer;
    }
    seen++;
  }
  throw new Error('no pair');
}
