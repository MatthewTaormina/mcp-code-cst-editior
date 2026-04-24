use std::sync::Arc;

use rmcp::{
    handler::server::wrapper::Parameters, schemars, tool, tool_router,
};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::access::AccessConfig;
use crate::cst::CstFile;
use crate::state::ServerState;
use crate::watcher::{watch_path, unwatch_path, WatcherHandle};

// ---------------------------------------------------------------------------
// Parameter structs (each tool's input schema)
// ---------------------------------------------------------------------------

/// Parameters for `track_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TrackParams {
    /// Absolute path to the file that should be monitored and held in memory.
    pub path: String,
}

/// Parameters for `untrack_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UntrackParams {
    /// Absolute path of the file to remove from active tracking.
    pub path: String,
}

/// Parameters for `load_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LoadParams {
    /// Absolute path of the already-tracked file to inspect.
    pub path: String,
}

/// Parameters for `get_node`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetNodeParams {
    /// Absolute path of the tracked file.
    pub path: String,
    /// 0-based line index of the node to retrieve.
    pub node_id: u32,
}

/// Parameters for `get_tree_skeleton`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SkeletonParams {
    /// Absolute path of the tracked file to inspect.
    pub path: String,
}

/// Parameters for `edit_node`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EditParams {
    /// Absolute path of the tracked file to edit.
    pub path: String,
    /// 0-based line index of the node (line) to replace.
    pub node_id: u32,
    /// New text content for the target line (without a trailing newline;
    /// the server preserves the original line-ending).
    pub new_text: String,
    /// If provided, the edit is only applied when the file's current CST
    /// version matches this value.  Use this to detect conflicts: obtain the
    /// version from a prior `load_file` or `get_node` call, and pass it here
    /// so the server can reject the edit if the file was reloaded by the
    /// watcher (or by another edit) in the meantime.
    pub expected_version: Option<u64>,
}

/// Parameters for `get_line_tokens`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetLineTokensParams {
    /// Absolute path of the tracked file.
    pub path: String,
    /// 0-based line index of the node whose tokens should be returned.
    pub node_id: u32,
}

/// Parameters for `insert_lines`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertLinesParams {
    /// Absolute path of the tracked file to modify.
    pub path: String,
    /// 0-based index of the line *after which* the new lines are inserted.
    /// Omit (or pass `null`) to prepend at the beginning of the file.
    pub insert_after: Option<u32>,
    /// One or more line strings to insert.  A trailing `\n` is appended
    /// automatically to any string that lacks one.
    pub lines: Vec<String>,
    /// If provided, the edit is rejected when the file's actual version
    /// differs from this value (optimistic concurrency control).
    pub expected_version: Option<u64>,
}

/// Parameters for `delete_lines`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteLinesParams {
    /// Absolute path of the tracked file to modify.
    pub path: String,
    /// 0-based index of the first line to delete.
    pub node_id: u32,
    /// Number of consecutive lines to remove starting at `node_id`.
    /// Must be at least 1.
    pub count: u32,
    /// If provided, the edit is rejected when the file's actual version
    /// differs from this value (optimistic concurrency control).
    pub expected_version: Option<u64>,
}

/// Parameters for `list_tracked_files` (no fields — lists all tracked paths).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListTrackedFilesParams {}

/// Parameters for `create_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateFileParams {
    /// Absolute path of the file to create (must be inside the workspace).
    pub path: String,
    /// Initial content for the new file.  Defaults to empty if omitted.
    pub content: Option<String>,
    /// When `true`, the newly-created file is automatically tracked in memory
    /// after it is written to disk.  Defaults to `false`.
    pub track: Option<bool>,
}

/// Parameters for `delete_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteFileParams {
    /// Absolute path of the file to delete (must be inside the workspace).
    pub path: String,
}

/// Parameters for `save_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveParams {
    /// Absolute path of the tracked file to flush to disk.
    pub path: String,
}

// ── Phase 6: query + help ────────────────────────────────────────────────────

/// Parameters for `query_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryFileParams {
    /// Path to the tracked file to query (absolute or workspace-relative).
    pub path: String,
    /// Structured query expression — see `query_tool` for full documentation.
    pub query: crate::cst::QueryExpr,
}

/// Optional graph-search specification (reserved for future use).
///
/// When graph search is implemented, this will allow expressing relationship
/// queries such as "files that import symbol X", "callers of function Y", or
/// "modules reachable from entry point Z".  The schema will be stabilised in a
/// future release; pass `null` or omit this field to use the current text/
/// semantic search behaviour.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GraphQuery {
    /// Relationship kind to traverse (e.g. `"imports"`, `"calls"`,
    /// `"references"`).  Reserved — not yet evaluated.
    pub relation: Option<String>,
    /// Starting file or symbol for the graph traversal.  Reserved.
    pub from: Option<String>,
    /// Maximum edge hops to follow.  Reserved.
    pub max_depth: Option<u32>,
}

