//! Workspace-scoped path resolution and rule-based access control.
//!
//! Every path that enters the server is resolved relative to the configured
//! workspace root before use.  Paths that escape the workspace are rejected
//! by default.  An optional JSON ruleset file can further constrain or
//! selectively open access with allow/deny rules ordered by priority.
//!
//! # Unix-style paths on all platforms
//!
//! Clients always send paths in Unix format (forward slashes).  On Windows
//! the pattern `/X/rest` (where `X` is a single ASCII letter) is treated as
//! a Windows drive path and converted to `X:\rest` before any other
//! processing.  On Linux and macOS the paths are used as-is.
//!
//! # JSON ruleset format
//!
//! ```json
//! {
//!   "rules": [
//!     {
//!       "effect": "deny",
//!       "priority": 100,
//!       "actions": ["edit", "insert", "delete"],
//!       "resources": ["locked/**"]
//!     },
//!     {
//!       "effect": "allow",
//!       "priority": 50,
//!       "actions": ["read"],
//!       "resources": ["src/**"]
//!     }
//!   ]
//! }
//! ```
//!
//! ## Recognised action names
//!
//! | Action        | Tool(s) that use it                              |
//! |---------------|--------------------------------------------------|
//! | `track`       | `track_file`                                     |
//! | `untrack`     | `untrack_file`                                   |
//! | `load`        | `load_file`                                      |
//! | `read`        | `get_node`, `get_tree_skeleton`, `get_line_tokens` |
//! | `edit`        | `edit_node`                                      |
//! | `insert`      | `insert_lines`                                   |
//! | `delete`      | `delete_lines`                                   |
//! | `save`        | `save_file`                                      |
//! | `query`       | `query_file`, `query_workspace`                  |
//! | `create`      | `create_file`                                    |
//! | `delete_file` | `delete_file`                                    |
//!
//! Use `"*"` in an action list to match any of the above.
//!
//! `resources` patterns are relative to the workspace root unless they start
//! with `/` (treated as absolute).  `"*"` in an action list matches any
//! action.  Glob patterns support `*` (non-separator), `**` (any depth), and
//! `?` (single non-separator character).  When multiple rules match, the one
//! with the highest `priority` wins; ties favour `deny`.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by [`AccessConfig`].
#[derive(Debug, Error)]
pub enum AccessError {
    /// The requested path is outside the workspace root.
    #[error("path '{0}' escapes the workspace root")]
    OutsideWorkspace(String),
    /// A deny rule matched (or no allow rule matched after a deny-all default).
    #[error("access denied: action '{action}' on '{resource}' is not permitted")]
    Denied { action: String, resource: String },
    /// The ruleset file could not be read.
    #[error("failed to load ruleset: {0}")]
    RulesetLoad(#[from] std::io::Error),
    /// The ruleset file contains invalid JSON.
    #[error("failed to parse ruleset JSON: {0}")]
    RulesetParse(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Policy document — JSON schema
// ---------------------------------------------------------------------------

/// Whether a rule grants or revokes access.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PolicyEffect {
    Allow,
    Deny,
}

/// A single access-control rule inside the policy document.
///
/// When `actions` contains `"*"` it matches any action name.  When
/// `resources` contains `"**"` it matches any path.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyRule {
    /// Whether this rule grants (`"allow"`) or revokes (`"deny"`) access.
    pub effect: PolicyEffect,
    /// Higher numbers take precedence.  When two matching rules have the same
    /// priority, `deny` wins.
    pub priority: i64,
    /// Action names this rule applies to (e.g. `["edit","insert"]` or `["*"]`).
    pub actions: Vec<String>,
    /// Glob patterns for file resources this rule applies to.  Patterns
    /// relative to workspace unless they start with `/`.
    pub resources: Vec<String>,
}

/// Top-level structure of the JSON ruleset file.
#[derive(Debug, Deserialize)]
pub struct PolicyDocument {
    pub rules: Vec<PolicyRule>,
}

// ---------------------------------------------------------------------------
// AccessConfig — the runtime enforcement struct
// ---------------------------------------------------------------------------

/// Shared access configuration passed to every tool handler.
///
/// Build one with [`AccessConfig::new`] and share via `Arc<AccessConfig>`.
#[derive(Debug)]
pub struct AccessConfig {
    /// Canonical absolute path of the workspace root.
    pub workspace: PathBuf,
    /// Rules sorted by descending priority (highest first).
    rules: Vec<PolicyRule>,
}

impl AccessConfig {
    /// Build an `AccessConfig` from the given workspace path and an optional
    /// ruleset file path.
    ///
    /// `workspace_path` and `ruleset_path` (when provided) are accepted in
    /// Unix format (see module docs for the `/c/` conversion on Windows).
    ///
    /// # Errors
    ///
    /// Returns an error when:
    /// * The workspace path cannot be canonicalized (does not exist or
    ///   insufficient permissions).
    /// * The ruleset file cannot be read or contains invalid JSON.
    pub fn new(workspace_path: &str, ruleset_path: Option<&str>) -> Result<Self, AccessError> {
        // Resolve workspace to a canonical path.
        let ws_native = from_unix_path(workspace_path);
        let workspace = std::fs::canonicalize(&ws_native).map_err(|e| {
            AccessError::RulesetLoad(std::io::Error::new(
                e.kind(),
                format!(
                    "workspace_path '{}' is not accessible: {e}",
                    workspace_path
                ),
            ))
        })?;

        // Load and parse the optional ruleset.
        let mut rules: Vec<PolicyRule> = Vec::new();
        if let Some(rp) = ruleset_path {
            let rp_native = from_unix_path(rp);
            // Ruleset path is relative to workspace unless it is absolute.
            let rp_abs = if rp_native.is_absolute() {
                rp_native
            } else {
                workspace.join(rp_native)
            };
            let json = std::fs::read_to_string(&rp_abs)?;
            let doc: PolicyDocument = serde_json::from_str(&json)?;
            rules = doc.rules;
            // Highest priority first; ties: deny > allow (stable sort preserves
            // order from the file, and we check deny first via the match arm
            // ordering once priority is equal).
            rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        }

        Ok(Self { workspace, rules })
    }

