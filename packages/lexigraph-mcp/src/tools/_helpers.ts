// Helpers shared across tool handlers.

import type { ToolResult } from '../types.js';

/** Convert any thrown value to a friendly error envelope. */
export function toErrorResult(err: unknown, prefix?: string): ToolResult {
  const message = err instanceof Error ? err.message : String(err);
  return { status: 'error', message: prefix ? `${prefix}: ${message}` : message };
}
