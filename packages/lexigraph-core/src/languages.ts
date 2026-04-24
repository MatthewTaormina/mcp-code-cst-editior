// Lexigraph language registry.
// Adding a new language: see .github/skills/tree-sitter-grammar-integration/SKILL.md.

export interface LanguageDef {
  /** Stable identifier used by the public API. */
  readonly id: string;
  /** File extensions (lowercase, with leading dot) that map to this language. */
  readonly extensions: readonly string[];
  /** WASM file name under packages/lexigraph-core/grammars/. */
  readonly wasm: string;
}

export const LANGUAGES = {
  javascript: {
    id: 'javascript',
    extensions: ['.js', '.cjs', '.mjs', '.jsx'],
    wasm: 'tree-sitter-javascript.wasm',
  },
  typescript: {
    id: 'typescript',
    extensions: ['.ts', '.cts', '.mts'],
    wasm: 'tree-sitter-typescript.wasm',
  },
  tsx: {
    id: 'tsx',
    extensions: ['.tsx'],
    wasm: 'tree-sitter-tsx.wasm',
  },
  json: {
    id: 'json',
    extensions: ['.json'],
    wasm: 'tree-sitter-json.wasm',
  },
  css: {
    id: 'css',
    extensions: ['.css'],
    wasm: 'tree-sitter-css.wasm',
  },
  html: {
    id: 'html',
    extensions: ['.html', '.htm'],
    wasm: 'tree-sitter-html.wasm',
  },
  rust: {
    id: 'rust',
    extensions: ['.rs'],
    wasm: 'tree-sitter-rust.wasm',
  },
  python: {
    id: 'python',
    extensions: ['.py', '.pyi'],
    wasm: 'tree-sitter-python.wasm',
  },
  toml: {
    id: 'toml',
    extensions: ['.toml'],
    wasm: 'tree-sitter-toml.wasm',
  },
  // yaml: deferred — the prebuilt tree-sitter-yaml.wasm in tree-sitter-wasms@0.1.13
  //   triggers a "resolved is not a function" trap inside web-tree-sitter@0.25 (external
  //   scanner ABI mismatch). Reintroduce by building a fresh WASM from
  //   tree-sitter-yaml@latest with the matching tree-sitter CLI.
  // markdown: deferred — split-grammar (markdown + markdown-inline) requires a separate
  //   WASM source; tracked as a Phase 1 follow-up.
} as const satisfies Record<string, LanguageDef>;

export type LanguageId = keyof typeof LANGUAGES;

const EXT_INDEX: ReadonlyMap<string, LanguageId> = (() => {
  const m = new Map<string, LanguageId>();
  for (const [id, def] of Object.entries(LANGUAGES) as [LanguageId, LanguageDef][]) {
    for (const ext of def.extensions) m.set(ext.toLowerCase(), id);
  }
  return m;
})();

/** Detect a language from a file path by extension. Returns undefined when unknown. */
export function detectLanguage(filePath: string): LanguageDef | undefined {
  const lower = filePath.toLowerCase();
  const dot = lower.lastIndexOf('.');
  if (dot < 0) return undefined;
  const ext = lower.slice(dot);
  const id = EXT_INDEX.get(ext);
  return id ? LANGUAGES[id] : undefined;
}

/** Resolve a language by id. Throws on unknown id. */
export function getLanguage(id: LanguageId): LanguageDef {
  return LANGUAGES[id];
}
