// Lexigraph MCP server factory.
//
// `createServer({ root })` returns an `McpServer` with every tool from `tools/index.ts`
// registered. Tool handlers wrap their `ToolResult` envelope into the MCP
// `CallToolResult` shape (text content carrying JSON, with `isError` set on failure).

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';

import { PermissiveAccessControl, type AccessControl } from './access.js';
import { TOOLS } from './tools/index.js';
import type { ToolContext, ToolDefinition, ToolResult } from './types.js';
import { Workspace } from './workspace.js';

export interface CreateServerOptions {
  /** Absolute or relative path to the workspace root. */
  readonly root: string;
  /** Optional access-control implementation; defaults to permissive. */
  readonly access?: AccessControl;
}

export interface LexigraphServer {
  readonly server: McpServer;
  readonly workspace: Workspace;
  readonly access: AccessControl;
}

export function createServer(options: CreateServerOptions): LexigraphServer {
  const workspace = new Workspace(options.root);
  const access = options.access ?? new PermissiveAccessControl();
  const ctx: ToolContext = { workspace, access };

  const server = new McpServer(
    { name: 'lexigraph', version: '0.0.0' },
    { capabilities: { tools: {} } },
  );

  for (const tool of TOOLS) {
    registerTool(server, tool, ctx);
  }

  return { server, workspace, access };
}

function registerTool(server: McpServer, tool: ToolDefinition, ctx: ToolContext): void {
  server.registerTool(
    tool.name,
    {
      description: tool.description,
      inputSchema: tool.inputSchema,
    },
    async (args: unknown) => {
      const result = await tool.handle(args as Parameters<typeof tool.handle>[0], ctx);
      return toCallToolResult(result);
    },
  );
}

function toCallToolResult(result: ToolResult): {
  content: Array<{ type: 'text'; text: string }>;
  isError?: boolean;
} {
  const text = JSON.stringify(result, null, 2);
  if (result.status === 'ok') {
    return { content: [{ type: 'text', text }] };
  }
  return { content: [{ type: 'text', text }], isError: true };
}
