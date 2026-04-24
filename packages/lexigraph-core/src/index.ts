// Public API for @lexigraph/core.

export { LANGUAGES, detectLanguage, getLanguage } from './languages.js';
export type { LanguageDef, LanguageId } from './languages.js';

export { parse } from './parser.js';
export type { ParseOptions } from './parser.js';

export { LxTree } from './tree.js';
export type { DiagnosticNode } from './tree.js';

export { buildLxNode, reconstruct } from './node.js';
export type { LxNode, LxPosition, LxRange } from './node.js';

export { encodeToken, decodeToken, parsePointer, buildPointer, childPointer } from './pointer.js';

export { executeQuery, QueryCompileError } from './query.js';
export type { QueryCapture, QueryMatch, QueryOptions, QueryResult } from './query.js';

export { applyPatch } from './patch.js';
export type { PatchOp, PatchResult, ApplyPatchOptions } from './patch.js';

export const VERSION = '0.0.0';
