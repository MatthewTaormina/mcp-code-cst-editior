// get_tree — return the parsed tree for a tracked file.
// By default emits a structural summary (node types + pointers, no source text) capped
// by depth to keep the response small. Pass includeText=true to include each node's text.

import { z } from 'zod';

import type { LxNode } from '@lexigraph/core';

import type { ToolDefinition, ToolResult } from '../types.js';
import { toErrorResult } from './_helpers.js';

interface SummaryNode {
  type: string;
  pointer: string;
  startByte: number;
  endByte: number;
  isError?: true;
  isMissing?: true;
  text?: string;
  children?: SummaryNode[];
  truncated?: number;
}

function summarize(node: LxNode, depth: number, includeText: boolean): SummaryNode {
  const out: SummaryNode = {
    type: node.type,
    pointer: node.pointer,
    startByte: node.range.startByte,
    endByte: node.range.endByte,
  };
  if (node.isError) out.isError = true;
  if (node.isMissing) out.isMissing = true;
  if (includeText) out.text = node.text;
  if (depth <= 0) {
    if (node.children.length > 0) out.truncated = node.children.length;
    return out;
  }
  if (node.children.length > 0) {
    out.children = node.children.map((c) => summarize(c, depth - 1, includeText));
  }
  return out;
}

export const getTreeTool: ToolDefinition = {
  name: 'get_tree',
  description:
    'Return a structural summary of the parsed tree for a tracked file. Each node is emitted with its JSON Pointer, type, and byte range. Use maxDepth to control verbosity (defaults to 4); pass includeText=true to inline node text.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path of a tracked file.'),
    maxDepth: z
      .number()
      .int()
      .min(0)
      .max(64)
      .optional()
      .describe('Maximum recursion depth from the root. Default 4.'),
    includeText: z
      .boolean()
      .optional()
      .describe('When true, include `text` on every emitted node. Default false.'),
  },
  action: 'read',
  async handle(args, ctx): Promise<ToolResult> {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('read', path);
      const file = ctx.workspace.get(path);
      if (!file) return { status: 'error', message: `not tracked: ${path}` };
      const depth = args.maxDepth ?? 4;
      const includeText = args.includeText ?? false;
      const root = summarize(file.tree.root, depth, includeText);
      return {
        status: 'ok',
        version: file.tree.version,
        data: { path: file.path, languageId: file.tree.languageId, root },
      };
    } catch (err) {
      return toErrorResult(err, 'get_tree');
    }
  },
};
