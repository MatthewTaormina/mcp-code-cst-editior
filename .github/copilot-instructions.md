# Copilot Instructions

The full project guide lives in [AGENTS.md](../AGENTS.md). Read it first.

## Hard rules (re-stated for fast load)

- TypeScript strict, **no `any`**, **no `@ts-ignore`**, ESM only, Node ≥ 20.
- **Lossless round-trip** is the project's green bar. Any parser change must keep
  `parse(text) → serialize === text` for every supported language.
- **Reject grammar-breaking edits.** Patches that produce tree-sitter `ERROR` / `MISSING`
  nodes must be rolled back with a diagnostic.
- Address tree nodes with **JSON Pointer (RFC 6901)**. Mutate with **JSON Patch (RFC 6902)**
  semantics. Don't invent new mutation verbs.
- Workspace confinement is the security boundary; reject `..` path escapes.
- **Don't edit `legacy/`** — it's archived v1 reference.

## Workflows

- New language → load skill [tree-sitter-grammar-integration](skills/tree-sitter-grammar-integration/SKILL.md).
- New MCP tool → load skill [mcp-tool-authoring](skills/mcp-tool-authoring/SKILL.md).
- For phase-bounded implementation work, prefer the [`cst-implementer`](chatmodes/cst-implementer.chatmode.md) agent mode.

## Conventions

- Conventional Commits.
- One package change per commit when feasible.
- Add or update tests with every behavioral change. Round-trip / patch fuzz tests live
  in `packages/lexigraph-core/tests/`.
