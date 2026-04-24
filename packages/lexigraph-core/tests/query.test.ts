import { describe, expect, it } from 'vitest';

import { executeQuery, parse, QueryCompileError } from '../src/index.js';

describe('executeQuery (S-expression)', () => {
  it('captures top-level keys in a JSON object', async () => {
    const tree = await parse({
      languageId: 'json',
      source: '{"a": 1, "b": 2, "c": 3}',
    });
    const result = await executeQuery(tree, '(pair key: (string) @key)');
    expect(result.total).toBe(3);
    expect(result.matches).toHaveLength(3);
    const keys = result.matches.map((m) => m.captures[0]!.node.text);
    expect(keys).toEqual(['"a"', '"b"', '"c"']);
    for (const m of result.matches) {
      expect(m.captures[0]!.name).toBe('key');
      expect(m.captures[0]!.node.pointer).toMatch(/^\/children\//);
    }
  });

  it('captures function declarations in JavaScript', async () => {
    const tree = await parse({
      languageId: 'javascript',
      source: 'function foo() {}\nfunction bar() {}\nfunction baz() {}\n',
    });
    const result = await executeQuery(tree, '(function_declaration name: (identifier) @name)');
    expect(result.matches.map((m) => m.captures[0]!.node.text)).toEqual(['foo', 'bar', 'baz']);
  });

  it('paginates with offset and limit', async () => {
    const tree = await parse({
      languageId: 'json',
      source: '[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]',
    });
    const all = await executeQuery(tree, '(number) @n');
    expect(all.total).toBe(10);
    expect(all.matches).toHaveLength(10);

    const page = await executeQuery(tree, '(number) @n', { offset: 3, limit: 4 });
    expect(page.total).toBe(10);
    expect(page.matches).toHaveLength(4);
    expect(page.matches.map((m) => m.captures[0]!.node.text)).toEqual(['4', '5', '6', '7']);
  });

  it('scopes by anchor pointer', async () => {
    const tree = await parse({
      languageId: 'json',
      source: '{"outer": [1, 2], "other": [3, 4, 5]}',
    });
    const allNumbers = await executeQuery(tree, '(number) @n');
    expect(allNumbers.total).toBe(5);

    // Find the array under "outer" by pointer.
    const root = tree.root;
    const obj = root.children[0]!;
    expect(obj.type).toBe('object');
    const firstPair = obj.children.find((c) => c.type === 'pair')!;
    const firstArr = firstPair.children.find((c) => c.type === 'array')!;
    const scoped = await executeQuery(tree, '(number) @n', { pointer: firstArr.pointer });
    expect(scoped.total).toBe(2);
    expect(scoped.matches.map((m) => m.captures[0]!.node.text)).toEqual(['1', '2']);
  });

  it('throws QueryCompileError on bad query syntax', async () => {
    const tree = await parse({ languageId: 'json', source: '{}' });
    await expect(executeQuery(tree, '(this is not (((')).rejects.toBeInstanceOf(QueryCompileError);
  });

  it('returns empty result when anchor pointer does not exist', async () => {
    const tree = await parse({ languageId: 'json', source: '{}' });
    const r = await executeQuery(tree, '(object) @o', { pointer: '/children/99' });
    expect(r.total).toBe(0);
    expect(r.matches).toHaveLength(0);
  });

  it('captures multiple names per match', async () => {
    const tree = await parse({ languageId: 'json', source: '{"k": 42}' });
    const r = await executeQuery(tree, '(pair key: (string) @k value: (number) @v)');
    expect(r.matches).toHaveLength(1);
    const names = r.matches[0]!.captures.map((c) => c.name).sort();
    expect(names).toEqual(['k', 'v']);
  });
});
