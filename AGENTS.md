# AGENTS.md — Lexigraph

> Authoritative project guide for AI agents and humans. Keep this file short and link out
> to deeper docs. If anything below conflicts with code, the code wins — fix the doc.

Lexigraph is an MCP server + library that lets AI agents edit source files through their
**Concrete Syntax Tree** (CST) instead of raw text. Files are parsed with **tree-sitter**
into typed, position-aware grammar trees that round-trip losslessly (comments + whitespace
preserved). Agents query trees with JSON Pointer or tree-sitter S-expressions and mutate
them with JSON-Patch-style operations that are rejected if they break the grammar.

## Repository layout

```
packages/
  lexigraph-core/   Pure library: parsers, tree model, query, patch
  lexigraph-mcp/    MCP server exposing lexigraph-core as tools
.github/
  workflows/        CI (matrix: Linux/macOS/Windows × Node 20/22)
  skills/           On-demand workflows (tree-sitter-grammar-integration, mcp-tool-authoring)
  prompts/          Slash-command prompts (add-language)
  chatmodes/        Restricted agent modes (cst-implementer)
  copilot-instructions.md   Pointer to this file
legacy/             Archived v1 Rust implementation (reference only — do not edit)
```

## Tech stack

- TypeScript 5.7, strict mode, **ESM only**, Node ≥ 20
- pnpm workspaces (v10)
- `web-tree-sitter` (WASM grammars — portable, no native compile)
- `@modelcontextprotocol/sdk` for the MCP server
- `chokidar` watcher, `picomatch` glob, `zod` schemas
- `vitest` + `fast-check` for testing
- `tsup` for builds, `eslint` v9 flat config, `prettier`

## Standards

- **JSON Pointer** (RFC 6901) for node addressing
- **JSON Patch** (RFC 6902) semantics for mutations (`add`, `remove`, `replace`, `move`, `copy`, `test`)
- **Tree-sitter S-expression query language** for pattern matching
- **MCP** latest stable spec
- **Conventional Commits** for git history
- **Semver** for the `@lexigraph/*` packages

## Build, test, lint

All commands run from the repo root and fan out across packages via pnpm.

```bash
pnpm install            # install all deps (use --frozen-lockfile in CI)
pnpm typecheck          # tsc --noEmit in every package
pnpm test               # vitest run in every package
pnpm lint               # eslint
pnpm format:check       # prettier --check
pnpm format             # prettier --write
pnpm build              # tsup in every package
```

CI must stay green on Linux/macOS/Windows × Node 20 + 22. If a change can't pass on all
matrices, stop and fix the matrix before merging.

## Implementation rules (non-negotiable)

1. **No `any` and no `// @ts-ignore`.** TypeScript strict mode is the contract.
2. **Every parser interaction must be covered by a round-trip test** (parse → serialize == input).
   Round-trip tests are the project's primary green bar — if they fail, all other work stops.
3. **JSON-Patch semantics over ad-hoc tools.** Don't add a new mutation tool when an existing
   patch op covers it.
4. **Reject grammar-breaking edits.** Any patch that introduces a tree-sitter `ERROR` or
   `MISSING` node inside the edited range must be rolled back atomically with a diagnostic.
5. **Workspace confinement is the primary security boundary.** Every path goes through the
   normalizer; `..` escapes are rejected.
6. **Add language support via the [tree-sitter-grammar-integration](.github/skills/tree-sitter-grammar-integration/SKILL.md) skill.**
   Don't freelance it.
7. **Add MCP tools via the [mcp-tool-authoring](.github/skills/mcp-tool-authoring/SKILL.md) skill.**
   Every new tool: zod schema → handler → access check → versioned response → integration test.
8. **Don't touch `legacy/`.** It's preserved for reference only.

## Phased plan

The current rewrite is on branch `rewrite/v2`. The full PRD, phase definitions, and
checklist live in session memory and are summarized below:

- **Phase 0** — Foundation (this branch): scaffolding, agent assets, CI
- **Phase 1** — Core tree primitive: parsers, uniform Node model, lossless round-trip
- **Phase 2** — Read API: JSON Pointer + S-expression queries, diagnostics
- **Phase 3** — Patch engine: JSON-Patch ops, ERROR-node guard, optimistic locking
- **Phase 4** — MCP server: tool surface (`workspace_info`, `track_file`, `get_tree`, `get_node`, `query_file`, `apply_patch`, `save_file`, `diagnostics`)
- **Phase 5** — Workspace + watcher: `chokidar`, glob tracking, cross-file queries
- **Phase 6** — Access control: ruleset port from legacy, audit log
- **Phase 7** _(v1.1)_ — Extended queries: CSS-selector, XPath subset
- **Phase 8** — Polish + release to npm

## Languages (v1)

JavaScript, TypeScript, JSON, CSS, HTML, Rust, Python, Markdown, YAML, TOML — all via
WASM grammars bundled inside `@lexigraph/core`.

## Branch + commit policy

- Feature work branches off `rewrite/v2` until v2 is merged to `main`
- Conventional Commits required (`feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`)
- Never `git push --force` to shared branches
- Never bypass hooks (`--no-verify`)
