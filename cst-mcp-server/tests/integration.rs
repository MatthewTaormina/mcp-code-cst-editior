//! Integration tests for the tree-sitter based CST-MCP server.
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
async fn setup() -> (Arc<RwLock<ServerState>>, cst_mcp_server::watcher::WatcherHandle) {
    let state = Arc::new(RwLock::new(ServerState::new()));
    let handle = start_watcher(Arc::clone(&state)).expect("watcher failed to start");
    (state, handle)
}
fn make_temp_file(content: &str) -> (tempfile::NamedTempFile, PathBuf) {
    let mut tmp = tempfile::NamedTempFile::new().expect("tempfile creation failed");
    tmp.write_all(content.as_bytes()).expect("write failed");
    tmp.flush().expect("flush failed");
    let path = tmp.path().to_path_buf();
    (tmp, path)
}
// ---------------------------------------------------------------------------
// Language detection
// ---------------------------------------------------------------------------
#[test]
fn rust_file_gets_rust_language() {
    let f = CstFile::parse(PathBuf::from("src/lib.rs"), "fn f() {}\n");
    assert_eq!(f.language(), FileLanguage::Rust);
}
#[test]
fn js_file_gets_javascript_language() {
    for ext in &["js", "jsx", "mjs", "cjs"] {
        let f = CstFile::parse(PathBuf::from(format!("file.{ext}")), "const x = 1;\n");
        assert_eq!(f.language(), FileLanguage::JavaScript, ".{ext}");
    }
}
#[test]
fn ts_file_gets_typescript_language() {
    let f = CstFile::parse(PathBuf::from("file.ts"), "const x: number = 1;\n");
    assert_eq!(f.language(), FileLanguage::TypeScript);
}
#[test]
fn tsx_file_gets_tsx_language() {
    let f = CstFile::parse(PathBuf::from("App.tsx"), "const x = <div/>;\n");
    assert_eq!(f.language(), FileLanguage::Tsx);
}
#[test]
fn css_file_gets_css_language() {
    for ext in &["css", "scss", "sass", "less"] {
        let f = CstFile::parse(PathBuf::from(format!("file.{ext}")), "body { color: red; }\n");
        assert_eq!(f.language(), FileLanguage::Css, ".{ext}");
    }
}
#[test]
fn html_file_gets_html_language() {
    for ext in &["html", "htm", "svg"] {
        let f = CstFile::parse(PathBuf::from(format!("file.{ext}")), "<div>hi</div>\n");
        assert_eq!(f.language(), FileLanguage::Html, ".{ext}");
    }
}
#[test]
fn non_recognised_extension_gets_plain_language() {
    for ext in &["txt", "py", "md", "toml", "json"] {
        let f = CstFile::parse(PathBuf::from(format!("file.{ext}")), "hello\n");
        assert_eq!(f.language(), FileLanguage::Plain, ".{ext}");
    }
}
// ---------------------------------------------------------------------------
// Roundtrip
// ---------------------------------------------------------------------------
#[test]
fn cst_parse_and_roundtrip_rust() {
    let src = "fn main() {\n    println!(\"hello\");\n}\n";
    let f = CstFile::parse(PathBuf::from("test.rs"), src);
    assert_eq!(f.to_text(), src);
}
#[test]
fn cst_parse_and_roundtrip_js() {
    let src = "const x = 1;\nfunction foo() { return x; }\n";
    let f = CstFile::parse(PathBuf::from("test.js"), src);
    assert_eq!(f.to_text(), src);
}
#[test]
fn cst_parse_and_roundtrip_plain() {
    let src = "hello world\n";
    let f = CstFile::parse(PathBuf::from("test.txt"), src);
    assert_eq!(f.to_text(), src);
}
// ---------------------------------------------------------------------------
// Parse tree structure
// ---------------------------------------------------------------------------
#[test]
fn rust_file_has_parse_tree() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn f() {}");
    assert!(f.root_node_id().is_some());
}
#[test]
fn plain_file_has_no_parse_tree() {
    let f = CstFile::parse(PathBuf::from("t.txt"), "hello");
    assert!(f.root_node_id().is_none());
}
#[test]
fn root_node_kind_is_source_file_for_rust() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn f() {}");
    let root_id = f.root_node_id().unwrap();
    let info = f.get_node(root_id).unwrap();
    assert_eq!(info.kind, "source_file");
}
#[test]
fn get_children_returns_function_items() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\nfn bar() {}\n");
    let root_id = f.root_node_id().unwrap();
    let children = f.get_children(root_id, true).unwrap();
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|c| c.kind == "function_item"));
}
#[test]
fn get_tree_skeleton_is_hierarchical() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\n");
    let skeleton = f.get_tree_skeleton(None, Some(3), true).unwrap();
    assert_eq!(skeleton["kind"], "source_file");
    let children = skeleton["children"].as_array().unwrap();
    assert!(!children.is_empty());
    assert_eq!(children[0]["kind"], "function_item");
}
#[test]
fn get_tree_skeleton_from_child_node() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let skeleton = f.get_tree_skeleton(Some(fn_id), Some(2), false).unwrap();
    assert_eq!(skeleton["kind"], "function_item");
    assert_eq!(skeleton["node_id"], fn_id);
}
#[test]
fn js_function_has_expected_structure() {
    let f = CstFile::parse(PathBuf::from("t.js"), "function greet(name) { return name; }\n");
    let root_id = f.root_node_id().unwrap();
    let children = f.get_children(root_id, true).unwrap();
    assert!(!children.is_empty());
    assert_eq!(children[0].kind, "function_declaration");
}
#[test]
fn css_file_has_parse_tree() {
    let f = CstFile::parse(PathBuf::from("t.css"), "body { color: red; }\n");
    assert!(f.root_node_id().is_some());
    let root_id = f.root_node_id().unwrap();
    let info = f.get_node(root_id).unwrap();
    // CSS root is "stylesheet"
    assert_eq!(info.kind, "stylesheet");
}
#[test]
fn html_file_has_parse_tree() {
    let f = CstFile::parse(PathBuf::from("t.html"), "<html><body></body></html>");
    assert!(f.root_node_id().is_some());
}
// ---------------------------------------------------------------------------
// Version management
// ---------------------------------------------------------------------------
#[test]
fn initial_version_is_zero() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn f() {}");
    assert_eq!(f.version, 0);
}
#[test]
fn replace_node_increments_version() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let f2 = f.replace_node(fn_id, "fn bar() {}\n", None).unwrap();
    assert_eq!(f2.version, 1);
}
#[test]
fn successive_mutations_increment_version() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn a() {}\nfn b() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_a_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let f1 = f.replace_node(fn_a_id, "fn aa() {}\n", None).unwrap();
    assert_eq!(f1.version, 1);
    // Re-query after edit since IDs are stale.
    let root_id2 = f1.root_node_id().unwrap();
    let fn_b_id = f1.get_children(root_id2, true).unwrap()[1].node_id;
    let f2 = f1.replace_node(fn_b_id, "fn bb() {}\n", None).unwrap();
    assert_eq!(f2.version, 2);
}
// ---------------------------------------------------------------------------
// Edit operations
// ---------------------------------------------------------------------------
#[test]
fn replace_node_changes_text_correctly() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let f2 = f.replace_node(fn_id, "fn bar() {}\n", None).unwrap();
    assert!(f2.to_text().contains("bar"));
    assert!(!f2.to_text().contains("foo"));
}
#[test]
fn replace_node_language_preserved() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let f2 = f.replace_node(fn_id, "fn bar() {}\n", None).unwrap();
    assert_eq!(f2.language(), FileLanguage::Rust);
}
#[test]
fn replace_node_conflict_detected() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn f() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let result = f.replace_node(fn_id, "fn g() {}\n", Some(99));
    assert!(result.is_err());
    let msg = result.err().unwrap().to_string();
    assert!(msg.contains("conflict"), "error should say 'conflict': {msg}");
}
#[test]
fn insert_before_node_prepends() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let f2 = f.insert_before_node(fn_id, "// comment\n", None).unwrap();
    assert!(f2.to_text().starts_with("// comment\n"));
    assert!(f2.to_text().contains("fn foo()"));
    assert_eq!(f2.version, 1);
}
#[test]
fn insert_after_node_appends() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let f2 = f.insert_after_node(fn_id, "fn bar() {}\n", None).unwrap();
    assert!(f2.to_text().contains("fn foo()"));
    assert!(f2.to_text().contains("fn bar()"));
    assert_eq!(f2.version, 1);
}
#[test]
fn delete_node_removes_text() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn foo() {}\nfn bar() {}\n");
    let root_id = f.root_node_id().unwrap();
    let children = f.get_children(root_id, true).unwrap();
    let foo_id = children[0].node_id;
    let f2 = f.delete_node(foo_id, None).unwrap();
    assert!(!f2.to_text().contains("foo"));
    assert!(f2.to_text().contains("bar"));
    assert_eq!(f2.version, 1);
}
#[test]
fn insert_into_node_at_end() {
    // Insert a new statement at the end of a function body block
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn f() { let x = 1; }\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    // Get the function body (block)
    let fn_children = f.get_children(fn_id, true).unwrap();
    let block_id = fn_children.iter().find(|c| c.kind == "block").map(|c| c.node_id).unwrap();
    let f2 = f.insert_into_node(block_id, " let y = 2;", false, None).unwrap();
    assert!(f2.to_text().contains("let y = 2"));
    assert_eq!(f2.version, 1);
}
// ---------------------------------------------------------------------------
// Query (tree-sitter s-expression)
// ---------------------------------------------------------------------------
#[test]
fn query_ts_finds_function_names_in_rust() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn alpha() {}\nfn beta() {}\n");
    let matches = f.query_ts("(function_item name: (identifier) @fn_name)", None).unwrap();
    let names: Vec<&str> = matches.iter().map(|m| m.text_preview.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}