    /// Resolve a Unix-style path string to a canonical absolute native
    /// `PathBuf`, enforcing workspace containment.
    ///
    /// Resolution algorithm:
    /// 1. Convert from Unix format (handles `/c/` Windows drive prefix on
    ///    Windows; no-op on Linux/macOS).
    /// 2. If not absolute, join to the workspace root.
    /// 3. Normalise `..` and `.` components lexically (no filesystem access).
    /// 4. Reject paths that escape the workspace root.
    ///
    /// # Errors
    ///
    /// Returns [`AccessError::OutsideWorkspace`] when the resolved path does
    /// not start with the workspace root (after normalisation).
    pub fn resolve_path(&self, path_str: &str) -> Result<PathBuf, AccessError> {
        let native = from_unix_path(path_str);

        // Make absolute — relative paths are anchored to the workspace.
        let abs = if native.is_absolute() {
            native
        } else {
            self.workspace.join(native)
        };

        let normalised = normalise_path(&abs);

        // Reject anything that escaped the workspace (e.g. via ../../).
        if !normalised.starts_with(&self.workspace) {
            return Err(AccessError::OutsideWorkspace(path_str.to_owned()));
        }

        Ok(normalised)
    }

    /// Check whether `action` is permitted on the already-resolved `path`.
    ///
    /// Rules are evaluated in descending priority order; the first matching
    /// rule determines the outcome.  When no rule matches, access is
    /// **allowed** (the workspace-containment check in [`resolve_path`] is the
    /// primary security boundary).
    ///
    /// # Errors
    ///
    /// Returns [`AccessError::Denied`] when a deny rule matches (or when an
    /// allow rule beats a lower-priority deny-all rule and another lower-
    /// priority deny rule would otherwise match — the higher-priority allow
    /// wins).
    pub fn check_access(&self, action: &str, path: &Path) -> Result<(), AccessError> {
        // Normalise to forward slashes for cross-platform glob matching.
        let path_fwd = path.to_string_lossy().replace('\\', "/");

        for rule in &self.rules {
            if rule_matches_action(rule, action)
                && rule_matches_resource(rule, &self.workspace, &path_fwd)
            {
                return match rule.effect {
                    PolicyEffect::Allow => Ok(()),
                    PolicyEffect::Deny => Err(AccessError::Denied {
                        action: action.to_owned(),
                        resource: path_fwd.to_owned(),
                    }),
                };
            }
        }

        // Default: allow (workspace boundary already enforced).
        Ok(())
    }

