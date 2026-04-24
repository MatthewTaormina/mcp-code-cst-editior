// Public programmatic API for embedders.

export { createServer } from './server.js';
export type { CreateServerOptions, LexigraphServer } from './server.js';

export { Workspace, WorkspaceError } from './workspace.js';
export type { TrackedFile, TrackOptions } from './workspace.js';

export { PermissiveAccessControl } from './access.js';
export type { AccessControl } from './access.js';

export { TOOLS } from './tools/index.js';
export type { AccessAction, ToolContext, ToolDefinition, ToolResult } from './types.js';

export const VERSION = '0.0.0';
