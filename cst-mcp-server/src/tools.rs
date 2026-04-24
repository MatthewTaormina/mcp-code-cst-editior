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
// Parameter structs
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TrackParams {
    pub path: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UntrackParams {
    pub path: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LoadParams {
    pub path: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetNodeParams {
    pub path: String,
    /// Node ID returned by get_tree_skeleton, get_children, or root_node_id.
    pub node_id: u64,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SkeletonParams {
    pub path: String,
    /// Start the skeleton from this node (omit for file root).
    pub node_id: Option<u64>,
    /// Maximum recursion depth (default 3).
    pub max_depth: Option<u32>,
    /// When true, omit anonymous punctuation/keyword nodes (default false).
    pub named_only: Option<bool>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetChildrenParams {
    pub path: String,
    pub node_id: u64,
    /// When true, return only named (semantic) children (default false).
    pub named_only: Option<bool>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EditNodeParams {
    pub path: String,
    pub node_id: u64,
    /// Replacement source text for the node's entire span.
    pub new_text: String,
    pub expected_version: Option<u64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertBeforeParams {
    pub path: String,
    pub node_id: u64,
    /// Text to insert immediately before the node.
    pub text: String,
    pub expected_version: Option<u64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertAfterParams {
    pub path: String,
    pub node_id: u64,
    /// Text to insert immediately after the node.
    pub text: String,
    pub expected_version: Option<u64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertIntoParams {
    pub path: String,
    pub node_id: u64,
    /// Text to insert inside the node.
    pub text: String,
    /// `"start"` to insert at the node's first byte, `"end"` for last byte (default `"end"`).
    pub position: Option<String>,
    pub expected_version: Option<u64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteNodeParams {
    pub path: String,
    pub node_id: u64,
    pub expected_version: Option<u64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListTrackedFilesParams {}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateFileParams {
    pub path: String,
    pub track: Option<bool>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteFileParams {
    pub path: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveParams {
    pub path: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryFileParams {
    pub path: String,
    /// A tree-sitter s-expression pattern with captures.
    /// Example: `"(function_item name: (identifier) @fn_name)"`
    pub ts_query: String,
    /// Maximum number of capture matches to return (omit for no limit).
    pub max_matches: Option<usize>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryWorkspaceParams {
    /// Same tree-sitter s-expression query as query_file.
    pub ts_query: String,
    /// Cap on total matches returned across all files.
    pub max_matches: Option<usize>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryToolParams {
    pub tool_name: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetLinesParams {
    pub path: String,
    /// First line to return, 1-based (default 1).
    pub start: Option<usize>,
    /// Last line to return, 1-based inclusive (default: last line).
    pub end: Option<usize>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EditLineParams {
    pub path: String,
    /// 1-based line number to replace.
    pub line: usize,
    /// Replacement text for the line (no trailing newline needed).
    pub new_text: String,
    pub expected_version: Option<u64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertLineParams {
    pub path: String,
    /// 1-based reference line number.
    pub line: usize,
    /// Text for the new line.
    pub text: String,
    /// Insert after the reference line (`true`) or before it (`false`, default).
    pub after: Option<bool>,
    pub expected_version: Option<u64>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteLineParams {
    pub path: String,
    /// 1-based line number to remove.
    pub line: usize,
    pub expected_version: Option<u64>,
}
// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------
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
        Self { state, watcher, access }
    }
}
// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------
#[tool_router(server_handler)]
impl CstMcpServer {
    // --- Tracking ---
    #[tool(description = "Track a file: read it from disk, parse it into a tree-sitter CST, and watch for external changes.")]
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
                let lang = format!("{:?}", file.language());
                let root_id = file.root_node_id();
                self.state.write().await.track(path.clone(), file);
                if let Err(e) = watch_path(&self.watcher, &path) {
                    eprintln!("watcher: could not watch {path:?}: {e}");
                }
                match root_id {
                    Some(id) => format!("ok: tracking {path:?} ({lang}, root node_id={id})"),
                    None => format!("ok: tracking {path:?} ({lang}, no parse tree)"),
                }
            }
        }
    }
    #[tool(description = "Untrack a file: remove it from memory and stop watching for changes.")]
    async fn untrack_file(
        &self,
        Parameters(UntrackParams { path }): Parameters<UntrackParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("untrack", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        if self.state.write().await.untrack(&path) {
            if let Err(e) = unwatch_path(&self.watcher, &path) {
                eprintln!("watcher: could not unwatch {path:?}: {e}");
            }
            format!("ok: untracked {path:?}")
        } else {
            format!("error: {path:?} was not being tracked")
        }
    }
    #[tool(description = "Quick summary of a tracked file: language, source size, root node, and version.")]
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
                let line_count = file.to_text().lines().count();
                let lang = format!("{:?}", file.language());
                match file.root_node_id() {
                    None => format!(
                        "ok: {path:?} — {lang}, {line_count} lines, no parse tree, version {}",
                        file.version
                    ),
                    Some(root_id) => {
                        let children = file.get_children(root_id, true)
                            .map(|c| c.len())
                            .unwrap_or(0);
                        format!(
                            "ok: {path:?} — {lang}, {line_count} lines, root={root_id} ({children} named children), version {}",
                            file.version
                        )
                    }
                }
            }
        }
    }
    #[tool(description = "List all files currently tracked in memory. Returns JSON: {count, files}.")]
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
        serde_json::json!({ "count": files.len(), "files": files }).to_string()
    }
    // --- File management ---
    #[tool(description = "Create a new empty file on disk. Set track=true to immediately load it. Returns ok or error.")]
    async fn create_file(
        &self,
        Parameters(CreateFileParams { path, track }): Parameters<CreateFileParams>,
    ) -> String {
        let resolved = match self.access.resolve_and_check("create", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        if resolved.exists() {
            return format!("error: {resolved:?} already exists");
        }
        if let Some(parent) = resolved.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return format!("error: could not create directories for {resolved:?}: {e}");
                }
            }
        }
        if let Err(e) = std::fs::write(&resolved, b"") {
            return format!("error: could not write {resolved:?}: {e}");
        }
        let mut msg = format!("ok: created {resolved:?}");
        if track.unwrap_or(false) {
            match std::fs::read_to_string(&resolved) {
                Err(e) => msg.push_str(&format!("; warning: track failed: {e}")),
                Ok(text) => {
                    let file = CstFile::parse(resolved.clone(), &text);
                    self.state.write().await.track(resolved.clone(), file);
                    if let Err(e) = watch_path(&self.watcher, &resolved) {
                        eprintln!("watcher: {e}");
                    }
                    msg.push_str("; tracking");
                }
            }
        }
        msg
    }
    #[tool(description = "Delete a file from disk. Auto-untracks if currently tracked. Returns ok or error.")]
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
        let was_tracked = self.state.write().await.untrack(&resolved);
        if was_tracked {
            if let Err(e) = unwatch_path(&self.watcher, &resolved) {
                eprintln!("watcher: {e}");
            }
        }
        match std::fs::remove_file(&resolved) {
            Ok(()) => format!("ok: deleted {resolved:?}"),
            Err(e) => format!("error: could not delete {resolved:?}: {e}"),
        }
    }
    #[tool(description = "Flush the in-memory CST back to disk (lossless). Returns ok or error.")]
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
                let text = file.to_text().to_string();
                let version = file.version;
                drop(state);
                match std::fs::write(&path, text.as_bytes()) {
                    Ok(()) => format!("ok: saved {path:?} (CST version {version})"),
                    Err(e) => format!("error: could not write {path:?}: {e}"),
                }
            }
        }
    }
    // --- Inspection ---
    /// Return rich metadata for a single tree-sitter node identified by its ID.
    ///
    /// Node IDs come from `get_tree_skeleton`, `get_children`, or the root_node_id
    /// shown in `track_file` / `load_file` responses.  IDs are valid only for the
    /// current version — re-query after any edit.
    #[tool(
        description = "Get metadata for one CST node: kind, text_preview, row/col, byte offsets, \
                        child count. Returns JSON. node_id comes from get_tree_skeleton or get_children."
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
                Ok(info) => serde_json::json!({
                    "node_id": info.node_id,
                    "kind": info.kind,
                    "text_preview": info.text_preview,
                    "start_row": info.start_row,
                    "start_col": info.start_col,
                    "end_row": info.end_row,
                    "end_col": info.end_col,
                    "start_byte": info.start_byte,
                    "end_byte": info.end_byte,
                    "is_named": info.is_named,
                    "has_error": info.has_error,
                    "named_child_count": info.named_child_count,
                    "version": file.version,
                }).to_string(),
            },
        }
    }
    /// Return a hierarchical JSON view of the parse tree starting from a
    /// given node (or the file root).
    ///
    /// Use this to understand the file's structure before navigating to a
    /// specific node for editing.  Increase `max_depth` (default 3) to see
    /// deeper subtrees.  Set `named_only=true` to hide punctuation tokens and
    /// get a cleaner view.
    #[tool(
        description = "Hierarchical JSON view of the parse tree. \
                        Omit node_id to start from the file root. \
                        Returns {node_id, kind, text_preview, children:[…], version}."
    )]
    async fn get_tree_skeleton(
        &self,
        Parameters(SkeletonParams { path, node_id, max_depth, named_only }): Parameters<SkeletonParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("read", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;
        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => {
                let only_named = named_only.unwrap_or(false);
                match file.get_tree_skeleton(node_id, max_depth, only_named) {
                    Err(e) => format!("error: {e}"),
                    Ok(mut tree) => {
                        tree["version"] = serde_json::json!(file.version);
                        tree.to_string()
                    }
                }
            }
        }
    }
    /// Return the direct children of a node, with field names and previews.
    ///
    /// Field names (like `"name"`, `"body"`, `"parameters"`) indicate how a
    /// child relates to its parent in the grammar.  Use the returned `node_id`
    /// values with `get_node`, `edit_node`, `insert_before`, etc.
    #[tool(
        description = "List the direct children of a CST node with field names and previews. \
                        Returns JSON {children:[{node_id, kind, field_name, text_preview, …}], version}."
    )]
    async fn get_children(
        &self,
        Parameters(GetChildrenParams { path, node_id, named_only }): Parameters<GetChildrenParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("read", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;
        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => {
                let only_named = named_only.unwrap_or(false);
                match file.get_children(node_id, only_named) {
                    Err(e) => format!("error: {e}"),
                    Ok(children) => {
                        let arr: Vec<_> = children.iter().map(|c| serde_json::json!({
                            "node_id": c.node_id,
                            "kind": c.kind,
                            "field_name": c.field_name,
                            "text_preview": c.text_preview,
                            "start_row": c.start_row,
                            "start_col": c.start_col,
                            "named_child_count": c.named_child_count,
                            "has_error": c.has_error,
                        })).collect();
                        serde_json::json!({
                            "node_id": node_id,
                            "child_count": arr.len(),
                            "children": arr,
                            "version": file.version,
                        }).to_string()
                    }
                }
            }
        }
    }
    // --- Editing ---
    /// Replace the entire source span of a CST node with new text.
    ///
    /// The file is re-parsed after the edit.  All node IDs in the response
    /// are stale — re-query with `get_tree_skeleton` or `get_children`.
    ///
    /// Pass `expected_version` (from a prior `get_node` or `load_file`) to
    /// detect conflicts caused by concurrent watcher reloads.
    #[tool(
        description = "Replace the source span of any CST node with new_text. Re-parses the file. \
                        Node IDs are stale after this call — re-query. \
                        Returns JSON {version, has_errors, errors:[…]} or conflict/error string."
    )]
    async fn edit_node(
        &self,
        Parameters(EditNodeParams { path, node_id, new_text, expected_version }): Parameters<EditNodeParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.replace_node(node_id, &new_text, expected_version),
            }
        };
        self.apply_edit(path, new_file).await
    }
    /// Insert text immediately before a CST node.
    #[tool(
        description = "Insert text immediately before a node. Re-parses the file. \
                        Returns JSON {version, has_errors, errors:[…]} or conflict/error string."
    )]
    async fn insert_before(
        &self,
        Parameters(InsertBeforeParams { path, node_id, text, expected_version }): Parameters<InsertBeforeParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.insert_before_node(node_id, &text, expected_version),
            }
        };
        self.apply_edit(path, new_file).await
    }
    /// Insert text immediately after a CST node.
    #[tool(
        description = "Insert text immediately after a node. Re-parses the file. \
                        Returns JSON {version, has_errors, errors:[…]} or conflict/error string."
    )]
    async fn insert_after(
        &self,
        Parameters(InsertAfterParams { path, node_id, text, expected_version }): Parameters<InsertAfterParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.insert_after_node(node_id, &text, expected_version),
            }
        };
        self.apply_edit(path, new_file).await
    }
    /// Insert text inside a node — at its start or end.
    ///
    /// Use `position="end"` (default) to append at the end of a block body,
    /// or `position="start"` to prepend.  For inserting inside a function body
    /// without disturbing the braces, target the block node.
    #[tool(
        description = "Insert text inside a node at its start or end (default end). \
                        Useful for adding statements to a block body. \
                        Returns JSON {version, has_errors, errors:[…]} or conflict/error string."
    )]
    async fn insert_into(
        &self,
        Parameters(InsertIntoParams { path, node_id, text, position, expected_version }): Parameters<InsertIntoParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let at_start = position.as_deref() == Some("start");
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.insert_into_node(node_id, &text, at_start, expected_version),
            }
        };
        self.apply_edit(path, new_file).await
    }
    /// Delete a CST node's source span entirely.
    #[tool(
        description = "Delete the source span of a CST node. Re-parses the file. \
                        Returns JSON {version, has_errors, errors:[…]} or conflict/error string."
    )]
    async fn delete_node(
        &self,
        Parameters(DeleteNodeParams { path, node_id, expected_version }): Parameters<DeleteNodeParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.delete_node(node_id, expected_version),
            }
        };
        self.apply_edit(path, new_file).await
    }
    // --- Query ---
    /// Run a tree-sitter s-expression query against a tracked file and return
    /// all capture matches.
    ///
    /// ## Query syntax
    ///
    /// Uses tree-sitter's standard s-expression query language.  Every
    /// capture must be named with `@name`.
    ///
    /// ```
    /// # Rust: find all function names
    /// (function_item name: (identifier) @fn_name)
    ///
    /// # JavaScript: find all variable declarators
    /// (variable_declarator name: (identifier) @var_name)
    ///
    /// # CSS: find all class selectors
    /// (class_selector (class_name) @class)
    ///
    /// # HTML: find all element tags
    /// (element (start_tag (tag_name) @tag))
    /// ```
    ///
    /// Use `get_tree_skeleton` to explore the node kinds available in the
    /// file before writing a query.
    #[tool(
        description = "Run a tree-sitter s-expression query on a tracked file and return captures. \
                        Example: `(function_item name: (identifier) @fn_name)` \
                        Returns JSON {file, language, version, match_count, matches:[{capture_name, node_id, kind, text_preview, start_row, start_col, end_row, end_col}]}."
    )]
    async fn query_file(
        &self,
        Parameters(QueryFileParams { path, ts_query, max_matches }): Parameters<QueryFileParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("query", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;
        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => match file.query_ts(&ts_query, max_matches) {
                Err(e) => format!("error: {e}"),
                Ok(matches) => {
                    let match_count = matches.len();
                    let match_values: Vec<_> = matches.iter().map(|m| serde_json::json!({
                        "capture_name": m.capture_name,
                        "node_id": m.node_id,
                        "kind": m.kind,
                        "text_preview": m.text_preview,
                        "start_row": m.start_row,
                        "start_col": m.start_col,
                        "end_row": m.end_row,
                        "end_col": m.end_col,
                    })).collect();
                    serde_json::json!({
                        "file": path.to_string_lossy(),
                        "language": format!("{:?}", file.language()),
                        "version": file.version,
                        "match_count": match_count,
                        "matches": match_values,
                    }).to_string()
                }
            }
        }
    }
    /// Run the same tree-sitter query across every tracked file and aggregate
    /// results.  Files without a parse tree (plain text) or with a grammar
    /// mismatch are silently skipped.
    #[tool(
        description = "Run a tree-sitter query across all tracked files. \
                        Same query syntax as query_file. \
                        Returns JSON {total_files_searched, files_with_matches, total_matches, results:[…]}."
    )]
    async fn query_workspace(
        &self,
        Parameters(QueryWorkspaceParams { ts_query, max_matches }): Parameters<QueryWorkspaceParams>,
    ) -> String {
        let state = self.state.read().await;
        let paths = state.tracked_paths();
        let total_files = paths.len();
        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut total_matches: usize = 0;
        let mut remaining = max_matches;
        'files: for path in paths {
            if let Some(file) = state.get(&path) {
                let limit = remaining;
                let matches = match file.query_ts(&ts_query, limit) {
                    Ok(m) if !m.is_empty() => m,
                    _ => continue,
                };
                if let Some(r) = remaining.as_mut() {
                    if *r <= matches.len() {
                        *r = 0;
                    } else {
                        *r -= matches.len();
                    }
                }
                total_matches += matches.len();
                let mc = matches.len();
                let match_values: Vec<_> = matches.iter().map(|m| serde_json::json!({
                    "capture_name": m.capture_name,
                    "node_id": m.node_id,
                    "kind": m.kind,
                    "text_preview": m.text_preview,
                    "start_row": m.start_row,
                    "start_col": m.start_col,
                    "end_row": m.end_row,
                    "end_col": m.end_col,
                })).collect();
                results.push(serde_json::json!({
                    "file": path.to_string_lossy(),
                    "language": format!("{:?}", file.language()),
                    "version": file.version,
                    "match_count": mc,
                    "matches": match_values,
                }));
                if remaining == Some(0) {
                    break 'files;
                }
            }
        }
        serde_json::json!({
            "total_files_searched": total_files,
            "files_with_matches": results.len(),
            "total_matches": total_matches,
            "results": results,
        }).to_string()
    }
    // --- Plain-text line tools ---
    /// Read lines from any tracked file with 1-based line numbers.
    ///
    /// Works for all file types but is the primary navigation tool for
    /// `Plain` files (`.txt`, `.env`, `.toml`, etc.) that have no parse tree.
    #[tool(
        description = "Read lines from a tracked file with 1-based line numbers. \
                        Provide start/end to slice; omit both for the whole file. \
                        Returns JSON {total_lines, start, end, lines:[{line, text}]}. \
                        Use for plain-text files that have no CST."
    )]
    async fn get_lines(
        &self,
        Parameters(GetLinesParams { path, start, end }): Parameters<GetLinesParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("read", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let state = self.state.read().await;
        match state.get(&path) {
            None => format!("error: {path:?} is not tracked — call track_file first"),
            Some(file) => {
                let mut result = file.get_lines(start, end);
                result["version"] = serde_json::json!(file.version);
                result["language"] = serde_json::json!(format!("{:?}", file.language()));
                result.to_string()
            }
        }
    }

    /// Replace the content of a single line in a plain-text file.
    #[tool(
        description = "Replace a single line (1-based) in a plain-text file. \
                        Only valid for Plain files — use edit_node for JSON, Markdown, and code files. \
                        Returns JSON {version} or an error string."
    )]
    async fn edit_line(
        &self,
        Parameters(EditLineParams { path, line, new_text, expected_version }): Parameters<EditLineParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.edit_line(line, &new_text, expected_version),
            }
        };
        self.apply_plain_edit(path, new_file).await
    }

    /// Insert a new line before or after a reference line in a plain-text file.
    #[tool(
        description = "Insert a new line before (default) or after a reference line (1-based) in a plain-text file. \
                        Only valid for Plain files — use insert_before/insert_after for parsed languages. \
                        Returns JSON {version} or an error string."
    )]
    async fn insert_line(
        &self,
        Parameters(InsertLineParams { path, line, text, after, expected_version }): Parameters<InsertLineParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.insert_line(line, &text, after.unwrap_or(false), expected_version),
            }
        };
        self.apply_plain_edit(path, new_file).await
    }

    /// Delete a single line from a plain-text file.
    #[tool(
        description = "Delete a line (1-based) from a plain-text file. \
                        Only valid for Plain files — use delete_node for parsed languages. \
                        Returns JSON {version} or an error string."
    )]
    async fn delete_line(
        &self,
        Parameters(DeleteLineParams { path, line, expected_version }): Parameters<DeleteLineParams>,
    ) -> String {
        let path = match self.access.resolve_and_check("edit", &path) {
            Err(e) => return format!("error: {e}"),
            Ok(p) => p,
        };
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.delete_line(line, expected_version),
            }
        };
        self.apply_plain_edit(path, new_file).await
    }

    // --- Help ---
    #[tool(
        description = "Get documentation and selection guidance. Omit tool_name for the full \
                        catalog. Provide tool_name for focused docs on one tool."
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
            }).to_string(),
            Some(name) => match catalog.iter().find(|t| t["name"] == name) {
                Some(entry) => entry.to_string(),
                None => {
                    let names: Vec<_> = catalog.iter()
                        .filter_map(|t| t["name"].as_str())
                        .collect();
                    format!("error: unknown tool {:?}. Available: {}", name, names.join(", "))
                }
            },
        }
    }
}
// ---------------------------------------------------------------------------
// Helper — apply an edit result and store the new CstFile
// ---------------------------------------------------------------------------
impl CstMcpServer {
    async fn apply_edit(
        &self,
        path: std::path::PathBuf,
        result: anyhow::Result<CstFile>,
    ) -> String {
        match result {
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("conflict") {
                    msg
                } else {
                    format!("error: {msg}")
                }
            }
            Ok(new_file) => {
                let version = new_file.version;
                let errors = new_file.get_errors();
                let has_errors = !errors.is_empty();
                self.state.write().await.track(path, new_file);
                serde_json::json!({
                    "version": version,
                    "has_errors": has_errors,
                    "errors": errors,
                }).to_string()
            }
        }
    }

    /// Plain-text edits don't have CST errors — just store the new file.
    async fn apply_plain_edit(
        &self,
        path: std::path::PathBuf,
        result: anyhow::Result<CstFile>,
    ) -> String {
        match result {
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("conflict") { msg } else { format!("error: {msg}") }
            }
            Ok(new_file) => {
                let version = new_file.version;
                self.state.write().await.track(path, new_file);
                serde_json::json!({ "version": version }).to_string()
            }
        }
    }
}
// ---------------------------------------------------------------------------
// Static tool catalog used by query_tool
// ---------------------------------------------------------------------------
const SELECTION_GUIDE: &str = "\
TRACKING — before reading or editing a file you must track it:\
\n  1. track_file        → loads file into memory, parses CST, starts watcher\
\n  2. list_tracked_files → see what is already tracked\
\n  3. untrack_file      → release a file from memory\
\
\nFILE MANAGEMENT — create or delete files on disk:\
\n  4. create_file       → create file (set track=true to load immediately)\
\n  5. delete_file       → delete file (auto-untracks if tracked)\
\
\nINSPECTION — explore a tracked file's CST:\
\n  6. load_file         → quick summary: language, line count, root node, version\
\n  7. get_tree_skeleton → hierarchical JSON view of the parse tree (start at root or node_id)\
\n  8. get_node          → metadata for one node: kind, row/col, byte offsets, child count\
\n  9. get_children      → direct children of a node with field names (name, body, params…)\
\n 10. get_lines         → read lines with 1-based numbers (all files; required for Plain)\
\
\nQUERY — find nodes using tree-sitter s-expression syntax:\
\n 11. query_file        → pattern match in one file, returns captured nodes\
\n 12. query_workspace   → same query across all tracked files\
\n     Example: (function_item name: (identifier) @fn_name)\
\
\nEDITING — CST node edits (Rust/JS/TS/CSS/HTML/JSON/Markdown; always pass expected_version):\
\n 13. edit_node         → replace a node's entire source span\
\n 14. insert_before     → insert text immediately before a node\
\n 15. insert_after      → insert text immediately after a node\
\n 16. insert_into       → insert text inside a node (at start or end of its span)\
\n 17. delete_node       → delete a node's source span entirely\
\n 18. save_file         → flush in-memory CST back to disk\
\
\nPLAIN-TEXT LINE EDITS (only for Plain files — .txt, .env, etc.):\
\n 19. edit_line         → replace a single line (1-based)\
\n 20. insert_line       → insert a new line before/after a reference line\
\n 21. delete_line       → delete a line\
\
\nPARSED LANGUAGES: Rust .rs | JS .js/.jsx | TS .ts | TSX .tsx\
\n  CSS .css/.scss | HTML .html | JSON .json/.jsonc | Markdown .md/.markdown\
\n  Everything else is Plain — use line tools.\
\
\nHELP:\
\n 22. query_tool        → this tool; docs for any tool or the full catalog\
\
\nTYPICAL WORKFLOW (parsed file):\
\n  track_file → get_tree_skeleton → get_children → edit_node/insert_into → save_file\
\
\nTYPICAL WORKFLOW (plain text):\
\n  track_file → get_lines → edit_line/insert_line/delete_line → save_file\
\
\nAFTER EVERY CST EDIT: Node IDs are stale. Re-query with get_tree_skeleton or get_children.";
fn tool_catalog() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"name":"track_file","category":"tracking","description":"Load file into memory as a tree-sitter CST and watch for external changes."}),
        serde_json::json!({"name":"untrack_file","category":"tracking","description":"Remove file from memory and stop watching."}),
        serde_json::json!({"name":"list_tracked_files","category":"tracking","description":"List all tracked files. Returns JSON {count, files}."}),
        serde_json::json!({"name":"create_file","category":"file_management","description":"Create a new empty file on disk. set track=true to load immediately.","params":["path","track?"]}),
        serde_json::json!({"name":"delete_file","category":"file_management","description":"Delete a file from disk (auto-untracks).","params":["path"]}),
        serde_json::json!({"name":"load_file","category":"inspection","description":"Quick summary: language, line count, root node, version.","params":["path"]}),
        serde_json::json!({"name":"get_tree_skeleton","category":"inspection","description":"Hierarchical JSON of the parse tree. Omit node_id for file root. named_only=true hides punctuation.","params":["path","node_id?","max_depth?","named_only?"]}),
        serde_json::json!({"name":"get_node","category":"inspection","description":"Metadata for one node: kind, text_preview, row/col, byte offsets, child count, version.","params":["path","node_id"]}),
        serde_json::json!({"name":"get_children","category":"inspection","description":"Direct children of a node with field names (name, body, params…). named_only=true omits punctuation.","params":["path","node_id","named_only?"]}),
        serde_json::json!({"name":"query_file","category":"query","description":"tree-sitter s-expression query in one file. Returns captured nodes.","params":["path","ts_query","max_matches?"],"example":"(function_item name: (identifier) @fn_name)"}),
        serde_json::json!({"name":"query_workspace","category":"query","description":"Same tree-sitter query across all tracked files.","params":["ts_query","max_matches?"]}),
        serde_json::json!({"name":"edit_node","category":"editing","description":"Replace a node's entire source span with new_text. Re-parses file. IDs stale after this.","params":["path","node_id","new_text","expected_version?"]}),
        serde_json::json!({"name":"insert_before","category":"editing","description":"Insert text immediately before a node.","params":["path","node_id","text","expected_version?"]}),
        serde_json::json!({"name":"insert_after","category":"editing","description":"Insert text immediately after a node.","params":["path","node_id","text","expected_version?"]}),
        serde_json::json!({"name":"insert_into","category":"editing","description":"Insert text inside a node at its start or end (position='start'|'end', default 'end'). Good for adding statements to a block body.","params":["path","node_id","text","position?","expected_version?"]}),
        serde_json::json!({"name":"delete_node","category":"editing","description":"Delete a node's entire source span.","params":["path","node_id","expected_version?"]}),
        serde_json::json!({"name":"save_file","category":"editing","description":"Flush in-memory CST to disk.","params":["path"]}),
        serde_json::json!({"name":"get_lines","category":"plain_text","description":"Read lines with 1-based numbers. Works for all files; required for Plain files with no CST.","params":["path","start?","end?"]}),
        serde_json::json!({"name":"edit_line","category":"plain_text","description":"Replace a single line (1-based) in a Plain file.","params":["path","line","new_text","expected_version?"]}),
        serde_json::json!({"name":"insert_line","category":"plain_text","description":"Insert a new line before (default) or after a reference line in a Plain file.","params":["path","line","text","after?","expected_version?"]}),
        serde_json::json!({"name":"delete_line","category":"plain_text","description":"Delete a line (1-based) from a Plain file.","params":["path","line","expected_version?"]}),
        serde_json::json!({"name":"query_tool","category":"help","description":"Get docs for any tool or the full catalog with selection guide.","params":["tool_name?"]}),
    ]
}
