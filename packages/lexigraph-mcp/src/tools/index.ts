// Tool registry. Add new tools here.

import type { ToolDefinition } from '../types.js';
import { applyPatchTool } from './apply_patch.js';
import { diagnosticsTool } from './diagnostics.js';
import { getNodeTool } from './get_node.js';
import { getTreeTool } from './get_tree.js';
import { queryFileTool } from './query_file.js';
import { saveFileTool } from './save_file.js';
import { trackFileTool } from './track_file.js';
import { untrackFileTool } from './untrack_file.js';
import { workspaceInfoTool } from './workspace_info.js';

export const TOOLS: readonly ToolDefinition[] = [
  workspaceInfoTool,
  trackFileTool,
  untrackFileTool,
  getTreeTool,
  getNodeTool,
  queryFileTool,
  diagnosticsTool,
  applyPatchTool,
  saveFileTool,
];

export {
  applyPatchTool,
  diagnosticsTool,
  getNodeTool,
  getTreeTool,
  queryFileTool,
  saveFileTool,
  trackFileTool,
  untrackFileTool,
  workspaceInfoTool,
};
