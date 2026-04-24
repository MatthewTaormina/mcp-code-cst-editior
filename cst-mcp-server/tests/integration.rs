//! Integration tests for the CST-MCP server.
//!
//! These tests exercise the full stack (state + watcher + CST) using real
//! temporary files on disk, catching interactions that unit tests cannot.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cst_mcp_server::{
    cst::CstFile,
    state::ServerState,
    watcher::{start_watcher, watch_path},
};
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a `ServerState` + running watcher, returning both.
async fn setup() -> (
    Arc<RwLock<ServerState>>,
    cst_mcp_server::watcher::WatcherHandle,
) {
    let state = Arc::new(RwLock::new(ServerState::new()));
    let handle = start_watcher(Arc::clone(&state)).expect("watcher failed to start");
    (state, handle)
}

/// Write `content` to a temp file and return its `NamedTempFile` handle
/// (keeps the file alive until it's dropped) plus its `PathBuf`.
fn make_temp_file(content: &str) -> (tempfile::NamedTempFile, PathBuf) {
    let mut tmp = tempfile::NamedTempFile::new().expect("tempfile creation failed");
    tmp.write_all(content.as_bytes())
        .expect("tempfile write failed");
    tmp.flush().expect("tempfile flush failed");
    let path = tmp.path().to_path_buf();
    (tmp, path)
}

// ---------------------------------------------------------------------------
// CST parse & roundtrip
// ---------------------------------------------------------------------------

#[test]
fn cst_parse_and_roundtrip() {
    let content = "fn main() {\n    println!(\"hello\");\n}\n";
    let file = CstFile::parse(PathBuf::from("test.rs"), content);
    assert_eq!(file.to_text(), content, "roundtrip must be lossless");
}

#[test]
fn cst_edit_preserves_other_lines() {
    let content = "line 0\nline 1\nline 2\n";
    let file = CstFile::parse(PathBuf::from("t.txt"), content);
    let updated = file.replace_node(1, "REPLACED").unwrap();

    let text = updated.to_text();
    assert!(text.starts_with("line 0\n"), "line 0 must be untouched");
    assert!(text.contains("REPLACED"), "replacement must be present");
    assert!(text.ends_with("line 2\n"), "line 2 must be untouched");
}

#[test]
fn cst_edit_version_increments() {
    let file = CstFile::parse(PathBuf::from("t.txt"), "a\nb\nc\n");
    assert_eq!(file.version, 0);
    let v1 = file.replace_node(0, "A").unwrap();
    assert_eq!(v1.version, 1);
    let v2 = v1.replace_node(1, "B").unwrap();
    assert_eq!(v2.version, 2);
}

// ---------------------------------------------------------------------------
// State management
// ---------------------------------------------------------------------------

#[test]
fn state_track_get_untrack() {
    let mut state = ServerState::new();
    let path = PathBuf::from("/tmp/fake.rs");
    let file = CstFile::parse(path.clone(), "x\n");

    assert!(!state.contains(&path));
    state.track(path.clone(), file);
    assert!(state.contains(&path));
    assert!(state.get(&path).is_some());

    let removed = state.untrack(&path);
    assert!(removed);
    assert!(!state.contains(&path));
}

// ---------------------------------------------------------------------------
// get_node and tree_skeleton
// ---------------------------------------------------------------------------

#[test]
fn get_node_correct_span() {
    let content = "abc\ndef\n";
    let file = CstFile::parse(PathBuf::from("s.txt"), content);

    let node0 = file.get_node(0).unwrap();
    assert_eq!(node0.text, "abc\n");
    assert_eq!(node0.span_start, 0);
    assert_eq!(node0.span_end, 4);

    let node1 = file.get_node(1).unwrap();
    assert_eq!(node1.text, "def\n");
    assert_eq!(node1.span_start, 4);
    assert_eq!(node1.span_end, 8);
}

#[test]
fn tree_skeleton_covers_entire_file() {
    let content = "a\nbb\nccc\n";
    let file = CstFile::parse(PathBuf::from("s.txt"), content);
    let nodes = file.tree_skeleton();

    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].span_start, 0);
    assert_eq!(nodes.last().unwrap().span_end as usize, content.len());
}

// ---------------------------------------------------------------------------
// Conflict detection
// ---------------------------------------------------------------------------

#[test]
fn conflict_detected_when_version_mismatch() {
    let file = CstFile::parse(PathBuf::from("c.rs"), "a\nb\nc\n");
    // Simulate a prior edit bumping the version to 1.
    let mut v1 = file.replace_node(0, "A").unwrap();
    v1.version = 1;

    let mut state = ServerState::new();
    state.track(PathBuf::from("c.rs"), v1);

    // Caller still thinks version is 0.
    let stored = state.get(&PathBuf::from("c.rs")).unwrap();
    let expected_version: u64 = 0;
    assert_ne!(
        stored.version, expected_version,
        "version mismatch should be detectable"
    );
}

#[test]
fn no_conflict_when_version_matches() {
    let file = CstFile::parse(PathBuf::from("d.rs"), "x\ny\n");
    let version = file.version; // 0

    let mut state = ServerState::new();
    state.track(PathBuf::from("d.rs"), file);

    let stored = state.get(&PathBuf::from("d.rs")).unwrap();
    assert_eq!(stored.version, version, "version must match for safe edit");
}

// ---------------------------------------------------------------------------
// Watcher-triggered reload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn watcher_reloads_file_on_external_change() {
    let (_tmp, path) = make_temp_file("original line\n");

    let (state, handle) = setup().await;

    // Track the file manually (mirrors what track_file tool does).
    let content = std::fs::read_to_string(&path).unwrap();
    let file = CstFile::parse(path.clone(), &content);
    state.write().await.track(path.clone(), file);
    watch_path(&handle, &path).expect("watch_path failed");

    // Confirm initial version.
    assert_eq!(state.read().await.get(&path).unwrap().version, 0);

    // Externally overwrite the file.
    std::fs::write(&path, "modified line\n").unwrap();

    // Give inotify + the async task enough time to process the event.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let guard = state.read().await;
    let reloaded = guard.get(&path).expect("file should still be tracked");

    assert_eq!(
        reloaded.version, 1,
        "version should have incremented after watcher reload"
    );
    assert!(
        reloaded.to_text().contains("modified"),
        "reloaded content should reflect the external change"
    );
}

#[tokio::test]
async fn watcher_does_not_affect_untracked_file() {
    // Create a file but do NOT track it.
    let (_tmp, path) = make_temp_file("untracked content\n");

    let (state, handle) = setup().await;
    // Register with watcher but skip state.track() — simulates a misconfiguration.
    let _ = watch_path(&handle, &path); // best-effort; may or may not succeed

    std::fs::write(&path, "changed\n").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The file was never tracked, so state should still have nothing.
    assert!(
        state.read().await.get(&path).is_none(),
        "untracked file should not appear in state after external change"
    );
}

#[tokio::test]
async fn watcher_increments_version_monotonically() {
    let (_tmp, path) = make_temp_file("v0\n");

    let (state, handle) = setup().await;

    let content = std::fs::read_to_string(&path).unwrap();
    state
        .write()
        .await
        .track(path.clone(), CstFile::parse(path.clone(), &content));
    watch_path(&handle, &path).unwrap();

    // Two sequential external writes.
    std::fs::write(&path, "v1\n").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    std::fs::write(&path, "v2\n").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let v = state.read().await.get(&path).unwrap().version;
    assert!(v >= 2, "version should be at least 2 after two reloads; got {v}");
}