/// Parameters for `query_workspace`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryWorkspaceParams {
    /// Structured query to run against every tracked file.
    pub query: crate::cst::QueryExpr,
    /// Graph search specification.  Currently reserved for future use — pass
    /// `null` or omit.  When graph search is implemented this will allow
    /// relationship-aware queries across the tracked file set (import graphs,
    /// call graphs, reference chains, etc.).
    pub graph: Option<GraphQuery>,
}

/// Parameters for `query_tool` (help / tool catalog).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryToolParams {
    /// Name of the specific tool to look up (`"track_file"`, `"query_file"`,
    /// …).  Omit to receive the full catalog plus a tool-selection guide.
    pub tool_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

/// The MCP server handler that exposes CST editing capabilities as tools.
#[derive(Clone)]
pub struct CstMcpServer {
    state: Arc<RwLock<ServerState>>,
    watcher: WatcherHandle,
    access: Arc<AccessConfig>,
}

impl CstMcpServer {
    pub fn new(
        state: Arc<RwLock<ServerState>>,
        watcher: WatcherHandle,
        access: Arc<AccessConfig>,
    ) -> Self {
        Self {
            state,
            watcher,
            access,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[tool_router(server_handler)]
impl CstMcpServer {
    /// Begin tracking a file: read it from disk, parse it into the in-memory
    /// CST, and register it with the filesystem watcher for auto-reload.
    #[tool(description = "Track a file: load it into memory as a CST and watch for external changes.")]
    async fn track_file(
        &self,
        Parameters(TrackParams { path }): Parameters<TrackParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("track", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };

        match std::fs::read_to_string(&path) {
            Err(e) => format!("error: could not read {path:?}: {e}"),
            Ok(content) => {
                let file = CstFile::parse(path.clone(), &content);
                self.state.write().await.track(path.clone(), file);

                // Best-effort registration with the watcher; if it fails we
                // still track the file (manual reloads via `track_file` work).
                if let Err(e) = watch_path(&self.watcher, &path) {
                    eprintln!("watcher: could not watch {path:?}: {e}");
                }

                format!("ok: tracking {path:?}")
            }
        }
    }

    /// Stop tracking a file and release its in-memory CST.
    #[tool(description = "Untrack a file: remove it from memory and stop watching for changes.")]
    async fn untrack_file(
        &self,
        Parameters(UntrackParams { path }): Parameters<UntrackParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("untrack", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let removed = self.state.write().await.untrack(&path);

        if removed {
            // Best-effort deregistration from the watcher.
            if let Err(e) = unwatch_path(&self.watcher, &path) {
                eprintln!("watcher: could not unwatch {path:?}: {e}");
            }
            format!("ok: untracked {path:?}")
        } else {
            format!("error: {path:?} was not being tracked")
        }
    }

    /// Return a summary of the in-memory CST for a tracked file.
    ///
    /// This includes the number of lines (top-level CST nodes) and the
    /// current version counter, which increments on every reload or edit.
    #[tool(description = "Inspect the in-memory CST of a tracked file (line count, version).")]
    async fn load_file(
        &self,
        Parameters(LoadParams { path }): Parameters<LoadParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("load", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;

        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => {
                let text = file.to_text();
                let line_count = text.lines().count();
                format!(
                    "ok: {path:?} — {line_count} lines, CST version {}",
                    file.version
                )
            }
        }
    }

    /// Return full metadata for a single CST node (line).
    ///
    /// The response is a JSON object with the node's `node_id`, `kind`,
    /// full `text` (including any trailing newline), byte `span` offsets,
    /// and the file's current `version`.  Use the `version` field with
    /// `edit_node`'s `expected_version` parameter to detect edit conflicts.
    #[tool(
        description = "Get the text content, kind, and byte-span of a single line node in the CST. \
                        Returns JSON. Use the returned version with edit_node to avoid conflicts."
    )]
    async fn get_node(
        &self,
        Parameters(GetNodeParams { path, node_id }): Parameters<GetNodeParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("read", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;

        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => match file.get_node(node_id) {
                Err(e) => format!("error: {e}"),
                Ok(info) => {
                    let version = file.version;
                    serde_json::json!({
                        "node_id": info.node_id,
                        "kind": info.kind,
                        "text": info.text,
                        "span": { "start": info.span_start, "end": info.span_end },
                        "version": version,
                    })
                    .to_string()
                }
            },
        }
    }

    /// Return a structural listing of all line nodes in a tracked file's CST.
    ///
    /// The response is a JSON array, one object per line, each containing
    /// `node_id`, `kind`, `text_preview` (truncated), byte `span`, and the
    /// file's current `version`.  Use this to navigate the file before
    /// calling `get_node` or `edit_node`.
    #[tool(
        description = "List all CST line nodes with their IDs, spans, and text previews. \
                        Returns a JSON array. Use node_id values with get_node or edit_node."
    )]
    async fn get_tree_skeleton(
        &self,
        Parameters(SkeletonParams { path }): Parameters<SkeletonParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("read", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;

        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => {
                let version = file.version;
                let nodes: Vec<_> = file
                    .tree_skeleton()
                    .into_iter()
                    .map(|info| {
                        serde_json::json!({
                            "node_id": info.node_id,
                            "kind": info.kind,
                            "text_preview": info.text_preview(),
                            "span": { "start": info.span_start, "end": info.span_end },
                        })
                    })
                    .collect();

                serde_json::json!({
                    "version": version,
                    "nodes": nodes,
                })
                .to_string()
            }
        }
    }

