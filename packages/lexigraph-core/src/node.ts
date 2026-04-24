// Uniform Lexigraph Node model + lossless reconstruction.

import { createHash } from 'node:crypto';

import type { Node as TSNode } from 'web-tree-sitter';

import { childPointer } from './pointer.js';

export interface LxPosition {
  readonly row: number;
  readonly column: number;
}

export interface LxRange {
  readonly startByte: number;
  readonly endByte: number;
  readonly startPosition: LxPosition;
  readonly endPosition: LxPosition;
}

export interface LxNode {
  /**
   * Hybrid stable ID: `<sha256(text)[0..16]>:<pointer>`. The hash component lets a
   * caller refer to "the function I just inserted" even after edits move its pointer;
   * the pointer component disambiguates duplicates within a tree.
   */
  readonly id: string;
  readonly type: string;
  readonly pointer: string;
  /** Tree-sitter field name when this node is the value of a parent field, else null. */
  readonly field: string | null;
  readonly isNamed: boolean;
  readonly isExtra: boolean;
  readonly isError: boolean;
  readonly isMissing: boolean;
  readonly range: LxRange;
  /** Verbatim source text of this node (always available). */
  readonly text: string;
  readonly children: readonly LxNode[];
}

/** Filter out the nulls that `web-tree-sitter` returns inside `children`. */
export function nonNullChildren(node: TSNode): TSNode[] {
  const out: TSNode[] = [];
  for (const c of node.children) if (c !== null) out.push(c);
  return out;
}

function rangeOf(n: TSNode): LxRange {
  return {
    startByte: n.startIndex,
    endByte: n.endIndex,
    startPosition: { row: n.startPosition.row, column: n.startPosition.column },
    endPosition: { row: n.endPosition.row, column: n.endPosition.column },
  };
}

function hashId(text: string, pointer: string): string {
  const h = createHash('sha256').update(text).digest('hex').slice(0, 16);
  return `${h}:${pointer}`;
}

/**
 * Build an LxNode tree from a tree-sitter root. Eager construction — fine for files up
 * to the size limits the MCP tools enforce; lazy construction is a Phase 2 optimization
 * if profiling demands it.
 */
export function buildLxNode(root: TSNode): LxNode {
  return walk(root, '', null);
}

function walk(n: TSNode, pointer: string, field: string | null): LxNode {
  const kids = nonNullChildren(n);
  const children: LxNode[] = new Array(kids.length);
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i]!;
    const childField = n.fieldNameForChild(i);
    children[i] = walk(k, childPointer(pointer, i), childField);
  }
  const text = n.text;
  return {
    id: hashId(text, pointer),
    type: n.type,
    pointer,
    field,
    isNamed: n.isNamed,
    isExtra: n.isExtra,
    isError: n.isError,
    isMissing: n.isMissing,
    range: rangeOf(n),
    text,
    children,
  };
}

/**
 * Lossless reconstruction of source text from a tree-sitter node, using the original
 * source string to recover whitespace gaps between children.
 *
 * Round-trip invariant (verified per language in tests):
 *   `reconstruct(source, parse(source).rootNode) === source`
 */
export function reconstruct(source: string, node: TSNode): string {
  const kids = nonNullChildren(node);
  if (kids.length === 0) return source.slice(node.startIndex, node.endIndex);
  let out = '';
  let cursor = node.startIndex;
  for (const k of kids) {
    if (k.startIndex > cursor) out += source.slice(cursor, k.startIndex);
    out += reconstruct(source, k);
    cursor = k.endIndex;
  }
  if (node.endIndex > cursor) out += source.slice(cursor, node.endIndex);
  return out;
}
