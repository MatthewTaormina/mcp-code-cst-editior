// Workspace registry: tracks parsed files relative to a root directory.
// The path normalizer is the primary security boundary — `..` escapes are rejected.

import { readFile, writeFile } from 'node:fs/promises';
import { isAbsolute, join, relative, resolve, sep } from 'node:path';

import { LANGUAGES, detectLanguage, parse, type LanguageId, type LxTree } from '@lexigraph/core';

export interface TrackedFile {
  /** Workspace-relative POSIX path. Stable id used by all tools. */
  readonly path: string;
  /** Parsed tree. Replaced (with bumped version) by mutating tools. */
  tree: LxTree;
  /** Source as last loaded from disk OR last saved. Used to detect dirty state. */
  diskSource: string;
}

export interface TrackOptions {
  /** Override language detection. */
  readonly languageId?: string;
}

export class WorkspaceError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'WorkspaceError';
  }
}

export class Workspace {
  /** Absolute, fully-resolved root directory. */
  readonly root: string;

  private readonly files = new Map<string, TrackedFile>();

  constructor(root: string) {
    this.root = resolve(root);
  }

  /**
   * Normalize a user-supplied path into a workspace-relative POSIX path.
   * Rejects `..` escapes and paths that resolve outside the workspace root.
   */
  normalizePath(input: string): string {
    if (typeof input !== 'string' || input.length === 0) {
      throw new WorkspaceError('path must be a non-empty string');
    }
    const absolute = isAbsolute(input) ? resolve(input) : resolve(this.root, input);
    const rel = relative(this.root, absolute);
    if (rel === '' || rel.startsWith('..') || isAbsolute(rel)) {
      throw new WorkspaceError(`path escapes workspace: ${input}`);
    }
    return rel.split(sep).join('/');
  }

  /** Resolve a normalized workspace-relative path back to an absolute filesystem path. */
  absolutePath(normalized: string): string {
    return join(this.root, normalized);
  }

  /** Look up a tracked file by normalized path. */
  get(normalized: string): TrackedFile | undefined {
    return this.files.get(normalized);
  }

  /** All tracked files, in insertion order. */
  list(): readonly TrackedFile[] {
    return [...this.files.values()];
  }

  /** True when the in-memory tree has diverged from disk. */
  isDirty(file: TrackedFile): boolean {
    return file.tree.serialize() !== file.diskSource;
  }

  /**
   * Read the file from disk, parse it, and register (or refresh) it. Returns the entry.
   * If the file is already tracked, the new tree replaces the existing one with version 0.
   */
  async track(input: string, options: TrackOptions = {}): Promise<TrackedFile> {
    const path = this.normalizePath(input);
    const absolute = this.absolutePath(path);
    const source = await readFile(absolute, 'utf8');
    const languageId = options.languageId ?? detectLanguage(path)?.id;
    if (!languageId) {
      throw new WorkspaceError(
        `cannot detect language for ${path}; pass languageId explicitly (one of: ${Object.keys(LANGUAGES).join(', ')})`,
      );
    }
    if (!(languageId in LANGUAGES)) {
      throw new WorkspaceError(`unknown languageId: ${languageId}`);
    }
    const tree = await parse({ languageId: languageId as LanguageId, source });
    const entry: TrackedFile = { path, tree, diskSource: source };
    this.files.set(path, entry);
    return entry;
  }

  /** Stop tracking. Returns true when removed. */
  untrack(input: string): boolean {
    const path = this.normalizePath(input);
    return this.files.delete(path);
  }

  /** Replace the in-memory tree (used by mutating tools). */
  setTree(file: TrackedFile, tree: LxTree): void {
    file.tree = tree;
  }

  /** Persist the current tree to disk and update diskSource. */
  async save(file: TrackedFile): Promise<void> {
    const text = file.tree.serialize();
    await writeFile(this.absolutePath(file.path), text, 'utf8');
    file.diskSource = text;
  }
}
