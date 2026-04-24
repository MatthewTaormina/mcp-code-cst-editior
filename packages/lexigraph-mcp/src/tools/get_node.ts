// get_node — resolve a single node by JSON Pointer.

import { z } from 'zod';

import type { ToolDefinition, ToolResult } from '../types.js';
import { toErrorResult } from './_helpers.js';

export const getNodeTool: ToolDefinition = {
  name: 'get_node',
  description:
    'Look up a single node by JSON Pointer (RFC 6901). Returns the node type, byte range, full text, and the pointers of its children.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path of a tracked file.'),
    pointer: z
      .string()
      .describe('JSON Pointer into the tree, e.g. "" for root or "/children/0/children/2".'),
  },
  action: 'read',
  async handle(args, ctx): Promise<ToolResult> {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('read', path);
      const file = ctx.workspace.get(path);
      if (!file) return { status: 'error', message: `not tracked: ${path}` };
      const node = file.tree.resolve(args.pointer);
      if (!node) return { status: 'error', message: `pointer not found: ${args.pointer}` };
      return {
        status: 'ok',
        version: file.tree.version,
        data: {
          path: file.path,
          pointer: node.pointer,
          type: node.type,
          startByte: node.range.startByte,
          endByte: node.range.endByte,
          startPosition: node.range.startPosition,
          endPosition: node.range.endPosition,
          isError: node.isError,
          isMissing: node.isMissing,
          text: node.text,
          children: node.children.map((c) => ({
            pointer: c.pointer,
            type: c.type,
            startByte: c.range.startByte,
            endByte: c.range.endByte,
          })),
        },
      };
    } catch (err) {
      return toErrorResult(err, 'get_node');
    }
  },
};
