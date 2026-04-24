// S-expression query engine — wraps web-tree-sitter's Query against an LxTree.
//
// Returns matches whose captures reference Lexigraph nodes (with JSON Pointer addresses)
// instead of raw tree-sitter nodes. Supports byte-range scoping and pagination.

import { Query, type Node as TSNode } from 'web-tree-sitter';

import { buildLxNode, nonNullChildren, type LxNode } from './node.js';
import { loadLanguageById } from './parser.js';
import type { LxTree } from './tree.js';

export interface QueryCapture {
  /** Capture name from the S-expression (without the leading `@`). */
  readonly name: string;
  /** The captured node, materialized as an LxNode. */
  readonly node: LxNode;
}

export interface QueryMatch {
  /** Index of the pattern that matched within the query source. */
  readonly patternIndex: number;
  readonly captures: readonly QueryCapture[];
}

export interface QueryOptions {
  /** Anchor matching to a sub-tree. Defaults to the tree root. */
  readonly pointer?: string;
  /** Byte-range scoping (inclusive start, exclusive end). */
  readonly startByte?: number;
  readonly endByte?: number;
  /** Pagination: skip the first N matches. */
  readonly offset?: number;
  /** Pagination: return at most N matches. */
  readonly limit?: number;
  /**
   * Cap on raw matches the underlying engine considers before pagination.
   * Defaults to 10_000 to bound memory on broad queries.
   */
  readonly matchLimit?: number;
}

export interface QueryResult {
  readonly matches: readonly QueryMatch[];
  /** Total matches before offset/limit was applied. */
  readonly total: number;
  /** True when the engine truncated results because matchLimit was hit. */
  readonly truncated: boolean;
}

export class QueryCompileError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'QueryCompileError';
  }
}

/**
 * Compile and execute a tree-sitter S-expression query against a parsed tree.
 *
 * The Query object is freed before this function returns; callers don't manage lifecycle.
 */
export async function executeQuery(
  tree: LxTree,
  source: string,
  options: QueryOptions = {},
): Promise<QueryResult> {
  const language = await loadLanguageById(
    tree.languageId as Parameters<typeof loadLanguageById>[0],
  );

  let query: Query;
  try {
    query = new Query(language, source);
  } catch (err) {
    throw new QueryCompileError(
      err instanceof Error ? err.message : `failed to compile query: ${String(err)}`,
    );
  }

  try {
    const anchor = resolveAnchor(tree, options.pointer);
    if (!anchor) {
      return { matches: [], total: 0, truncated: false };
    }

    const matchLimit = options.matchLimit ?? 10_000;
    const queryOpts: { matchLimit: number; startIndex?: number; endIndex?: number } = {
      matchLimit,
    };
    if (options.startByte !== undefined) queryOpts.startIndex = options.startByte;
    if (options.endByte !== undefined) queryOpts.endIndex = options.endByte;
    const rawMatches = query.matches(anchor, queryOpts);

    const truncated = query.didExceedMatchLimit();
    const total = rawMatches.length;

    const offset = Math.max(0, options.offset ?? 0);
    const limit = options.limit ?? total;
    const sliced = rawMatches.slice(offset, offset + Math.max(0, limit));

    const matches: QueryMatch[] = sliced.map((m) => ({
      patternIndex: m.patternIndex,
      captures: m.captures.map((c) => ({
        name: c.name,
        node: lxNodeFor(tree, c.node),
      })),
    }));

    return { matches, total, truncated };
  } finally {
    query.delete();
  }
}

function resolveAnchor(tree: LxTree, pointer: string | undefined): TSNode | undefined {
  if (!pointer) return tree.nativeTree.rootNode;
  // Walk the native tree using the same /children/<index> convention used by LxTree.
  // We can't reuse LxTree.resolve because it returns LxNode and we need TSNode.
  const segments = pointer.split('/').slice(1);
  let n: TSNode = tree.nativeTree.rootNode;
  for (let i = 0; i < segments.length; i += 2) {
    if (segments[i] !== 'children') return undefined;
    const idxTok = segments[i + 1];
    if (idxTok === undefined) return undefined;
    const idx = Number(idxTok);
    if (!Number.isInteger(idx) || idx < 0) return undefined;
    const kids = nonNullChildren(n);
    const next = kids[idx];
    if (!next) return undefined;
    n = next;
  }
  return n;
}

/**
 * Compute the JSON Pointer for a tree-sitter node by walking up to the root, recording
 * each step as the node's index among its parent's non-null children. Then materialize
 * the LxNode subtree rooted there.
 */
function lxNodeFor(tree: LxTree, node: TSNode): LxNode {
  const path: number[] = [];
  let current: TSNode | null = node;
  while (current && current.parent) {
    const parent: TSNode = current.parent;
    const siblings = nonNullChildren(parent);
    const target: TSNode = current;
    const idx = siblings.findIndex((s) => s.id === target.id);
    if (idx < 0) {
      // Detached node — fall back to building from the node itself with empty pointer.
      return buildLxNode(node);
    }
    path.push(idx);
    current = parent;
  }
  path.reverse();

  const pointer = path.length === 0 ? '' : path.map((i) => `/children/${i}`).join('');
  // Walk the LxTree to the same address; this is cheaper than rebuilding the subtree.
  const lx = tree.resolve(pointer);
  return lx ?? buildLxNode(node);
}
