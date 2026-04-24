// diagnostics — list ERROR / MISSING nodes in a tracked file's tree.

import { z } from 'zod';

import type { ToolDefinition, ToolResult } from '../types.js';
import { toErrorResult } from './_helpers.js';

export const diagnosticsTool: ToolDefinition = {
  name: 'diagnostics',
  description:
    'Return all tree-sitter ERROR and MISSING nodes for a tracked file, with their JSON Pointers and byte ranges. Use to inspect why a parse is broken before patching.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path of a tracked file.'),
  },
  action: 'read',
  async handle(args, ctx): Promise<ToolResult> {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('read', path);
      const file = ctx.workspace.get(path);
      if (!file) return { status: 'error', message: `not tracked: ${path}` };
      const diagnostics = file.tree.diagnostics();
      return {
        status: 'ok',
        version: file.tree.version,
        data: {
          path: file.path,
          hasError: file.tree.hasError(),
          diagnostics,
        },
      };
    } catch (err) {
      return toErrorResult(err, 'diagnostics');
    }
  },
};