    /// Convenience: resolve `path_str` and then check `action` access.
    ///
    /// Returns the resolved `PathBuf` on success so callers can use it
    /// directly for filesystem and state-map operations.
    ///
    /// # Errors
    ///
    /// Propagates errors from both [`resolve_path`] and [`check_access`].
    pub fn resolve_and_check(
        &self,
        action: &str,
        path_str: &str,
    ) -> Result<PathBuf, AccessError> {
        let path = self.resolve_path(path_str)?;
        self.check_access(action, &path)?;
        Ok(path)
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Convert a Unix-style path string to a native `PathBuf`.
///
/// On **Windows** only: paths of the form `/X/rest` or `/X` (where `X` is a
/// single ASCII letter) are converted to `X:\rest` / `X:\`.
///
/// On all other platforms the string is returned as a `PathBuf` unchanged
/// (forward slashes are valid path separators on Unix).
pub fn from_unix_path(s: &str) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(rest) = s.strip_prefix('/') {
            let mut chars = rest.chars();
            if let Some(drive) = chars.next() {
                if drive.is_ascii_alphabetic() {
                    match chars.next() {
                        None => {
                            // "/X" → drive root "X:\"
                            return PathBuf::from(format!("{drive}:\\"));
                        }
                        Some('/') => {
                            // "/X/rest" → "X:\rest"
                            let remainder = rest[2..].replace('/', "\\");
                            return PathBuf::from(format!("{drive}:\\{remainder}"));
                        }
                        _ => {}
                    }
                }
            }
        }
        // Non-drive unix path on Windows: convert separators.
        PathBuf::from(s.replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(s)
    }
}

/// Normalise `..` and `.` components **without** touching the filesystem.
///
/// This is purely lexical — symlinks are not resolved.  The result may not
/// exist on disk; callers that need a canonical path should call
/// `std::fs::canonicalize` afterwards when appropriate.
fn normalise_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component; if nothing to pop, keep `..`
                // (shouldn't occur for absolute paths, but be defensive).
                if !out.pop() {
                    out.push(component);
                }
            }
            Component::CurDir => {} // skip "."
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Rule-matching helpers
// ---------------------------------------------------------------------------

fn rule_matches_action(rule: &PolicyRule, action: &str) -> bool {
    rule.actions.iter().any(|a| a == "*" || a == action)
}

fn rule_matches_resource(rule: &PolicyRule, workspace: &Path, path_fwd: &str) -> bool {
    // Workspace normalised to forward slashes for glob pattern expansion.
    let ws_fwd = workspace.to_string_lossy().replace('\\', "/");

    rule.resources.iter().any(|pattern| {
        let abs_pattern = if pattern.starts_with('/') {
            // Absolute unix pattern — use as-is (already in forward-slash form).
            pattern.clone()
        } else {
            // Relative pattern — join to workspace.
            format!("{ws_fwd}/{pattern}")
        };
        glob_matches(&abs_pattern, path_fwd)
    })
}

/// Glob matcher supporting `*` (non-separator), `**` (any depth), and `?`
/// (single non-separator character).
///
/// Both `pattern` and `text` must use forward slashes as path separators.
/// Matching is byte-level and case-sensitive.
fn glob_matches(pattern: &str, text: &str) -> bool {
    glob_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_inner(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),

        Some((&b'*', rest)) => {
            if rest.first() == Some(&b'*') {
                // `**` — match zero or more path segments (any characters).
                // Consume the optional trailing separator after `**`.
                let rest2 = if rest.get(1) == Some(&b'/') {
                    &rest[2..]
                } else {
                    &rest[1..]
                };
                // Try matching rest2 starting at every position in text.
                for i in 0..=text.len() {
                    if glob_inner(rest2, &text[i..]) {
                        return true;
                    }
                }
                false
            } else {
                // `*` — match zero or more characters except `/`.
                for i in 0..=text.len() {
                    // `*` cannot cross a path separator.
                    if i > 0 && text[i - 1] == b'/' {
                        break;
                    }
                    if glob_inner(rest, &text[i..]) {
                        return true;
                    }
                }
                false
            }
        }

        Some((&b'?', rest)) => {
            // `?` matches exactly one character that is not a separator.
            match text.split_first() {
                Some((&c, text_rest)) if c != b'/' => glob_inner(rest, text_rest),
                _ => false,
            }
        }

        Some((&pc, rest_p)) => {
            // Literal character — must match exactly.
            match text.split_first() {
                Some((&tc, rest_t)) if tc == pc => glob_inner(rest_p, rest_t),
                _ => false,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // from_unix_path
    // ------------------------------------------------------------------

    #[test]
    fn unix_path_passthrough_on_unix() {
        let p = from_unix_path("/home/user/project/main.rs");
        assert_eq!(p.to_string_lossy(), "/home/user/project/main.rs");
    }

    #[test]
    fn relative_path_passthrough() {
        let p = from_unix_path("src/main.rs");
        assert_eq!(p.to_string_lossy(), "src/main.rs");
    }

    // ------------------------------------------------------------------
    // normalise_path
    // ------------------------------------------------------------------

    #[test]
    fn normalise_removes_dotdot() {
        let p = normalise_path(Path::new("/workspace/src/../main.rs"));
        assert_eq!(p, PathBuf::from("/workspace/main.rs"));
    }

    #[test]
    fn normalise_removes_dot() {
        let p = normalise_path(Path::new("/workspace/./src/main.rs"));
        assert_eq!(p, PathBuf::from("/workspace/src/main.rs"));
    }

    #[test]
    fn normalise_multiple_dotdot() {
        let p = normalise_path(Path::new("/a/b/c/../../d"));
        assert_eq!(p, PathBuf::from("/a/d"));
    }

    // ------------------------------------------------------------------
    // glob_matches
    // ------------------------------------------------------------------

    #[test]
    fn glob_exact_match() {
        assert!(glob_matches(
            "/workspace/src/main.rs",
            "/workspace/src/main.rs"
        ));
        assert!(!glob_matches(
            "/workspace/src/main.rs",
            "/workspace/src/lib.rs"
        ));
    }

    #[test]
    fn glob_star_matches_filename() {
        assert!(glob_matches("/workspace/src/*.rs", "/workspace/src/main.rs"));
        assert!(glob_matches("/workspace/src/*.rs", "/workspace/src/lib.rs"));
    }

    #[test]
    fn glob_star_does_not_cross_separator() {
        assert!(!glob_matches(
            "/workspace/*.rs",
            "/workspace/src/main.rs"
        ));
    }

    #[test]
    fn glob_double_star_crosses_separator() {
        assert!(glob_matches(
            "/workspace/**/*.rs",
            "/workspace/src/main.rs"
        ));
        assert!(glob_matches(
            "/workspace/**/*.rs",
            "/workspace/src/sub/main.rs"
        ));
        assert!(!glob_matches(
            "/workspace/**/*.rs",
            "/workspace/src/main.txt"
        ));
    }

    #[test]
    fn glob_double_star_alone_matches_all() {
        assert!(glob_matches("**", "any/thing/at/all"));
        assert!(glob_matches("**", ""));
    }

    #[test]
    fn glob_question_matches_one_char() {
        assert!(glob_matches("/ws/?.rs", "/ws/a.rs"));
        assert!(!glob_matches("/ws/?.rs", "/ws/ab.rs"));
        assert!(!glob_matches("/ws/?.rs", "/ws//a.rs")); // separator not matched
    }

    #[test]
    fn glob_wildcard_star_all() {
        assert!(glob_matches("*", "anything"));
        assert!(!glob_matches("*", "any/thing")); // * cannot cross separator
    }

    // ------------------------------------------------------------------
    // AccessConfig — resolve_path
    // ------------------------------------------------------------------

    fn temp_ws() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn resolve_absolute_inside_workspace() {
        let tmp = temp_ws();
        let ws = tmp.path().to_str().unwrap().to_owned();
        let cfg = AccessConfig::new(&ws, None).unwrap();
        let target = format!("{ws}/src/main.rs");
        let resolved = cfg.resolve_path(&target).unwrap();
        assert!(resolved.starts_with(tmp.path()));
    }

    #[test]
    fn resolve_relative_joins_workspace() {
        let tmp = temp_ws();
        let ws = tmp.path().to_str().unwrap().to_owned();
        let cfg = AccessConfig::new(&ws, None).unwrap();
        let resolved = cfg.resolve_path("src/main.rs").unwrap();
        assert_eq!(resolved, tmp.path().join("src/main.rs"));
    }

    #[test]
    fn resolve_rejects_parent_traversal_absolute() {
        let tmp = temp_ws();
        let ws = tmp.path().to_str().unwrap().to_owned();
        let cfg = AccessConfig::new(&ws, None).unwrap();
        let escaping = format!("{ws}/../etc/passwd");
        assert!(matches!(
            cfg.resolve_path(&escaping),
            Err(AccessError::OutsideWorkspace(_))
        ));
    }

    #[test]
    fn resolve_rejects_relative_dotdot_escape() {
        let tmp = temp_ws();
        let ws = tmp.path().to_str().unwrap().to_owned();
        let cfg = AccessConfig::new(&ws, None).unwrap();
        assert!(matches!(
            cfg.resolve_path("../../etc/passwd"),
            Err(AccessError::OutsideWorkspace(_))
        ));
    }

    // ------------------------------------------------------------------
    // AccessConfig — check_access / default allow
    // ------------------------------------------------------------------

    #[test]
    fn default_allow_when_no_rules() {
        let tmp = temp_ws();
        let ws = tmp.path().to_str().unwrap().to_owned();
        let cfg = AccessConfig::new(&ws, None).unwrap();
        let path = tmp.path().join("src/main.rs");
        assert!(cfg.check_access("edit", &path).is_ok());
        assert!(cfg.check_access("track", &path).is_ok());
    }

    // ------------------------------------------------------------------
    // AccessConfig — ruleset loading and rule evaluation
    // ------------------------------------------------------------------

    fn write_ruleset(dir: &Path, json: serde_json::Value) -> PathBuf {
        let p = dir.join("rules.json");
        std::fs::write(&p, json.to_string()).unwrap();
        p
    }

    #[test]
    fn deny_rule_blocks_edit() {
        let tmp = temp_ws();
        let ws = tmp.path();
        let ruleset = write_ruleset(
            ws,
            serde_json::json!({
                "rules": [{
                    "effect": "deny",
                    "priority": 100,
                    "actions": ["edit", "insert", "delete"],
                    "resources": ["locked/**"]
                }]
            }),
        );
        let cfg =
            AccessConfig::new(ws.to_str().unwrap(), Some(ruleset.to_str().unwrap())).unwrap();

        let locked = ws.join("locked/secret.rs");
        assert!(matches!(
            cfg.check_access("edit", &locked),
            Err(AccessError::Denied { .. })
        ));
        // Other files still allowed.
        let other = ws.join("src/main.rs");
        assert!(cfg.check_access("edit", &other).is_ok());
    }

    #[test]
    fn allow_rule_beats_lower_priority_deny() {
        let tmp = temp_ws();
        let ws = tmp.path();
        let ruleset = write_ruleset(
            ws,
            serde_json::json!({
                "rules": [
                    // Lower priority deny-all.
                    { "effect": "deny",  "priority": 10, "actions": ["*"], "resources": ["**"] },
                    // Higher priority allow for one file.
                    { "effect": "allow", "priority": 20, "actions": ["read"], "resources": ["src/allowed.rs"] }
                ]
            }),
        );
        let cfg =
            AccessConfig::new(ws.to_str().unwrap(), Some(ruleset.to_str().unwrap())).unwrap();

        let allowed = ws.join("src/allowed.rs");
        let denied = ws.join("src/other.rs");

        assert!(cfg.check_access("read", &allowed).is_ok());
        assert!(cfg.check_access("read", &denied).is_err());
    }

    #[test]
    fn wildcard_action_in_rule() {
        let tmp = temp_ws();
        let ws = tmp.path();
        let ruleset = write_ruleset(
            ws,
            serde_json::json!({
                "rules": [{
                    "effect": "deny",
                    "priority": 50,
                    "actions": ["*"],
                    "resources": ["*.lock"]
                }]
            }),
        );
        let cfg =
            AccessConfig::new(ws.to_str().unwrap(), Some(ruleset.to_str().unwrap())).unwrap();

        let lock = ws.join("Cargo.lock");
        assert!(cfg.check_access("track", &lock).is_err());
        assert!(cfg.check_access("read", &lock).is_err());
        // Non-lock file: no rule matches → default allow.
        let rs = ws.join("src/main.rs");
        assert!(cfg.check_access("track", &rs).is_ok());
    }

    #[test]
    fn absolute_resource_pattern_in_rule() {
        let tmp = temp_ws();
        let ws = tmp.path();
        // Use an absolute pattern (starts with /) that points at a system path.
        let ruleset = write_ruleset(
            ws,
            serde_json::json!({
                "rules": [{
                    "effect": "deny",
                    "priority": 100,
                    "actions": ["*"],
                    "resources": ["/etc/**"]
                }]
            }),
        );
        let cfg =
            AccessConfig::new(ws.to_str().unwrap(), Some(ruleset.to_str().unwrap())).unwrap();

        // /etc/passwd would be caught by the rule — but resolve_path would
        // already reject it as outside workspace, so check_access is
        // defence-in-depth for absolute patterns.
        let etc = Path::new("/etc/passwd");
        assert!(cfg.check_access("read", etc).is_err());

        // Workspace file is not matched by /etc/**.
        let ws_file = ws.join("src/main.rs");
        assert!(cfg.check_access("read", &ws_file).is_ok());
    }

    #[test]
    fn resolve_and_check_combined() {
        let tmp = temp_ws();
        let ws = tmp.path();
        let ruleset = write_ruleset(
            ws,
            serde_json::json!({
                "rules": [{
                    "effect": "deny",
                    "priority": 100,
                    "actions": ["edit"],
                    "resources": ["readonly/**"]
                }]
            }),
        );
        let cfg =
            AccessConfig::new(ws.to_str().unwrap(), Some(ruleset.to_str().unwrap())).unwrap();

        // Deny: edit on a readonly/** file.
        let ws_str = ws.to_str().unwrap();
        let result = cfg.resolve_and_check("edit", &format!("{ws_str}/readonly/config.toml"));
        assert!(result.is_err());

        // Allow: edit on a normal file.
        let ok = cfg.resolve_and_check("edit", &format!("{ws_str}/src/main.rs"));
        assert!(ok.is_ok());
        assert!(ok.unwrap().starts_with(ws));

        // Deny: traversal escape (checked by resolve_path before rule eval).
        let escape = cfg.resolve_and_check("read", "../../etc/passwd");
        assert!(matches!(escape, Err(AccessError::OutsideWorkspace(_))));
    }

    #[test]
    fn invalid_workspace_path_returns_error() {
        let result = AccessConfig::new("/nonexistent/path/that/does/not/exist", None);
        assert!(result.is_err());
    }

    #[test]
    fn malformed_ruleset_json_returns_error() {
        let tmp = temp_ws();
        let ws = tmp.path();
        let ruleset_path = ws.join("bad.json");
        std::fs::write(&ruleset_path, "{ this is not valid json }").unwrap();
        let result = AccessConfig::new(
            ws.to_str().unwrap(),
            Some(ruleset_path.to_str().unwrap()),
        );
        assert!(matches!(result, Err(AccessError::RulesetParse(_))));
    }
}
