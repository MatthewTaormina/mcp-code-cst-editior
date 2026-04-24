// Parser pool + parse() entry point. One Parser instance per language, cached.

import { readFileSync } from 'node:fs';

import { Language, Parser } from 'web-tree-sitter';

import { LANGUAGES, type LanguageDef, type LanguageId } from './languages.js';
import { LxTree } from './tree.js';
import { grammarPath, runtimeWasmPath } from './wasm.js';

let initPromise: Promise<void> | null = null;

async function ensureInit(): Promise<void> {
  if (initPromise) return initPromise;
  const wasmFile = runtimeWasmPath();
  initPromise = Parser.init({
    locateFile(name: string): string {
      // The Emscripten runtime asks for "tree-sitter.wasm". Always return our resolved path.
      return name === 'tree-sitter.wasm' ? wasmFile : name;
    },
  });
  return initPromise;
}

const languageCache = new Map<string, Promise<Language>>();
const parserCache = new Map<string, Parser>();

async function loadLanguage(def: LanguageDef): Promise<Language> {
  let p = languageCache.get(def.id);
  if (!p) {
    p = (async () => {
      await ensureInit();
      const bytes = readFileSync(grammarPath(def));
      return Language.load(new Uint8Array(bytes));
    })();
    languageCache.set(def.id, p);
  }
  return p;
}

/** Internal: load (and cache) the native tree-sitter Language for a registered language. */
export async function loadLanguageById(id: LanguageId): Promise<Language> {
  const def = LANGUAGES[id];
  if (!def) throw new Error(`lexigraph: unknown language id: ${String(id)}`);
  return loadLanguage(def);
}

async function getParser(def: LanguageDef): Promise<Parser> {
  const cached = parserCache.get(def.id);
  if (cached) return cached;
  const lang = await loadLanguage(def);
  const parser = new Parser();
  parser.setLanguage(lang);
  parserCache.set(def.id, parser);
  return parser;
}

export interface ParseOptions {
  readonly languageId: LanguageId;
  readonly source: string;
}

/** Parse a source string into an LxTree. */
export async function parse(opts: ParseOptions): Promise<LxTree> {
  const def = LANGUAGES[opts.languageId];
  if (!def) {
    throw new Error(`lexigraph: unknown language id: ${String(opts.languageId)}`);
  }
  const parser = await getParser(def);
  const tree = parser.parse(opts.source);
  if (!tree) {
    throw new Error(`lexigraph: parser returned null for language ${def.id}`);
  }
  return new LxTree(def, opts.source, tree);
}
