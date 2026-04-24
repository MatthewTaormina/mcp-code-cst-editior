// query_file — run a tree-sitter S-expression query against a tracked file.

import { z } from 'zod';

import { executeQuery, QueryCompileError } from '@lexigraph/core';

import type { ToolDefinition, ToolResult } from '../types.js';
import { toErrorResult } from './_helpers.js';

export const queryFileTool: ToolDefinition = {
  name: 'query_file',
  description:
    'Run a tree-sitter S-expression query against a tracked file. Returns matches with captures (each capture is a node addressed by JSON Pointer). Supports byte-range scoping and offset/limit pagination.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path of a tracked file.'),
    query: z.string().describe('S-expression query source. See tree-sitter docs.'),
    pointer: z.string().optional().describe('Anchor the query to a sub-tree. Default: tree root.'),
    startByte: z.number().int().nonnegative().optional().describe('Inclusive byte range start.'),
    endByte: z.number().int().nonnegative().optional().describe('Exclusive byte range end.'),
    offset: z.number().int().nonnegative().optional().describe('Skip the first N matches.'),
    limit: z.number().int().positive().optional().describe('Return at most N matches.'),
  },
  action: 'query',
  async handle(args, ctx): Promise<ToolResult> {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('query', path);
      const file = ctx.workspace.get(path);
      if (!file) return { status: 'error', message: `not tracked: ${path}` };
      const opts: {
        pointer?: string;
        startByte?: number;
        endByte?: number;
        offset?: number;
        limit?: number;
      } = {};
      if (args.pointer !== undefined) opts.pointer = args.pointer;
      if (args.startByte !== undefined) opts.startByte = args.startByte;
      if (args.endByte !== undefined) opts.endByte = args.endByte;
      if (args.offset !== undefined) opts.offset = args.offset;
      if (args.limit !== undefined) opts.limit = args.limit;
      const result = await executeQuery(file.tree, args.query, opts);
      return {
        status: 'ok',
        version: file.tree.version,
        data: {
          path: file.path,
          total: result.total,
          truncated: result.truncated,
          matches: result.matches.map((m) => ({
            patternIndex: m.patternIndex,
            captures: m.captures.map((c) => ({
              name: c.name,
              pointer: c.node.pointer,
              type: c.node.type,
              startByte: c.node.range.startByte,
              endByte: c.node.range.endByte,
              text: c.node.text,
            })),
          })),
        },
      };
    } catch (err) {
      if (err instanceof QueryCompileError) {
        return { status: 'error', message: `query compile error: ${err.message}` };
      }
      return toErrorResult(err, 'query_file');
    }
  },
};
