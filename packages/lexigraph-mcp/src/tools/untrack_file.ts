// untrack_file — remove a file from the workspace registry.

import { z } from 'zod';

import type { ToolDefinition } from '../types.js';
import { toErrorResult } from './_helpers.js';

export const untrackFileTool: ToolDefinition = {
  name: 'untrack_file',
  description: 'Remove a previously-tracked file from the in-memory registry. Disk is not touched.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path passed to track_file.'),
  },
  action: 'track',
  async handle(args, ctx) {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('track', path);
      const removed = ctx.workspace.untrack(path);
      return { status: 'ok', data: { path, removed } };
    } catch (err) {
      return toErrorResult(err, 'untrack_file');
    }
  },
};
