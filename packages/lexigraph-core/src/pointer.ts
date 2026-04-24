// JSON Pointer (RFC 6901) — encode/decode + tree resolution.
//
// Lexigraph addresses CST nodes with a strict subset of JSON Pointer:
//   - The empty pointer "" addresses the root.
//   - The only object key used is "children".
//   - Children are indexed by their position in the FILTERED child array
//     (nulls returned by web-tree-sitter are removed first).
//
// Examples:
//   ""                          -> root
//   "/children/0"               -> first child of root
//   "/children/3/children/1"    -> second child of the fourth child of root

const TILDE = /~/g;
const SLASH = /\//g;
const TILDE_DECODE = /~[01]/g;

/** Encode a single reference token per RFC 6901 §4. */
export function encodeToken(token: string): string {
  return token.replace(TILDE, '~0').replace(SLASH, '~1');
}

/** Decode a single reference token per RFC 6901 §4. */
export function decodeToken(token: string): string {
  return token.replace(TILDE_DECODE, (m) => (m === '~1' ? '/' : '~'));
}

/** Parse a JSON Pointer into an array of decoded tokens. Throws on malformed input. */
export function parsePointer(pointer: string): string[] {
  if (pointer === '') return [];
  if (!pointer.startsWith('/')) {
    throw new Error(`invalid JSON Pointer (must start with "/" or be empty): ${pointer}`);
  }
  return pointer
    .slice(1)
    .split('/')
    .map((t) => decodeToken(t));
}

/** Build a pointer from a list of tokens (each will be encoded). */
export function buildPointer(tokens: readonly string[]): string {
  if (tokens.length === 0) return '';
  return '/' + tokens.map(encodeToken).join('/');
}

/** Append a child index to an existing pointer. */
export function childPointer(parent: string, index: number): string {
  return `${parent}/children/${index}`;
}
