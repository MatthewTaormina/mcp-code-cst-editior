import { describe, it, expect } from 'vitest';
import { VERSION } from '../src/index.js';

describe('lexigraph-core', () => {
  it('exports a version placeholder', () => {
    expect(VERSION).toBe('0.0.0');
  });
});
