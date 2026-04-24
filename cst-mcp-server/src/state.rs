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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(path: &str, content: &str) -> (PathBuf, CstFile) {
        let p = PathBuf::from(path);
        let f = CstFile::parse(p.clone(), content);
        (p, f)
    }

    #[test]
    fn track_and_get() {
        let mut state = ServerState::new();
        let (path, file) = make_file("a.rs", "fn a() {}\n");
        state.track(path.clone(), file);
        assert!(state.get(&path).is_some());
    }

    #[test]
    fn untrack_returns_true_when_present() {
        let mut state = ServerState::new();
        let (path, file) = make_file("b.rs", "fn b() {}\n");
        state.track(path.clone(), file);
        assert!(state.untrack(&path));
        assert!(state.get(&path).is_none());
    }

    #[test]
    fn untrack_returns_false_when_absent() {
        let mut state = ServerState::new();
        let path = PathBuf::from("nonexistent.rs");
        assert!(!state.untrack(&path));
    }

    #[test]
    fn contains_reflects_tracking_state() {
        let mut state = ServerState::new();
        let (path, file) = make_file("c.rs", "fn c() {}\n");
        assert!(!state.contains(&path));
        state.track(path.clone(), file);
        assert!(state.contains(&path));
        state.untrack(&path);
        assert!(!state.contains(&path));
    }

    #[test]
    fn track_replaces_existing_entry() {
        let mut state = ServerState::new();
        let (path, file1) = make_file("d.rs", "v1\n");
        state.track(path.clone(), file1);

        let (_, mut file2) = make_file("d.rs", "v2\n");
        file2.version = 5;
        state.track(path.clone(), file2);

        let stored = state.get(&path).unwrap();
        assert_eq!(stored.version, 5);
        assert_eq!(stored.to_text(), "v2\n");
    }

    #[test]
    fn version_preserved_after_replace_node() {
        let mut state = ServerState::new();
        let (path, file) = make_file("e.rs", "line1\nline2\n");
        state.track(path.clone(), file);

        let updated = state.get(&path).unwrap().replace_node(0, "replaced").unwrap();
        assert_eq!(updated.version, 1);
        state.track(path.clone(), updated);

        assert_eq!(state.get(&path).unwrap().version, 1);
    }
}
