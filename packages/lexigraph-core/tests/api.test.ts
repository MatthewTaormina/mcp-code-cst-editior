import { describe, expect, it } from 'vitest';

import {
  buildPointer,
  childPointer,
  decodeToken,
  detectLanguage,
  encodeToken,
  parse,
  parsePointer,
} from '../src/index.js';

describe('JSON Pointer encoding (RFC 6901)', () => {
  it('encodes ~ and /', () => {
    expect(encodeToken('a/b')).toBe('a~1b');
    expect(encodeToken('a~b')).toBe('a~0b');
    expect(encodeToken('a~/b')).toBe('a~0~1b');
  });

  it('decodes ~ and /', () => {
    expect(decodeToken('a~1b')).toBe('a/b');
    expect(decodeToken('a~0b')).toBe('a~b');
    // Order matters: ~01 is "~1" not "/", because we decode left-to-right per spec.
    expect(decodeToken('a~01b')).toBe('a~1b');
  });

  it('parses and rebuilds pointers', () => {
    expect(parsePointer('')).toEqual([]);
    expect(parsePointer('/children/0')).toEqual(['children', '0']);
    expect(buildPointer([])).toBe('');
    expect(buildPointer(['children', '3'])).toBe('/children/3');
  });

  it('rejects malformed pointers', () => {
    expect(() => parsePointer('children/0')).toThrow(/JSON Pointer/);
  });

  it('childPointer composes', () => {
    expect(childPointer('', 0)).toBe('/children/0');
    expect(childPointer('/children/2', 5)).toBe('/children/2/children/5');
  });
});

describe('language detection', () => {
  it('maps common extensions', () => {
    expect(detectLanguage('foo.ts')?.id).toBe('typescript');
    expect(detectLanguage('foo.tsx')?.id).toBe('tsx');
    expect(detectLanguage('foo.js')?.id).toBe('javascript');
    expect(detectLanguage('foo.json')?.id).toBe('json');
    expect(detectLanguage('foo.toml')?.id).toBe('toml');
    expect(detectLanguage('foo.unknown')).toBeUndefined();
  });
});

describe('LxTree.resolve', () => {
  it('returns the root for the empty pointer', async () => {
    const tree = await parse({ languageId: 'json', source: '{"a": 1}' });
    const root = tree.resolve('');
    expect(root?.pointer).toBe('');
    expect(root?.type).toBe('document');
  });

  it('resolves into the children path', async () => {
    const tree = await parse({ languageId: 'json', source: '[1, 2, 3]' });
    const root = tree.root;
    const arrayNode = root.children[0]!;
    expect(arrayNode.type).toBe('array');
    const resolved = tree.resolve(arrayNode.pointer);
    expect(resolved?.type).toBe('array');
  });

  it('returns undefined for out-of-range indices', async () => {
    const tree = await parse({ languageId: 'json', source: '{}' });
    expect(tree.resolve('/children/99')).toBeUndefined();
  });

  it('node.id starts with a 16-char hex hash', async () => {
    const tree = await parse({ languageId: 'json', source: '{"a": 1}' });
    expect(tree.root.id).toMatch(/^[0-9a-f]{16}:$/);
  });
});

describe('diagnostics', () => {
  it('flags syntax errors', async () => {
    const tree = await parse({ languageId: 'json', source: '{"a": ' });
    const diags = tree.diagnostics();
    expect(diags.length).toBeGreaterThan(0);
    expect(tree.hasError()).toBe(true);
  });

  it('returns empty diagnostics for well-formed input', async () => {
    const tree = await parse({ languageId: 'json', source: '{"a": 1}' });
    expect(tree.diagnostics()).toEqual([]);
    expect(tree.hasError()).toBe(false);
  });
});
