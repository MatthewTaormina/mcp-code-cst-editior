//! Integration tests for the CST-MCP server.
//!
//! These tests exercise the full stack (state + watcher + CST) using real
//! temporary files on disk, catching interactions that unit tests cannot.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cst_mcp_server::{
    cst::{CstFile, FileLanguage},
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

// ---------------------------------------------------------------------------
// Phase 3: Language-specific lexer (Rust token-level grammar)
// ---------------------------------------------------------------------------

#[test]
fn rust_file_gets_rust_language() {
    let file = CstFile::parse(PathBuf::from("src/lib.rs"), "fn f() {}\n");
    assert_eq!(file.language(), FileLanguage::Rust);
}

#[test]
fn non_rust_file_gets_plain_language() {
    for ext in &["txt", "py", "md", "toml", "json"] {
        let path = PathBuf::from(format!("file.{ext}"));
        let file = CstFile::parse(path.clone(), "hello\n");
        assert_eq!(
            file.language(),
            FileLanguage::Plain,
            ".{ext} should use Plain language"
        );
    }
}

#[test]
fn rust_lexer_emits_keyword_tokens() {
    let src = "pub fn hello() -> bool {\n    true\n}\n";
    let file = CstFile::parse(PathBuf::from("a.rs"), src);

    let tokens_line0 = file.get_line_tokens(0).unwrap();
    let kinds: Vec<&str> = tokens_line0.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&"Keyword"), "line 0 should contain a Keyword token");

    let keywords: Vec<&str> = tokens_line0
        .iter()
        .filter(|t| t.kind == "Keyword")
        .map(|t| t.text.as_str())
        .collect();
    assert!(keywords.contains(&"pub"), "expected 'pub' keyword");
    assert!(keywords.contains(&"fn"), "expected 'fn' keyword");
}

#[test]
fn rust_lexer_lossless_per_line() {
    let src = "fn main() {\n    let x = 42;\n    println!(\"{x}\");\n}\n";
    let file = CstFile::parse(PathBuf::from("b.rs"), src);
    let node_count = file.tree_skeleton().len();

    for i in 0..node_count as u32 {
        let line_info = file.get_node(i).unwrap();
        let tokens = file.get_line_tokens(i).unwrap();
        let reconstructed: String = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            reconstructed, line_info.text,
            "line {i} token reconstruction must be lossless"
        );
    }
}

#[test]
fn rust_lexer_comment_token() {
    let src = "// this is a comment\nfn f() {}\n";
    let file = CstFile::parse(PathBuf::from("c.rs"), src);
    let tokens = file.get_line_tokens(0).unwrap();
    assert!(
        tokens.iter().any(|t| t.kind == "Comment"),
        "line 0 should be a Comment token"
    );
}

#[test]
fn rust_lexer_string_literal_token() {
    let src = "let s = \"hello world\";\n";
    let file = CstFile::parse(PathBuf::from("d.rs"), src);
    let tokens = file.get_line_tokens(0).unwrap();
    assert!(
        tokens.iter().any(|t| t.kind == "Literal" && t.text.contains("hello")),
        "should contain a Literal token for the string"
    );
}

#[test]
fn get_line_tokens_span_continuity() {
    let src = "fn foo(x: u32) -> u32 { x + 1 }\n";
    let file = CstFile::parse(PathBuf::from("e.rs"), src);
    let tokens = file.get_line_tokens(0).unwrap();

    // Spans must be contiguous: each token starts where the previous ended.
    for window in tokens.windows(2) {
        assert_eq!(
            window[0].span_end, window[1].span_start,
            "token spans must be contiguous"
        );
    }
}

#[test]
fn plain_lexer_preserves_full_line_text() {
    let src = "hello world: some=random, text!\n";
    let file = CstFile::parse(PathBuf::from("notes.txt"), src);
    let tokens = file.get_line_tokens(0).unwrap();
    let reconstructed: String = tokens.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(reconstructed, src);
}

// ---------------------------------------------------------------------------
// Phase 3: list_tracked_files via ServerState
// ---------------------------------------------------------------------------

#[test]
fn tracked_paths_is_sorted() {
    let mut state = ServerState::new();
    let paths = [
        PathBuf::from("/z/file.rs"),
        PathBuf::from("/a/file.rs"),
        PathBuf::from("/m/file.rs"),
    ];
    for p in &paths {
        state.track(p.clone(), CstFile::parse(p.clone(), "x\n"));
    }

    let tracked = state.tracked_paths();
    let mut expected = paths.to_vec();
    expected.sort();
    assert_eq!(tracked, expected, "tracked_paths() must return sorted paths");
}

#[test]
fn tracked_paths_empty_when_none_tracked() {
    let state = ServerState::new();
    assert!(state.tracked_paths().is_empty());
}

#[test]
fn tracked_paths_updates_after_untrack() {
    let mut state = ServerState::new();
    let p1 = PathBuf::from("/a.rs");
    let p2 = PathBuf::from("/b.rs");
    state.track(p1.clone(), CstFile::parse(p1.clone(), "x\n"));
    state.track(p2.clone(), CstFile::parse(p2.clone(), "y\n"));

    state.untrack(&p1);
    let tracked = state.tracked_paths();
    assert_eq!(tracked, vec![p2]);
}

// ---------------------------------------------------------------------------
// Phase 3: replace_node preserves language-specific lexing
// ---------------------------------------------------------------------------

#[test]
fn edit_preserves_rust_token_structure() {
    let src = "fn main() {\n    let x = 1;\n}\n";
    let file = CstFile::parse(PathBuf::from("f.rs"), src);

    let edited = file.replace_node(1, "    let y = 2;").unwrap();

    // The edited file should still use the Rust lexer.
    assert_eq!(edited.language(), FileLanguage::Rust);

    // The edited line should contain a Keyword token for "let".
    let tokens = edited.get_line_tokens(1).unwrap();
    assert!(
        tokens.iter().any(|t| t.kind == "Keyword" && t.text == "let"),
        "edited line should contain 'let' keyword token"
    );

    // Unedited lines must be unchanged.
    assert_eq!(edited.get_node(0).unwrap().text, "fn main() {\n");
    assert_eq!(edited.get_node(2).unwrap().text, "}\n");
}

