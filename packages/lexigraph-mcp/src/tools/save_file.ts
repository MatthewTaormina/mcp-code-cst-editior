// save_file — write a tracked file's current tree text to disk.

import { z } from 'zod';

import type { ToolDefinition, ToolResult } from '../types.js';
import { toErrorResult } from './_helpers.js';

export const saveFileTool: ToolDefinition = {
  name: 'save_file',
  description:
    'Persist a tracked file to disk by writing its current serialized tree. Optimistic locking: if expectedVersion is provided and the in-memory version differs, returns status=conflict.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path of a tracked file.'),
    expectedVersion: z
      .number()
      .int()
      .nonnegative()
      .optional()
      .describe('If set, only save when the current version matches.'),
  },
  action: 'save',
  async handle(args, ctx): Promise<ToolResult> {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('save', path);
      const file = ctx.workspace.get(path);
      if (!file) return { status: 'error', message: `not tracked: ${path}` };
      if (args.expectedVersion !== undefined && args.expectedVersion !== file.tree.version) {
        return { status: 'conflict', currentVersion: file.tree.version };
      }
      await ctx.workspace.save(file);
      return {
        status: 'ok',
        version: file.tree.version,
        data: { path: file.path, version: file.tree.version, bytes: file.diskSource.length },
      };
    } catch (err) {
      return toErrorResult(err, 'save_file');
    }
  },
};
