---
description: 'Add a new language to @lexigraph/core end-to-end (grammar, registry, corpus, tests, docs).'
argument-hint: '<language-name> (e.g. ruby)'
agent: 'agent'
---

Add support for the language **${input:language}** to `@lexigraph/core`.

Follow the [tree-sitter-grammar-integration](../skills/tree-sitter-grammar-integration/SKILL.md)
skill end-to-end. Do not skip steps. Stop and report if the round-trip corpus fails — do
not silence the test.

Deliverables for this PR:

1. WASM grammar file under `packages/lexigraph-core/grammars/` with `MANIFEST.json` entry
2. Entry in `packages/lexigraph-core/src/languages.ts`
3. ≥ 20 real-world corpus files under `packages/lexigraph-core/tests/corpus/${input:language}/`
4. Property test updated to include `${input:language}` in its language matrix
5. Example queries at `packages/lexigraph-core/grammars/${input:language}/examples.md`
6. Docs updated (`AGENTS.md` language list, root README when it exists)
7. Conventional Commit: `feat(core): add ${input:language} language support`

Run `pnpm --filter @lexigraph/core test` and confirm green before opening the PR.
