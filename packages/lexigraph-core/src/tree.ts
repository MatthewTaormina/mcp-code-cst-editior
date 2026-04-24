// LxTree — wraps a native tree-sitter Tree with lossless serialization,
// JSON Pointer addressing, and basic diagnostics.

import type { Tree as TSTree, Node as TSNode } from 'web-tree-sitter';

import type { LanguageDef } from './languages.js';
import { buildLxNode, nonNullChildren, reconstruct, type LxNode } from './node.js';
import { parsePointer } from './pointer.js';

export interface DiagnosticNode {
  readonly type: 'error' | 'missing';
  readonly pointer: string;
  readonly nodeType: string;
  readonly startByte: number;
  readonly endByte: number;
}

export class LxTree {
  readonly languageId: string;
  readonly source: string;
  /**
   * Monotonically-increasing version number. A freshly parsed tree starts at 0; each
   * successful patch produces a new tree with `version + 1`. Used for optimistic locking.
   */
  readonly version: number;

  /** Internal native tree-sitter tree. Public for advanced consumers; do not mutate. */
  readonly nativeTree: TSTree;

  /** Language definition this tree was parsed with. Internal use. */
  readonly _language: LanguageDef;

  private _root: LxNode | null = null;

  constructor(language: LanguageDef, source: string, nativeTree: TSTree, version = 0) {
    this.languageId = language.id;
    this.source = source;
    this.nativeTree = nativeTree;
    this.version = version;
    this._language = language;
  }

  /** Materialize and cache the full LxNode tree. */
  get root(): LxNode {
    if (!this._root) this._root = buildLxNode(this.nativeTree.rootNode);
    return this._root;
  }

  /** Lossless serialization: must equal `this.source`. */
  serialize(): string {
    return reconstruct(this.source, this.nativeTree.rootNode);
  }

  /**
   * Resolve a JSON Pointer to an LxNode. Throws on malformed pointer; returns undefined
   * when the path doesn't exist in this tree.
   */
  resolve(pointer: string): LxNode | undefined {
    const tokens = parsePointer(pointer);
    let node: LxNode = this.root;
    for (let i = 0; i < tokens.length; i += 2) {
      const key = tokens[i];
      const idxTok = tokens[i + 1];
      if (key !== 'children') return undefined;
      if (idxTok === undefined) return undefined;
      const idx = Number(idxTok);
      if (!Number.isInteger(idx) || idx < 0 || idx >= node.children.length) return undefined;
      const next = node.children[idx];
      if (!next) return undefined;
      node = next;
    }
    return node;
  }

  /** Enumerate ERROR / MISSING nodes anywhere in the tree. */
  diagnostics(): DiagnosticNode[] {
    const out: DiagnosticNode[] = [];
    walkDiagnostics(this.nativeTree.rootNode, '', out);
    return out;
  }

  /** True when the tree contains any ERROR or MISSING nodes. */
  hasError(): boolean {
    return this.nativeTree.rootNode.hasError;
  }
}

function walkDiagnostics(n: TSNode, pointer: string, out: DiagnosticNode[]): void {
  if (n.isError) {
    out.push({
      type: 'error',
      pointer,
      nodeType: n.type,
      startByte: n.startIndex,
      endByte: n.endIndex,
    });
  } else if (n.isMissing) {
    out.push({
      type: 'missing',
      pointer,
      nodeType: n.type,
      startByte: n.startIndex,
      endByte: n.endIndex,
    });
  }
  const kids = nonNullChildren(n);
  for (let i = 0; i < kids.length; i++) {
    walkDiagnostics(kids[i]!, `${pointer}/children/${i}`, out);
  }
}
