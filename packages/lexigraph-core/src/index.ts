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

export const VERSION = '0.0.0';
