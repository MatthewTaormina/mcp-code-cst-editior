# Lexigraph

Structural code editing for AI agents — via tree-sitter Concrete Syntax Trees.

> **Status: rewrite in progress on `rewrite/v2`.** The v1 Rust implementation is archived
> under [`legacy/`](./legacy/) for reference only.

Lexigraph turns any source file into a typed, position-aware **grammar tree** that an AI
queries with structured paths and mutates with JSON-Patch-style operations. Edits are
re-validated against the grammar — anything that breaks syntax is rejected before it
hits disk. Comments and whitespace round-trip losslessly.

## Packages

| Package                                        | Description                                     |
| ---------------------------------------------- | ----------------------------------------------- |
| [`@lexigraph/core`](./packages/lexigraph-core) | Pure library: parsers, tree model, query, patch |
| [`@lexigraph/mcp`](./packages/lexigraph-mcp)   | MCP server exposing the library as tools        |

## Languages (v1)

JavaScript · TypeScript · JSON · CSS · HTML · Rust · Python · Markdown · YAML · TOML

## Quick start

```bash
pnpm install
pnpm typecheck
pnpm test
pnpm build
```

## Documentation

- [AGENTS.md](./AGENTS.md) — project guide, standards, phased plan
- [`.github/skills/`](./.github/skills/) — workflows for adding languages and MCP tools
- [`.github/prompts/`](./.github/prompts/) — slash-command prompts
- [`.github/chatmodes/`](./.github/chatmodes/) — restricted agent mode for implementation

## License

See [LICENSE](./LICENSE).
