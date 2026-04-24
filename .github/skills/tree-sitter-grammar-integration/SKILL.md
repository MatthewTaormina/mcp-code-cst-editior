---
name: tree-sitter-grammar-integration
description: 'Add a new language to @lexigraph/core via web-tree-sitter. Use when integrating, upgrading, or debugging a tree-sitter WASM grammar (JavaScript, TypeScript, Python, Rust, CSS, HTML, JSON, YAML, TOML, Markdown, or any new grammar). Covers WASM acquisition, parser registry wiring, lossless round-trip corpus, and example queries.'
---

# Tree-sitter Grammar Integration

Repeatable workflow for adding a new language (or upgrading an existing one) to
`@lexigraph/core`. The lossless round-trip test is the **gate** — a grammar is not
"integrated" until its corpus passes byte-exact round-trip.

## When to Use

- Adding language support beyond the v1 ten (JS, TS, JSON, CSS, HTML, Rust, Python, Markdown, YAML, TOML)
- Upgrading the version of an already-integrated grammar
- Debugging grammar errors after parser changes

## Prerequisites

- `web-tree-sitter` is the only runtime parser API used in this repo.
- Grammars are shipped as `.wasm` files under `packages/lexigraph-core/grammars/`.
- Each language has a stable identifier in `packages/lexigraph-core/src/languages.ts`.

## Procedure

### 1. Acquire the WASM grammar

Two acceptable sources, in order of preference:

1. The grammar's official npm package (`tree-sitter-<lang>`) — build the `.wasm` with
   `tree-sitter build --wasm` (requires Emscripten or Docker).
2. A pinned upstream release (`https://github.com/tree-sitter/tree-sitter-<lang>/releases`).

Pin the version in `packages/lexigraph-core/grammars/MANIFEST.json` (sha256 + version).
Never check in a grammar without recording its provenance.

### 2. Register the language

Edit `packages/lexigraph-core/src/languages.ts`:

```ts
export const LANGUAGES = {
  // ...existing
  ruby: {
    id: 'ruby',
    extensions: ['.rb'],
    wasm: 'tree-sitter-ruby.wasm',
    namedNodeWhitelist: undefined, // optional perf hint
  },
} as const satisfies Record<string, LanguageDef>;
```

### 3. Add the round-trip corpus

Drop 20+ representative real-world files (varied sizes, comments, edge-cases) into
`packages/lexigraph-core/tests/corpus/<lang>/`. Include at minimum:

- A trivial "hello world"
- A file with comments at every legal position
- A file with deeply nested constructs
- A file with the language's most awkward whitespace rules (heredocs, template literals, indentation-sensitive syntax)

### 4. Wire the round-trip test

`tests/roundtrip.test.ts` enumerates corpus directories. New language is picked up
automatically. Run:

```bash
pnpm --filter @lexigraph/core test -- roundtrip
```

**Every file must round-trip byte-exactly.** Failures usually mean the grammar drops
trivia — file an upstream issue and pin to the last known-good version.

### 5. Add a property test

`tests/roundtrip.property.test.ts` uses `fast-check` to mutate input. Add the new
language to its language matrix.

### 6. Add example queries (documentation)

`packages/lexigraph-core/grammars/<lang>/examples.md` — at least 3 useful S-expression
queries an AI might run (e.g. find all functions, find all imports, find all string
literals).

### 7. Update docs

- Append the language to the table in [AGENTS.md](../../../AGENTS.md) and root README (when it exists).
- Bump `@lexigraph/core`'s changeset (`pnpm changeset` once configured).

## Verification Checklist

- [ ] WASM file present + recorded in `MANIFEST.json` with version + sha256
- [ ] `LANGUAGES` registry updated
- [ ] ≥ 20 corpus files added
- [ ] `pnpm --filter @lexigraph/core test` is green
- [ ] Property test includes the new language
- [ ] Example queries documented
- [ ] Docs updated

## Common Pitfalls

- **Grammar drops whitespace/comments.** Round-trip will fail. Don't silence the test —
  pin to a working version or skip the language until upstream fixes it.
- **`.wasm` built with wrong tree-sitter version.** API mismatch crashes at parser load.
  Use the version from `web-tree-sitter`'s peer compat table.
- **Adding a language without a corpus.** A passing test on one file proves nothing.
  20+ files minimum.
