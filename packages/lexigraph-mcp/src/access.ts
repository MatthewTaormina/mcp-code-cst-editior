// Permissive access control stub. Phase 6 ports the legacy ruleset.

import type { AccessAction } from './types.js';

export interface AccessControl {
  /** Throws if the action on the given workspace-relative path is denied. */
  require(action: AccessAction, path: string): void;
}

/** Default implementation: allows everything. */
export class PermissiveAccessControl implements AccessControl {
  require(_action: AccessAction, _path: string): void {
    // Phase 6: enforce ruleset.
  }
}