#[test]
fn query_ts_respects_max_matches() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n");
    let matches = f.query_ts("(function_item name: (identifier) @n)", Some(2)).unwrap();
    assert!(matches.len() <= 2);
}
#[test]
fn query_ts_finds_js_variables() {
    let f = CstFile::parse(PathBuf::from("t.js"), "const x = 1;\nconst y = 2;\n");
    let matches = f
        .query_ts("(lexical_declaration (variable_declarator name: (identifier) @name))", None)
        .unwrap();
    let names: Vec<&str> = matches.iter().map(|m| m.text_preview.as_str()).collect();
    assert!(names.contains(&"x"));
    assert!(names.contains(&"y"));
}
#[test]
fn query_ts_returns_error_for_invalid_query() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn f() {}");
    let result = f.query_ts("(this_is_not_valid !!!)", None);
    assert!(result.is_err());
}
#[test]
fn query_ts_returns_error_for_plain_text() {
    let f = CstFile::parse(PathBuf::from("t.txt"), "hello");
    let result = f.query_ts("(anything @cap)", None);
    assert!(result.is_err());
}
// ---------------------------------------------------------------------------
// get_errors
// ---------------------------------------------------------------------------
#[test]
fn get_errors_empty_for_valid_rust() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn f() {}\n");
    assert!(f.get_errors().is_empty());
}
#[test]
fn get_errors_does_not_panic_on_broken_code() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn broken( {\n");
    let _ = f.get_errors(); // must not panic
}
// ---------------------------------------------------------------------------
// State management
// ---------------------------------------------------------------------------
#[test]
fn state_track_get_untrack() {
    let mut state = ServerState::new();
    let path = PathBuf::from("/tmp/fake.rs");
    let file = CstFile::parse(path.clone(), "fn f() {}\n");
    assert!(!state.contains(&path));
    state.track(path.clone(), file);
    assert!(state.contains(&path));
    assert!(state.get(&path).is_some());
    assert!(state.untrack(&path));
    assert!(!state.contains(&path));
}
#[test]
fn contains_reflects_tracking_state() {
    let mut state = ServerState::new();
    let p = PathBuf::from("/a.rs");
    assert!(!state.contains(&p));
    state.track(p.clone(), CstFile::parse(p.clone(), "x\n"));
    assert!(state.contains(&p));
}
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
    assert_eq!(tracked, expected);
}
// ---------------------------------------------------------------------------
// Conflict detection
// ---------------------------------------------------------------------------
#[test]
fn conflict_detected_when_version_mismatch() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn a() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    // Caller thinks version is 99, but actual is 0.
    let result = f.replace_node(fn_id, "fn b() {}\n", Some(99));
    assert!(result.is_err());
}
#[test]
fn no_conflict_when_version_matches() {
    let f = CstFile::parse(PathBuf::from("t.rs"), "fn x() {}\n");
    let root_id = f.root_node_id().unwrap();
    let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
    let result = f.replace_node(fn_id, "fn y() {}\n", Some(0));
    assert!(result.is_ok());
}
// ---------------------------------------------------------------------------
// Watcher tests
// ---------------------------------------------------------------------------
#[tokio::test]
async fn watcher_reloads_file_on_external_change() {
    let (_tmp, path) = make_temp_file("fn original() {}\n");
    let (state, handle) = setup().await;
    let content = std::fs::read_to_string(&path).unwrap();
    let file = CstFile::parse(path.clone(), &content);
    state.write().await.track(path.clone(), file);
    watch_path(&handle, &path).expect("watch_path failed");
    assert_eq!(state.read().await.get(&path).unwrap().version, 0);
    std::fs::write(&path, "fn modified() {}\n").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let guard = state.read().await;
    let reloaded = guard.get(&path).expect("file should still be tracked");
    assert!(reloaded.version >= 1, "version should increment after watcher reload; got {}", reloaded.version);
    assert!(reloaded.to_text().contains("modified"));
}
#[tokio::test]
async fn watcher_does_not_affect_untracked_file() {
    let (_tmp, path) = make_temp_file("untracked\n");
    let (state, _handle) = setup().await;
    std::fs::write(&path, "changed\n").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(state.read().await.get(&path).is_none());
}
#[tokio::test]
async fn watcher_increments_version_monotonically() {
    let (_tmp, path) = make_temp_file("v0\n");
    let (state, handle) = setup().await;
    let content = std::fs::read_to_string(&path).unwrap();
    state.write().await.track(path.clone(), CstFile::parse(path.clone(), &content));
    watch_path(&handle, &path).unwrap();
    std::fs::write(&path, "v1\n").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    std::fs::write(&path, "v2\n").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let v = state.read().await.get(&path).unwrap().version;
    assert!(v >= 2, "version should be at least 2; got {v}");
}
