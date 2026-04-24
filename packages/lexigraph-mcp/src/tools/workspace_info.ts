// workspace_info — workspace root, supported languages, currently tracked files.

import { LANGUAGES } from '@lexigraph/core';

import type { ToolDefinition } from '../types.js';

export const workspaceInfoTool: ToolDefinition = {
  name: 'workspace_info',
  description:
    'Return the workspace root, the list of supported languages (id + extensions), and currently tracked files. Call before other tools to discover what is available.',
  inputSchema: {},
  action: 'read',
  async handle(_args, ctx) {
    const languages = Object.values(LANGUAGES).map((l) => ({
      id: l.id,
      extensions: l.extensions,
    }));
    const tracked = ctx.workspace.list().map((f) => ({
      path: f.path,
      languageId: f.tree.languageId,
      version: f.tree.version,
      dirty: ctx.workspace.isDirty(f),
    }));
    return {
      status: 'ok',
      data: { root: ctx.workspace.root, languages, tracked },
    };
  },
};
