// JSON-Patch-style mutations against an LxTree.
//
// Ops are applied sequentially; each op re-parses the document so subsequent pointers
// resolve against the post-op tree. The whole batch is atomic — any failure leaves the
// caller's tree untouched. Optimistic locking via `expectedVersion`. Patches that
// introduce new ERROR / MISSING nodes overlapping the edit zone are rejected.

import { LANGUAGES } from './languages.js';
import { parse } from './parser.js';
import { parsePointer } from './pointer.js';
import { LxTree, type DiagnosticNode } from './tree.js';
export type PatchOp =
  | { readonly op: 'replace'; readonly path: string; readonly value: string }
  | { readonly op: 'remove'; readonly path: string }
  | { readonly op: 'add'; readonly path: string; readonly value: string }
  | { readonly op: 'test'; readonly path: string; readonly value: string }
  | { readonly op: 'move'; readonly from: string; readonly path: string };

export interface ApplyPatchOptions {
  /** When set, reject the patch with a `conflict` if `tree.version !== expectedVersion`. */
  readonly expectedVersion?: number;
}

export type PatchResult =
  | { readonly status: 'ok'; readonly tree: LxTree; readonly version: number }
  | {
      readonly status: 'conflict';
      readonly expectedVersion: number;
      readonly actualVersion: number;
    }
  | {
      readonly status: 'error';
      readonly opIndex: number;
      readonly reason: string;
      readonly diagnostics?: readonly DiagnosticNode[];
    };

/** Apply a sequence of patch ops atomically, returning a new LxTree on success. */
export async function applyPatch(
  tree: LxTree,
  ops: readonly PatchOp[],
  options: ApplyPatchOptions = {},
): Promise<PatchResult> {
  if (options.expectedVersion !== undefined && options.expectedVersion !== tree.version) {
    return {
      status: 'conflict',
      expectedVersion: options.expectedVersion,
      actualVersion: tree.version,
    };
  }

  let current: LxTree = tree;
  const wasClean = !tree.hasError();

  for (let i = 0; i < ops.length; i++) {
    const op = ops[i]!;
    let edit: TextEdit | TextEdit[] | { error: string };
    try {
      edit = planOp(current, op);
    } catch (err) {
      return {
        status: 'error',
        opIndex: i,
        reason: err instanceof Error ? err.message : String(err),
      };
    }
    if ('error' in edit) {
      return { status: 'error', opIndex: i, reason: edit.error };
    }

    const edits = Array.isArray(edit) ? edit : [edit];
    let nextSource = current.source;
    // Apply edits right-to-left so earlier byte indices remain valid.
    const sorted = [...edits].sort((a, b) => b.startByte - a.startByte);
    for (const e of sorted) {
      nextSource = nextSource.slice(0, e.startByte) + e.text + nextSource.slice(e.endByte);
    }

    const fresh = await parse({ languageId: current._language.id as never, source: nextSource });
    current = new LxTree(
      LANGUAGES[current._language.id as keyof typeof LANGUAGES],
      nextSource,
      fresh.nativeTree,
      current.version + 1,
    );
  }

  // ERROR guard: if the tree was clean before, it must remain clean after the batch.
  // (Patches against an already-broken tree are allowed to leave it broken; they just
  // can't introduce *new* failure modes when the input was healthy.)
  if (wasClean && current.hasError()) {
    return {
      status: 'error',
      opIndex: ops.length - 1,
      reason: 'patch introduced grammar errors',
      diagnostics: current.diagnostics(),
    };
  }

  return { status: 'ok', tree: current, version: current.version };
}

interface TextEdit {
  readonly startByte: number;
  readonly endByte: number;
  readonly text: string;
}

function planOp(tree: LxTree, op: PatchOp): TextEdit | TextEdit[] | { error: string } {
  switch (op.op) {
    case 'replace': {
      const node = tree.resolve(op.path);
      if (!node) return { error: `path not found: ${op.path}` };
      return { startByte: node.range.startByte, endByte: node.range.endByte, text: op.value };
    }
    case 'remove': {
      if (op.path === '') return { error: 'cannot remove root' };
      const node = tree.resolve(op.path);
      if (!node) return { error: `path not found: ${op.path}` };
      return { startByte: node.range.startByte, endByte: node.range.endByte, text: '' };
    }
    case 'add': {
      const insert = resolveInsertionPoint(tree, op.path);
      if ('error' in insert) return insert;
      return { startByte: insert.byte, endByte: insert.byte, text: op.value };
    }
    case 'test': {
      const node = tree.resolve(op.path);
      if (!node) return { error: `test failed: path not found: ${op.path}` };
      if (node.text !== op.value) {
        return {
          error: `test failed at ${op.path}: expected ${JSON.stringify(op.value)}, got ${JSON.stringify(node.text)}`,
        };
      }
      // A passing test is a no-op edit (zero-length range, empty text).
      return { startByte: node.range.startByte, endByte: node.range.startByte, text: '' };
    }
    case 'move': {
      const fromNode = tree.resolve(op.from);
      if (!fromNode) return { error: `move source not found: ${op.from}` };
      if (op.from === '') return { error: 'cannot move root' };
      const insert = resolveInsertionPoint(tree, op.path);
      if ('error' in insert) return insert;
      const text = fromNode.text;
      // Two edits: remove from + insert at path. Sorted right-to-left at apply time.
      const removeEdit: TextEdit = {
        startByte: fromNode.range.startByte,
        endByte: fromNode.range.endByte,
        text: '',
      };
      const insertEdit: TextEdit = {
        startByte: insert.byte,
        endByte: insert.byte,
        text,
      };
      // Reject overlapping move (insertion inside removed range).
      if (insert.byte > fromNode.range.startByte && insert.byte < fromNode.range.endByte) {
        return { error: 'move target overlaps source range' };
      }
      return [removeEdit, insertEdit];
    }
  }
}

function resolveInsertionPoint(tree: LxTree, path: string): { byte: number } | { error: string } {
  const tokens = parsePointer(path);
  if (tokens.length < 2) return { error: `add path must address a child slot: ${path}` };
  const lastIdxTok = tokens[tokens.length - 1]!;
  const lastKey = tokens[tokens.length - 2]!;
  if (lastKey !== 'children') return { error: `add path must end in /children/<N>: ${path}` };
  const idx = Number(lastIdxTok);
  if (!Number.isInteger(idx) || idx < 0) {
    return { error: `add path index must be a non-negative integer: ${path}` };
  }
  const parentTokens = tokens.slice(0, -2);
  const parentPath =
    parentTokens.length === 0 ? '' : '/' + parentTokens.map(escapeForPointer).join('/');
  const parent = tree.resolve(parentPath);
  if (!parent) return { error: `add parent not found: ${parentPath}` };

  if (idx > parent.children.length) {
    return {
      error: `add index ${idx} out of range (parent has ${parent.children.length} children)`,
    };
  }
  const byte =
    idx < parent.children.length ? parent.children[idx]!.range.startByte : parent.range.endByte;
  return { byte };
}

function escapeForPointer(token: string): string {
  return token.replace(/~/g, '~0').replace(/\//g, '~1');
}
