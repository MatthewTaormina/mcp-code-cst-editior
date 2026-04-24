use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    handler::server::wrapper::Parameters, schemars, tool, tool_router,
};
use serde::Deserialize;
use tokio::sync::RwLock;

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
}

/// Parameters for `save_file`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveParams {
    /// Absolute path of the tracked file to flush to disk.
    pub path: String,
}

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

/// The MCP server handler that exposes CST editing capabilities as tools.
#[derive(Clone)]
pub struct CstMcpServer {
    state: Arc<RwLock<ServerState>>,
    watcher: WatcherHandle,
}

impl CstMcpServer {
    pub fn new(state: Arc<RwLock<ServerState>>, watcher: WatcherHandle) -> Self {
        Self { state, watcher }
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
        let path = PathBuf::from(&path);

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
        let path = PathBuf::from(&path);
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
        let path = PathBuf::from(&path);
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

    /// Replace the content of one line (identified by its 0-based `node_id`)
    /// inside a tracked file's in-memory CST.
    ///
    /// All other lines — including their whitespace and comments — are
    /// preserved verbatim (lossless round-trip via rowan).
    #[tool(
        description = "Edit a single line node in the CST of a tracked file. All other lines are preserved verbatim."
    )]
    async fn edit_node(
        &self,
        Parameters(EditParams {
            path,
            node_id,
            new_text,
        }): Parameters<EditParams>,
    ) -> String {
        let path = PathBuf::from(&path);

        // Build the new CST outside of the write lock to minimise hold time.
        let new_file = {
            let state = self.state.read().await;
            match state.get(&path) {
                None => return format!("error: {path:?} is not tracked — call track_file first"),
                Some(file) => file.replace_node(node_id, &new_text),
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

    /// Flush the current in-memory CST for a tracked file back to disk.
    ///
    /// The file is reconstructed from the rowan tree, guaranteeing a lossless
    /// round-trip for all unedited content.
    #[tool(description = "Save the in-memory CST of a tracked file to disk (lossless round-trip).")]
    async fn save_file(
        &self,
        Parameters(SaveParams { path }): Parameters<SaveParams>,
    ) -> String {
        let path = PathBuf::from(&path);
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
}
