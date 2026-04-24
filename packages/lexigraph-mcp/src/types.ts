// Shared types for the lexigraph MCP server.

import type { z, ZodRawShape } from 'zod';

import type { Workspace } from './workspace.js';
import type { AccessControl } from './access.js';

/** Standard envelope every tool returns. */
export type ToolResult =
  | { status: 'ok'; version?: number; data?: unknown }
  | { status: 'error'; message: string; details?: unknown }
  | { status: 'conflict'; currentVersion: number };

/** Context handed to each tool handler. */
export interface ToolContext {
  readonly workspace: Workspace;
  readonly access: AccessControl;
}

/** Stable action labels used by the access control ruleset (Phase 6 fills in real rules). */
export type AccessAction = 'track' | 'read' | 'edit' | 'save' | 'query';

/** A registered MCP tool. */
export interface ToolDefinition<Shape extends ZodRawShape = ZodRawShape> {
  /** Tool name as exposed over MCP. snake_case. */
  readonly name: string;
  /** Short, AI-facing description of what the tool does and when to use it. */
  readonly description: string;
  /** Zod raw shape for the tool's input. Each field should call .describe(). */
  readonly inputSchema: Shape;
  /** Access ruleset action label. */
  readonly action: AccessAction;
  /** Pure handler over (validated args, context). */
  handle(args: z.objectOutputType<Shape, z.ZodTypeAny>, ctx: ToolContext): Promise<ToolResult>;
}
