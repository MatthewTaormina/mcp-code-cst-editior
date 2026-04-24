// apply_patch — run a JSON-Patch-style batch against a tracked file's tree.

import { z } from 'zod';

import { applyPatch, type PatchOp } from '@lexigraph/core';

import type { ToolDefinition, ToolResult } from '../types.js';
import { toErrorResult } from './_helpers.js';

const opSchema = z
  .discriminatedUnion('op', [
    z.object({
      op: z.literal('replace'),
      path: z.string().describe('Pointer to the node whose verbatim text is replaced.'),
      value: z.string().describe('Replacement source text.'),
    }),
    z.object({
      op: z.literal('remove'),
      path: z.string().describe('Pointer to the node to remove. Cannot be the root.'),
    }),
    z.object({
      op: z.literal('add'),
      path: z
        .string()
        .describe(
          'Insertion point: must end in "/children/N". N==length appends; otherwise inserts before child N.',
        ),
      value: z.string().describe('Source text to insert verbatim.'),
    }),
    z.object({
      op: z.literal('test'),
      path: z.string().describe('Pointer to the node whose text must equal `value`.'),
      value: z.string().describe('Expected verbatim text.'),
    }),
    z.object({
      op: z.literal('move'),
      from: z.string().describe('Pointer to the node to remove.'),
      path: z
        .string()
        .describe(
          'Insertion point under "/children/N". The caller is responsible for separators; the ERROR-node guard catches invalid results.',
        ),
    }),
  ])
  .describe(
    'A single JSON-Patch op. Supported: replace | remove | add | test | move (no copy in v1).',
  );

export const applyPatchTool: ToolDefinition = {
  name: 'apply_patch',
  description:
    'Apply a batch of JSON-Patch ops to a tracked file. The batch is atomic: any failure (op error, optimistic-locking conflict, or grammar-breaking edit) leaves the in-memory tree unchanged. On success the file version is incremented by 1.',
  inputSchema: {
    path: z.string().describe('Workspace-relative path of a tracked file.'),
    expectedVersion: z
      .number()
      .int()
      .nonnegative()
      .optional()
      .describe('Optimistic lock. If the current version differs, returns status=conflict.'),
    ops: z.array(opSchema).min(1).describe('Sequence of patch ops, applied in order.'),
  },
  action: 'edit',
  async handle(args, ctx): Promise<ToolResult> {
    try {
      const path = ctx.workspace.normalizePath(args.path);
      ctx.access.require('edit', path);
      const file = ctx.workspace.get(path);
      if (!file) return { status: 'error', message: `not tracked: ${path}` };
      const opts: { expectedVersion?: number } = {};
      if (args.expectedVersion !== undefined) opts.expectedVersion = args.expectedVersion;
      const result = await applyPatch(file.tree, args.ops as PatchOp[], opts);
      if (result.status === 'ok') {
        ctx.workspace.setTree(file, result.tree);
        return {
          status: 'ok',
          version: result.tree.version,
          data: { path: file.path, version: result.tree.version },
        };
      }
      if (result.status === 'conflict') {
        return { status: 'conflict', currentVersion: result.actualVersion };
      }
      return {
        status: 'error',
        message: result.reason,
        details: { opIndex: result.opIndex, diagnostics: result.diagnostics },
      };
    } catch (err) {
      return toErrorResult(err, 'apply_patch');
    }
  },
};
