---
name: mcp-tool-authoring
description: 'Author a new MCP tool in @lexigraph/mcp. Use when adding, modifying, or reviewing any tool exposed over the Model Context Protocol — including query tools, mutation tools, file tools, or workspace tools. Enforces zod schemas, access checks, optimistic locking, standard ok/error/conflict envelope, and integration test requirements.'
---

# MCP Tool Authoring

Single source of truth for adding tools to `@lexigraph/mcp`. Every tool follows the
same five-step shape so callers (AI agents) get a predictable surface.

## When to Use

- Adding a new MCP tool
- Refactoring an existing tool
- Reviewing a PR that touches `packages/lexigraph-mcp/src/tools/`

## Required shape (every tool)

```ts
// packages/lexigraph-mcp/src/tools/<tool_name>.ts
import { z } from 'zod';
import type { ToolContext, ToolResult } from '../types.js';

// 1. Zod input schema — exported for tests + JSON-Schema generation
export const toolNameInput = z.object({
  path: z.string().describe('Workspace-relative path to the file'),
  expectedVersion: z.number().int().nonnegative().optional(),
});
export type ToolNameInput = z.infer<typeof toolNameInput>;

// 2. Tool definition (description is the AI's discovery surface — be precise)
export const toolName = {
  name: 'tool_name',
  description: 'One sentence: what it does. One sentence: when to use it.',
  inputSchema: toolNameInput,
  // 3. Action label for the access ruleset (track | read | edit | insert | delete | save | query | …)
  action: 'read' as const,
  // 4. Handler
  async handle(input: ToolNameInput, ctx: ToolContext): Promise<ToolResult> {
    const path = ctx.workspace.normalizePath(input.path); // confinement + Windows /c/
    ctx.access.require('read', path); // ruleset enforcement
    const file = ctx.workspace.get(path);
    if (!file) return { status: 'error', message: `not tracked: ${input.path}` };
    if (input.expectedVersion !== undefined && file.version !== input.expectedVersion) {
      return { status: 'conflict', currentVersion: file.version };
    }
    // ... real work ...
    return {
      status: 'ok',
      version: file.version,
      data: {
        /* … */
      },
    };
  },
};
```

## The five steps

1. **Zod schema** — strictly typed input. Every field has `.describe(...)` so AI sees it.
2. **Handler** — pure function over `(input, ctx)`. No global state. No console output
   (stdio is the MCP transport — anything on stdout breaks the wire protocol; use
   `console.error` for diagnostics only).
3. **Access check** — call `ctx.access.require(action, path)` before any side effect.
4. **Versioned response** — every response is `{ status: 'ok' | 'error' | 'conflict', … }`.
   Include `version` on every successful read or write so optimistic locking works.
5. **Integration test** — `packages/lexigraph-mcp/tests/tools/<name>.test.ts` spawns a
   real MCP server over an in-memory transport and exercises the tool end-to-end.

## Response envelope (mandatory)

```ts
type ToolResult =
  | { status: 'ok'; version?: number; data?: unknown }
  | { status: 'error'; message: string; details?: unknown }
  | { status: 'conflict'; currentVersion: number };
```

Errors must include a human-readable `message`. Conflict responses must include
`currentVersion` so the caller can re-read and retry.

## Registration

`packages/lexigraph-mcp/src/server.ts` imports the tool from `tools/index.ts`. Add
the export there — never register a tool in `server.ts` directly.

## Verification Checklist

- [ ] Tool file lives at `packages/lexigraph-mcp/src/tools/<name>.ts`
- [ ] Input schema is zod, every field has `.describe(...)`
- [ ] Handler calls `ctx.workspace.normalizePath` for any path input
- [ ] Handler calls `ctx.access.require(action, path)` before side effects
- [ ] Mutation tools accept `expectedVersion` and return `{ status: 'conflict' }` on mismatch
- [ ] Response uses the standard envelope
- [ ] Integration test exercises ok / error / conflict paths
- [ ] No `console.log` (would corrupt stdio MCP transport)
- [ ] Exported from `tools/index.ts`

## Common Pitfalls

- **Writing to stdout.** MCP stdio uses stdout for JSON-RPC framing. `console.log` will
  break the connection. Always `console.error`.
- **Skipping the access check.** Containment is enforced by `normalizePath`, but ruleset
  checks are per-action and per-resource. Both are required.
- **Forgetting `expectedVersion`.** Without it, watcher reloads can silently clobber
  AI edits.
- **Adding a tool that duplicates a JSON Patch op.** If the request is structurally a
  `replace`/`add`/`remove`/`move`, route through `apply_patch` instead.