    /// Replace the content of one line (identified by its 0-based `node_id`)
    /// inside a tracked file's in-memory CST.
    ///
    /// All other lines — including their whitespace and comments — are
    /// preserved verbatim (lossless round-trip via rowan).
    ///
    /// If `expected_version` is supplied, the edit is rejected with a
    /// `"conflict"` response when the file's actual version differs.  This
    /// protects against clobbering a watcher-triggered reload that happened
    /// between your `load_file`/`get_node` call and this `edit_node` call.
    #[tool(
        description = "Edit a single line node in the CST of a tracked file. All other lines are \
                        preserved verbatim.  Pass expected_version (from get_node or load_file) to \
                        guard against concurrent external file changes."
    )]
    async fn edit_node(
        &self,
        Parameters(EditParams {
            path,
            node_id,
            new_text,
            expected_version,
        }): Parameters<EditParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };

        // Build the new CST outside of the write lock to minimise hold time.
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => {
                    // Version conflict detection: if the caller supplied an
                    // expected_version and it doesn't match the actual version,
                    // the file was modified (by the watcher or another edit)
                    // since the caller last read it.
                    if let Some(expected) = expected_version {
                        if file.version != expected {
                            return format!(
                                "conflict: {path:?} is at version {} but expected version {} — \
                                 re-read the file with load_file or get_node and retry",
                                file.version, expected
                            );
                        }
                    }
                    file.replace_node(node_id, &new_text)
                }
            }
        };

        match new_file {
            Err(e) => format!("error: {e}"),
            Ok(new_file) => {
                let version = new_file.version;
                self.state.write().await.track(path.clone(), new_file);
                format!("ok: node {node_id} in {path:?} updated (CST version {version})")
            }
        }
    }

    /// Return a sorted list of every file currently held in memory.
    ///
    /// The response is a JSON object with a `count` field and a `files` array
    /// of absolute path strings.  Use this to check which files are available
    /// for inspection or editing without making a separate `load_file` call
    /// for each candidate.
    #[tool(
        description = "List all files currently tracked in memory. \
                        Returns JSON: {count, files: [sorted absolute paths]}."
    )]
    async fn list_tracked_files(
        &self,
        Parameters(ListTrackedFilesParams {}): Parameters<ListTrackedFilesParams>,
    ) -> String {
        let state = self.state.read().await;
        let files: Vec<String> = state
            .tracked_paths()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        serde_json::json!({
            "count": files.len(),
            "files": files,
        })
        .to_string()
    }

    /// Return the token-level children of a single Line node.
    ///
    /// For `.rs` files, each token carries a semantic `kind` — one of
    /// `"Keyword"`, `"Identifier"`, `"Literal"`, `"Comment"`,
    /// `"Whitespace"`, `"Newline"`, or `"Punctuation"`.  For all other file
    /// types each line contains a single `"Text"` token (the same as the
    /// plain-text grammar used before language-specific lexing was added).
    ///
    /// Use `get_tree_skeleton` to discover valid `node_id` values, then call
    /// this tool to inspect sub-line token structure.
    #[tool(
        description = "Get the token-level children of a Line node in the CST. \
                        Returns JSON: {line_node_id, language, tokens: [{token_idx, kind, text, span}], version}. \
                        Kind values for .rs files: Keyword | Identifier | Literal | Comment | Whitespace | Newline | Punctuation."
    )]
    async fn get_line_tokens(
        &self,
        Parameters(GetLineTokensParams { path, node_id }): Parameters<GetLineTokensParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("read", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;

        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => match file.get_line_tokens(node_id) {
                Err(e) => format!("error: {e}"),
                Ok(tokens) => {
                    let version = file.version;
                    let lang = format!("{:?}", file.language());
                    let token_values: Vec<_> = tokens
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "token_idx": t.token_idx,
                                "kind": t.kind,
                                "text": t.text,
                                "span": { "start": t.span_start, "end": t.span_end },
                            })
                        })
                        .collect();

                    serde_json::json!({
                        "line_node_id": node_id,
                        "language": lang,
                        "tokens": token_values,
                        "version": version,
                    })
                    .to_string()
                }
            },
        }
    }

    /// Flush the current in-memory CST for a tracked file back to disk.
    ///
    /// The file is reconstructed from the rowan tree, guaranteeing a lossless
    /// round-trip for all unedited content.
    #[tool(description = "Save the in-memory CST of a tracked file to disk (lossless round-trip).")]
    async fn save_file(
        &self,
        Parameters(SaveParams { path }): Parameters<SaveParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("save", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;

        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => {
                let text = file.to_text();
                let version = file.version;
                // Drop the read lock before the (potentially slow) write.
                drop(state);

                match std::fs::write(&path, text.as_bytes()) {
                    Ok(()) => format!("ok: saved {path:?} (CST version {version})"),
                    Err(e) => format!("error: could not write {path:?}: {e}"),
                }
            }
        }
    }

    /// Insert one or more new lines at a position in a tracked file's CST.
    ///
    /// Lines are inserted *after* `insert_after` (0-based line index).  Set
    /// `insert_after` to `null` (or omit it) to prepend lines at the
    /// beginning of the file.  Trailing `\n` is added automatically to any
    /// line that lacks one.
    ///
    /// All existing lines — including their whitespace and comments — are
    /// preserved verbatim.  The insertion uses rowan's native `splice_children`
    /// so unchanged Line nodes are shared without re-parsing.
    ///
    /// Pass `expected_version` (from a prior `get_node` or `load_file`) to
    /// guard against concurrent external file changes.
    #[tool(
        description = "Insert one or more new lines into the CST of a tracked file. \
                        insert_after=null prepends at start; insert_after=N inserts after line N. \
                        Returns JSON: {inserted_count, first_node_id, version}."
    )]
    async fn insert_lines(
        &self,
        Parameters(InsertLinesParams {
            path,
            insert_after,
            lines,
            expected_version,
        }): Parameters<InsertLinesParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("insert", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };

        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => {
                    if let Some(expected) = expected_version {
                        if file.version != expected {
                            return format!(
                                "conflict: {path:?} is at version {} but expected version {} — \
                                 re-read the file with load_file or get_node and retry",
                                file.version, expected
                            );
                        }
                    }
                    file.insert_lines(insert_after, &lines)
                }
            }
        };

        match new_file {
            Err(e) => format!("error: {e}"),
            Ok(new_file) => {
                let version = new_file.version;
                let inserted_count = lines.len();
                let first_node_id: u32 = match insert_after {
                    None => 0,
                    Some(id) => id + 1,
                };
                self.state.write().await.track(path.clone(), new_file);
                serde_json::json!({
                    "inserted_count": inserted_count,
                    "first_node_id": first_node_id,
                    "version": version,
                })
                .to_string()
            }
        }
    }

    /// Delete one or more consecutive Line nodes from a tracked file's CST.
    ///
    /// `node_id` is the 0-based index of the first line to remove.  `count`
    /// specifies how many consecutive lines to delete (minimum 1).  All
    /// remaining lines are preserved verbatim and their `node_id`s are
    /// compacted down to fill the gap.
    ///
    /// The deletion uses rowan's native `splice_children` so unchanged Line
    /// nodes are shared without re-parsing.
    ///
    /// Pass `expected_version` (from a prior `get_node` or `load_file`) to
    /// guard against concurrent external file changes.
    #[tool(
        description = "Delete one or more consecutive line nodes from the CST of a tracked file. \
                        node_id is the first line to delete; count is how many to remove (≥ 1). \
                        Returns ok: or conflict: or error: string."
    )]
    async fn delete_lines(
        &self,
        Parameters(DeleteLinesParams {
            path,
            node_id,
            count,
            expected_version,
        }): Parameters<DeleteLinesParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("delete", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };

        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => {
                    if let Some(expected) = expected_version {
                        if file.version != expected {
                            return format!(
                                "conflict: {path:?} is at version {} but expected version {} — \
                                 re-read the file with load_file or get_node and retry",
                                file.version, expected
                            );
                        }
                    }
                    file.delete_lines(node_id, count)
                }
            }
        };

        match new_file {
            Err(e) => format!("error: {e}"),
            Ok(new_file) => {
                let version = new_file.version;
                self.state.write().await.track(path.clone(), new_file);
                format!(
                    "ok: deleted {count} line(s) starting at node {node_id} in {path:?} \
                     (CST version {version})"
                )
            }
        }
    }

    /// Create a new file on disk inside the workspace.
    ///
    /// The file is written with the provided `content` (or empty if omitted).
    /// If `track` is `true`, the file is also loaded into the in-memory CST
    /// immediately (equivalent to calling `track_file` afterwards).
    ///
    /// Returns an error if the file already exists or if the path is outside
    /// the workspace root.
    #[tool(
        description = "Create a new file on disk inside the workspace with optional initial content. \
                        Set track=true to immediately load it into memory. \
                        Returns \"ok: created <path>\" or \"error: …\"."
    )]
    async fn create_file(
        &self,
        Parameters(CreateFileParams { path, content, track }): Parameters<CreateFileParams>,
    ) -> String {
        let resolved = match self.access.resolve_and_check("create", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };

        // Refuse to overwrite an existing file.
        if resolved.exists() {
            return format!("error: {resolved:?} already exists");
        }

        // Create parent directories if necessary.
        if let Some(parent) = resolved.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return format!("error: could not create parent directories for {resolved:?}: {e}");
                }
            }
        }

        let body = content.unwrap_or_default();
        if let Err(e) = std::fs::write(&resolved, body.as_bytes()) {
            return format!("error: could not write {resolved:?}: {e}");
        }

        let mut msg = format!("ok: created {resolved:?}");

        // Optionally track the new file immediately.
        if track.unwrap_or(false) {
            match std::fs::read_to_string(&resolved) {
                Err(e) => {
                    msg.push_str(&format!("; warning: track failed: {e}"));
                }
                Ok(text) => {
                    let file = CstFile::parse(resolved.clone(), &text);
                    self.state.write().await.track(resolved.clone(), file);
                    if let Err(e) = watch_path(&self.watcher, &resolved) {
                        eprintln!("watcher: could not watch {resolved:?}: {e}");
                    }
                    msg.push_str("; tracking");
                }
            }
        }

        msg
    }

    /// Delete a file from disk.
    ///
    /// If the file is currently tracked it is automatically untracked (and
    /// unwatched) before deletion so the in-memory state stays consistent.
    ///
    /// Returns an error if the file does not exist or is outside the
    /// workspace root.
    #[tool(
        description = "Delete a file from disk inside the workspace. \
                        If the file is currently tracked it is automatically untracked first. \
                        Returns \"ok: deleted <path>\" or \"error: …\"."
    )]
    async fn delete_file(
        &self,
        Parameters(DeleteFileParams { path }): Parameters<DeleteFileParams>,
    ) -> String {
        let resolved = match self.access.resolve_and_check("delete_file", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };

        if !resolved.exists() {
            return format!("error: {resolved:?} does not exist");
        }

        // Untrack first so in-memory state stays consistent.
        let was_tracked = self.state.write().await.untrack(&resolved);
        if was_tracked {
            if let Err(e) = unwatch_path(&self.watcher, &resolved) {
                eprintln!("watcher: could not unwatch {resolved:?}: {e}");
            }
        }

        match std::fs::remove_file(&resolved) {
            Ok(()) => format!("ok: deleted {resolved:?}"),
            Err(e) => format!("error: could not delete {resolved:?}: {e}"),
        }
    }

    /// Query the CST of a single tracked file using structured filters,
    /// semantic patterns, and scope-depth constraints.
    ///
    /// ## Filters
    ///
    /// All filter fields in `query` are optional and AND-ed together.
    ///
    /// **Text / kind filters** (`kind`, `text_contains`, `text_glob`) match
    /// the text of a line or token.  `kind` is case-insensitive.
    ///
    /// **Line-range filter** (`node_id_from`, `node_id_to`) restricts the
    /// search to a contiguous block of lines.
    ///
    /// **Depth** (`depth`) selects `"line"` (default) or `"token"` level.
    ///
    /// **Semantic patterns** (`semantic`) find named syntactic constructs in
    /// Rust files and return a `capture` (function name, variable name, …):
    /// `fn_def`, `struct_def`, `enum_def`, `trait_def`, `impl_block`,
    /// `type_def`, `variable_def`, `use_stmt`, `macro_call`.
    ///
    /// **Identifier search** (`identifier_name`) finds every token-level
    /// occurrence of an exact identifier name.
    ///
    /// **Scope-depth filters** (`scope_depth_min`, `scope_depth_max`) restrict
    /// results to a specific brace-nesting level (0 = top-level).  Combine
    /// with `semantic` to get, e.g., only top-level function definitions:
    /// `{"semantic":"fn_def","scope_depth_max":0}`.
    ///
    /// ## Response JSON
    ///
    /// ```json
    /// {
    ///   "file": "/abs/path",
    ///   "language": "Rust",
    ///   "version": 1,
    ///   "match_count": 2,
    ///   "matches": [
    ///     { "node_id":0, "kind":"Line", "text":"fn main() {\n",
    ///       "span_start":0, "span_end":12, "scope_depth":0,
    ///       "capture":"main" }
    ///   ]
    /// }
    /// ```
    #[tool(
        description = "Query the CST of a tracked file with text, semantic, identifier, and \
                        scope-depth filters.  Returns JSON: {file, language, version, \
                        match_count, matches:[{node_id,kind,text,span_start,span_end,\
                        scope_depth,token_idx?,capture?}]}."
    )]
    async fn query_file(
        &self,
        Parameters(QueryFileParams { path, query }): Parameters<QueryFileParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("query", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;
        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => {
                let matches = file.query(&query);
                let match_count = matches.len();
                let lang = format!("{:?}", file.language());
                let version = file.version;
                let match_values: Vec<_> = matches
                    .into_iter()
                    .map(|m| {
                        let mut obj = serde_json::json!({
                            "node_id": m.node_id,
                            "kind": m.kind,
                            "text": m.text,
                            "span_start": m.span_start,
                            "span_end": m.span_end,
                            "scope_depth": m.scope_depth,
                        });
                        if let Some(tidx) = m.token_idx {
                            obj["token_idx"] = serde_json::json!(tidx);
                        }
                        if let Some(cap) = m.capture {
                            obj["capture"] = serde_json::json!(cap);
                        }
                        obj
                    })
                    .collect();

                serde_json::json!({
                    "file": path.to_string_lossy(),
                    "language": lang,
                    "version": version,
                    "match_count": match_count,
                    "matches": match_values,
                })
                .to_string()
            }
        }
    }

    /// Run the same query across every tracked file in the workspace.
    ///
    /// The `query` field accepts exactly the same expression as `query_file`.
    /// Results are grouped by file and include per-file `version` and
    /// `match_count`.  Files with zero matches are omitted from the output.
    ///
    /// ## Graph search (future)
    ///
    /// The `graph` field is reserved for a planned cross-file relationship
    /// search layer (import graphs, call graphs, reference chains).  Pass
    /// `null` or omit it — it is accepted but ignored in the current release.
    ///
    /// ## Response JSON
    ///
    /// ```json
    /// {
    ///   "total_files_searched": 3,
    ///   "files_with_matches": 1,
    ///   "total_matches": 2,
    ///   "results": [
    ///     { "file": "/abs/path/a.rs", "language": "Rust", "version": 0,
    ///       "match_count": 2, "matches": [ … ] }
    ///   ]
    /// }
    /// ```
    #[tool(
        description = "Run a query across all tracked files in the workspace.  Same query \
                        expression as query_file.  The `graph` field is reserved for future \
                        cross-file relationship search (import/call graphs).  Returns JSON: \
                        {total_files_searched, files_with_matches, total_matches, results:[…]}."
    )]
    async fn query_workspace(
        &self,
        Parameters(QueryWorkspaceParams { query, graph: _ }): Parameters<QueryWorkspaceParams>,
    ) -> String {
        let state = self.state.read().await;
        let paths = state.tracked_paths();
        let total_files_searched = paths.len();
        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut total_matches: usize = 0;

        for path in paths {
            if let Some(file) = state.get(&path) {
                let matches = file.query(&query);
                if matches.is_empty() {
                    continue;
                }
                total_matches += matches.len();
                let lang = format!("{:?}", file.language());
                let version = file.version;
                let match_count = matches.len();
                let match_values: Vec<_> = matches
                    .into_iter()
                    .map(|m| {
                        let mut obj = serde_json::json!({
                            "node_id": m.node_id,
                            "kind": m.kind,
                            "text": m.text,
                            "span_start": m.span_start,
                            "span_end": m.span_end,
                            "scope_depth": m.scope_depth,
                        });
                        if let Some(tidx) = m.token_idx {
                            obj["token_idx"] = serde_json::json!(tidx);
                        }
                        if let Some(cap) = m.capture {
                            obj["capture"] = serde_json::json!(cap);
                        }
                        obj
                    })
                    .collect();

                results.push(serde_json::json!({
                    "file": path.to_string_lossy(),
                    "language": lang,
                    "version": version,
                    "match_count": match_count,
                    "matches": match_values,
                }));
            }
        }

        serde_json::json!({
            "total_files_searched": total_files_searched,
            "files_with_matches": results.len(),
            "total_matches": total_matches,
            "results": results,
        })
        .to_string()
    }

    /// Return documentation and tool-selection guidance for this MCP server.
    ///
    /// When `tool_name` is omitted, returns the complete tool catalog plus a
    /// categorised selection guide.  When `tool_name` is provided, returns
    /// detailed documentation for that one tool.
    ///
    /// Use this tool whenever you are unsure which tool to call next, or when
    /// you need parameter details, example queries, or a reminder of available
    /// semantic patterns.
    #[tool(
        description = "Get documentation and tool-selection guidance.  Omit tool_name for the \
                        full catalog + selection guide.  Provide tool_name for focused docs on \
                        one tool.  Call this when unsure which tool to use."
    )]
    async fn query_tool(
        &self,
        Parameters(QueryToolParams { tool_name }): Parameters<QueryToolParams>,
    ) -> String {
        let catalog = tool_catalog();
        match tool_name.as_deref() {
            None | Some("") => serde_json::json!({
                "selection_guide": SELECTION_GUIDE,
                "tool_count": catalog.len(),
                "tools": catalog,
            })
            .to_string(),
            Some(name) => match catalog.iter().find(|t| t["name"] == name) {
                Some(entry) => entry.to_string(),
                None => {
                    let names: Vec<_> = catalog
                        .iter()
                        .filter_map(|t| t["name"].as_str())
                        .collect();
                    format!(
                        "error: unknown tool {:?}.  Available: {}",
                        name,
                        names.join(", ")
                    )
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Static tool catalog used by query_tool
// ---------------------------------------------------------------------------

const SELECTION_GUIDE: &str = "\
TRACKING — before reading or editing a file you must track it:\
\n  1. track_file        → loads file, starts watching for external changes\
\n  2. list_tracked_files → see what is already tracked\
\n  3. untrack_file      → release a file from memory\
\
\nFILE MANAGEMENT — create or delete files on disk:\
\n  4. create_file       → create a new file (optionally with content; set track=true to load it immediately)\
\n  5. delete_file       → delete a file from disk (auto-untracks if it was tracked)\
\
\nINSPECTION — explore a tracked file:\
\n  6. load_file         → quick summary: line count + version\
\n  7. get_tree_skeleton → all line node_ids with text previews and spans\
\n  8. get_node          → full text + span for one line (use node_id from skeleton)\
\n  9. get_line_tokens   → token-level breakdown of one line (Rust files: keyword/ident/…)\
\
\nQUERY — find content by pattern (no need to track first, works on already-tracked files):\
\n 10. query_file        → text, semantic, identifier, or scope-depth search in one file\
\n 11. query_workspace   → same query across all tracked files; reserved `graph` field for\
\n                          future cross-file import/call-graph search\
\
\nEDITING — mutate the in-memory CST (always pass expected_version to guard conflicts):\
\n 12. edit_node         → replace one line\
\n 13. insert_lines      → insert one or more lines at a position\
\n 14. delete_lines      → remove one or more consecutive lines\
\n 15. save_file         → flush in-memory CST back to disk\
\
\nHELP:\
\n 16. query_tool        → this tool; get docs for any tool or the full catalog\
\
\nTYPICAL WORKFLOW:\
\n  track_file → query_file (find target) → get_node (read version) → edit_node → save_file\
\nCREATE WORKFLOW:\
\n  create_file (track=true) → insert_lines → save_file\
\nDELETE WORKFLOW:\
\n  delete_file (auto-untracks)";


fn tool_catalog() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "track_file",
            "category": "tracking",
            "description": "Load a file from disk into memory, parse it as a CST, and register it with the filesystem watcher for auto-reload.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Absolute or workspace-relative path to the file."}
            ],
            "returns": "\"ok: tracking <path>\" or \"error: …\"",
        }),
        serde_json::json!({
            "name": "untrack_file",
            "category": "tracking",
            "description": "Remove a file from memory and stop watching it for changes.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Path of the tracked file to release."}
            ],
            "returns": "\"ok: untracked <path>\" or \"error: …\"",
        }),
        serde_json::json!({
            "name": "list_tracked_files",
            "category": "tracking",
            "description": "List every file currently held in memory, sorted lexicographically.",
            "parameters": [],
            "returns": "JSON {count, files:[sorted absolute paths]}",
        }),
        serde_json::json!({
            "name": "create_file",
            "category": "file_management",
            "description": "Create a new file on disk inside the workspace with optional initial content. Set track=true to immediately load the new file into memory. Fails if the file already exists.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Absolute or workspace-relative path of the new file."},
                {"name":"content","type":"string","required":false,"description":"Initial file content. Defaults to empty."},
                {"name":"track","type":"boolean","required":false,"description":"If true, track the file immediately after creation. Default false."}
            ],
            "returns": "\"ok: created <path>\" (with \"; tracking\" suffix when track=true) or \"error: …\"",
        }),
        serde_json::json!({
            "name": "delete_file",
            "category": "file_management",
            "description": "Delete a file from disk. If the file is currently tracked it is automatically untracked and unwatched first.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Absolute or workspace-relative path of the file to delete."}
            ],
            "returns": "\"ok: deleted <path>\" or \"error: …\"",
        }),
        serde_json::json!({
            "name": "load_file",
            "category": "inspection",
            "description": "Quick summary of a tracked file: line count and current CST version.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Path of the tracked file."}
            ],
            "returns": "\"ok: <path> — N lines, CST version V\" or \"error: …\"",
        }),
        serde_json::json!({
            "name": "get_tree_skeleton",
            "category": "inspection",
            "description": "List all line (CST node) IDs in a tracked file with text previews and byte spans. Use node_id values with get_node, edit_node, etc.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Path of the tracked file."}
            ],
            "returns": "JSON {version, nodes:[{node_id,kind,text_preview,span:{start,end}}]}",
        }),
        serde_json::json!({
            "name": "get_node",
            "category": "inspection",
            "description": "Get the full text, kind, and byte span of one line node. The returned version should be passed to edit_node as expected_version.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Path of the tracked file."},
                {"name":"node_id","type":"integer","required":true,"description":"0-based line index."}
            ],
            "returns": "JSON {node_id, kind, text, span:{start,end}, version}",
        }),
        serde_json::json!({
            "name": "get_line_tokens",
            "category": "inspection",
            "description": "Token-level breakdown of a single line. For .rs files: Keyword, Identifier, Literal, Comment, Whitespace, Punctuation, Newline. For plain files: Text.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Path of the tracked file."},
                {"name":"node_id","type":"integer","required":true,"description":"0-based line index."}
            ],
            "returns": "JSON {line_node_id, language, tokens:[{token_idx,kind,text,span}], version}",
        }),
        serde_json::json!({
            "name": "query_file",
            "category": "query",
            "description": "Search the CST of one tracked file using text, semantic, identifier, and scope-depth filters.",
            "parameters": [
                {"name":"path","type":"string","required":true,"description":"Path of the tracked file."},
                {"name":"query","type":"object","required":true,"description":"QueryExpr — see fields below."}
            ],
            "query_fields": {
                "kind": "Filter by token/line kind (case-insensitive). Line-depth: 'Line'. Token-depth (.rs): Keyword|Identifier|Literal|Comment|Whitespace|Punctuation|Newline.",
                "text_contains": "Substring match (case-sensitive).",
                "text_glob": "Glob pattern: * = any chars, ? = one char.",
                "node_id_from": "First line to consider (0-based, inclusive).",
                "node_id_to": "Last line to consider (0-based, inclusive).",
                "depth": "'line' (default) or 'token'. Overridden by semantic/identifier_name.",
                "semantic": "Named construct: fn_def|struct_def|enum_def|trait_def|impl_block|type_def|variable_def|use_stmt|macro_call. Returns capture (name).",
                "identifier_name": "Find every token-level occurrence of this exact identifier.",
                "scope_depth_min": "Only lines at brace depth >= N. 0=top-level.",
                "scope_depth_max": "Only lines at brace depth <= N."
            },
            "examples": [
                {"description":"All top-level functions","query":{"semantic":"fn_def","scope_depth_max":0}},
                {"description":"All uses of identifier 'conn'","query":{"identifier_name":"conn"}},
                {"description":"Lines with TODO","query":{"text_contains":"TODO"}},
                {"description":"All keyword tokens in first 10 lines","query":{"depth":"token","kind":"Keyword","node_id_to":9}}
            ],
            "returns": "JSON {file, language, version, match_count, matches:[{node_id,kind,text,span_start,span_end,scope_depth,token_idx?,capture?}]}",
        }),
        serde_json::json!({
            "name": "query_workspace",
            "category": "query",
            "description": "Run the same QueryExpr across all tracked files. Files with zero matches are omitted. The `graph` field is reserved for future cross-file relationship search (import graphs, call graphs, reference chains) — pass null for now.",
            "parameters": [
                {"name":"query","type":"object","required":true,"description":"Same QueryExpr as query_file."},
                {"name":"graph","type":"object|null","required":false,"description":"Reserved for future graph search. Pass null or omit."}
            ],
            "returns": "JSON {total_files_searched, files_with_matches, total_matches, results:[{file,language,version,match_count,matches}]}",
        }),
        serde_json::json!({
            "name": "edit_node",
            "category": "editing",
            "description": "Replace the text of one line node in a tracked file's CST. All other lines are preserved verbatim. Pass expected_version (from get_node or load_file) to detect conflicts.",
            "parameters": [
                {"name":"path","type":"string","required":true},
                {"name":"node_id","type":"integer","required":true,"description":"0-based line index."},
                {"name":"new_text","type":"string","required":true,"description":"Replacement text (trailing newline preserved from original)."},
                {"name":"expected_version","type":"integer","required":false,"description":"Guard against stale edits."}
            ],
            "returns": "\"ok: …\" | \"conflict: …\" | \"error: …\"",
        }),
        serde_json::json!({
            "name": "insert_lines",
            "category": "editing",
            "description": "Insert one or more new lines into the CST. insert_after=null prepends; insert_after=N inserts after line N. Trailing newline added automatically.",
            "parameters": [
                {"name":"path","type":"string","required":true},
                {"name":"insert_after","type":"integer|null","required":false,"description":"Insert after this line (null = prepend)."},
                {"name":"lines","type":"array of string","required":true,"description":"Lines to insert."},
                {"name":"expected_version","type":"integer","required":false}
            ],
            "returns": "JSON {inserted_count, first_node_id, version} or \"error: …\"",
        }),
        serde_json::json!({
            "name": "delete_lines",
            "category": "editing",
            "description": "Delete one or more consecutive line nodes from the CST.",
            "parameters": [
                {"name":"path","type":"string","required":true},
                {"name":"node_id","type":"integer","required":true,"description":"First line to delete."},
                {"name":"count","type":"integer","required":true,"description":"Number of lines to delete (>= 1)."},
                {"name":"expected_version","type":"integer","required":false}
            ],
            "returns": "\"ok: …\" | \"conflict: …\" | \"error: …\"",
        }),
        serde_json::json!({
            "name": "save_file",
            "category": "editing",
            "description": "Flush the in-memory CST for a tracked file to disk (lossless round-trip).",
            "parameters": [
                {"name":"path","type":"string","required":true}
            ],
            "returns": "\"ok: saved <path> (CST version V)\" or \"error: …\"",
        }),
        serde_json::json!({
            "name": "query_tool",
            "category": "help",
            "description": "Return documentation and tool-selection guidance. Omit tool_name for the full catalog. Provide tool_name for focused docs.",
            "parameters": [
                {"name":"tool_name","type":"string","required":false,"description":"Name of the tool to look up, or omit for full catalog."}
            ],
            "returns": "JSON catalog or single tool entry.",
        }),
    ]
}
