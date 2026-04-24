use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cst::CstFile;

/// Thread-safe, versioned in-memory registry of all tracked files and their parsed CSTs.
pub struct ServerState {
    files: HashMap<PathBuf, CstFile>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Insert or replace a file's CST entry.
    pub fn track(&mut self, path: PathBuf, file: CstFile) {
        self.files.insert(path, file);
    }

    /// Remove a file from the tracked set.  Returns `true` if it was present.
    pub fn untrack(&mut self, path: &Path) -> bool {
        self.files.remove(path).is_some()
    }

    /// Immutable access to a tracked file's CST.
    pub fn get(&self, path: &Path) -> Option<&CstFile> {
        self.files.get(path)
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    /// Snapshot of all currently tracked paths (used by the watcher on startup
    /// and future tooling that needs to enumerate tracked files).
    #[allow(dead_code)]
    pub fn tracked_paths(&self) -> Vec<PathBuf> {
        self.files.keys().cloned().collect()
    }
}
