// track_file — read a file from disk, parse it, and add to the workspace registry.

import { z } from 'zod';

import { LANGUAGES } from '@lexigraph/core';

import type { ToolDefinition } from '../types.js';
import { toErrorResult } from './_helpers.js';

export const trackFileTool: ToolDefinition = {
  name: 'track_file',
  description:
    'Load a workspace file from disk, parse it with the matching tree-sitter grammar, and register it for subsequent reads, queries, and patches. Returns the assigned version (always 0 on first track) and any parse diagnostics.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path to the file (no `..` escapes).'),
    languageId: z
      .string()
      .optional()
      .describe(
        `Override the auto-detected language. Must be one of: ${Object.keys(LANGUAGES).join(', ')}.`,
      ),
  },
  action: 'track',
  async handle(args, ctx) {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('track', path);
      if (args.languageId !== undefined && !(args.languageId in LANGUAGES)) {
        return {
          status: 'error',
          message: `unknown languageId: ${args.languageId}`,
        };
      }
      const file = await ctx.workspace.track(path, {
        ...(args.languageId !== undefined ? { languageId: args.languageId } : {}),
      });
      return {
        status: 'ok',
        version: file.tree.version,
        data: {
          path: file.path,
          languageId: file.tree.languageId,
          version: file.tree.version,
          diagnostics: file.tree.diagnostics(),
        },
      };
    } catch (err) {
      return toErrorResult(err, 'track_file');
    }
  },
};
