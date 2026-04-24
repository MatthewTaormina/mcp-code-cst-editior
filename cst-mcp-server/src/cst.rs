use std::path::{Path, PathBuf};
use serde::Serialize;
use tree_sitter::StreamingIterator as _;
// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------
/// A tree-sitter node identity within a single parse tree.
///
/// Equal to `tree_sitter::Node::id() as u64`.  Node IDs are unique within one
/// parse tree but are **not** stable across re-parses (any edit increments the
/// file version and all IDs become stale).  Always re-query after an edit.
pub type NodeId = u64;
// ---------------------------------------------------------------------------
// FileLanguage
// ---------------------------------------------------------------------------
/// The grammar to use when parsing a source file with tree-sitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLanguage {
    Rust,
    JavaScript,
    /// TypeScript (`.ts`).
    TypeScript,
    /// TSX — TypeScript with JSX (`.tsx`).
    Tsx,
    Css,
    Html,
    /// No recognised grammar — file is stored verbatim without a parse tree.
    Plain,
}
impl FileLanguage {
    /// Infer the language from a file path extension.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => FileLanguage::Rust,
            Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => FileLanguage::JavaScript,
            Some("ts") => FileLanguage::TypeScript,
            Some("tsx") => FileLanguage::Tsx,
            Some("css") | Some("scss") | Some("sass") | Some("less") => FileLanguage::Css,
            Some("html") | Some("htm") | Some("svg") => FileLanguage::Html,
            _ => FileLanguage::Plain,
        }
    }
    /// Return the tree-sitter `Language` for this variant, or `None` for
    /// `Plain` (no grammar).
    pub fn ts_language(self) -> Option<tree_sitter::Language> {
        match self {
            FileLanguage::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            FileLanguage::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            FileLanguage::TypeScript => {
                Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            }
            FileLanguage::Tsx => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
            FileLanguage::Css => Some(tree_sitter_css::LANGUAGE.into()),
            FileLanguage::Html => Some(tree_sitter_html::LANGUAGE.into()),
            FileLanguage::Plain => None,
        }
    }
}
// ---------------------------------------------------------------------------
// NodeInfo
// ---------------------------------------------------------------------------
/// Rich metadata for a single CST node.
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    /// Unique ID within the current parse tree.
    pub node_id: NodeId,
    /// tree-sitter node kind (e.g. `"function_definition"`, `"identifier"`).
    pub kind: String,
    /// First ~80 chars of the node's source text (truncated at first newline).
    pub text_preview: String,
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub is_named: bool,
    /// `true` if this subtree contains a parse error.
    pub has_error: bool,
    /// Number of named (non-anonymous) children.
    pub named_child_count: u32,
}
// ---------------------------------------------------------------------------
// ChildInfo
// ---------------------------------------------------------------------------
/// Summary of one direct child of a CST node.
#[derive(Debug, Clone, Serialize)]
pub struct ChildInfo {
    pub node_id: NodeId,
    pub kind: String,
    /// Field name if the child is a named field of its parent (e.g. `"name"`,
    /// `"body"`, `"parameters"`), or `None` for anonymous structural children.
    pub field_name: Option<String>,
    pub text_preview: String,
    pub start_row: u32,
    pub start_col: u32,
    pub named_child_count: u32,
    pub has_error: bool,
}
// ---------------------------------------------------------------------------
// QueryMatchResult
// ---------------------------------------------------------------------------
/// One result entry from a tree-sitter s-expression query.
#[derive(Debug, Clone, Serialize)]
pub struct QueryMatchResult {
    /// The `@capture_name` that matched.
    pub capture_name: String,
    pub node_id: NodeId,
    pub kind: String,
    pub text_preview: String,
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
}
// ---------------------------------------------------------------------------
// CstFile
// ---------------------------------------------------------------------------
/// An in-memory representation of a parsed source file.
pub struct CstFile {
    pub path: PathBuf,
    /// Owned UTF-8 source text — the single source of truth for file content.
    source: String,
    /// Parsed tree-sitter CST, or `None` for plain-text files.
    tree: Option<tree_sitter::Tree>,
    /// Monotonically increasing version counter.  Incremented on every reload
    /// or successful mutation.  Node IDs are stale after any version change.
    pub version: u64,
    language: FileLanguage,
}
impl CstFile {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------
    /// Parse `content` into a CST using the grammar appropriate for `path`'s
    /// file extension.  Plain-text files are stored without a parse tree.
    pub fn parse(path: PathBuf, content: &str) -> Self {
        Self::parse_with_version(path, content, 0)
    }
    fn parse_with_version(path: PathBuf, content: &str, version: u64) -> Self {
        let language = FileLanguage::from_path(&path);
        let source = content.to_string();
        let tree = Self::do_parse(language, &source);
        Self { path, source, tree, version, language }
    }
    fn do_parse(language: FileLanguage, source: &str) -> Option<tree_sitter::Tree> {
        let lang = language.ts_language()?;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&lang).ok()?;
        parser.parse(source, None)
    }
    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------
    /// The grammar used for this file.
    pub fn language(&self) -> FileLanguage {
        self.language
    }
    /// The file's source text (lossless — never modified by edits in place).
    pub fn to_text(&self) -> &str {
        &self.source
    }
    /// The root node ID of the parse tree, if one exists.
    pub fn root_node_id(&self) -> Option<NodeId> {
        self.tree.as_ref().map(|t| t.root_node().id() as NodeId)
    }
    // -----------------------------------------------------------------------
    // Node lookup
    // -----------------------------------------------------------------------
    /// Find a node in the parse tree by its ID using an iterative DFS walk.
    ///
    /// Returns `None` if the tree is absent (plain text) or if the ID does
    /// not match any current node (stale reference after a version change).
    pub fn find_node(&self, target_id: NodeId) -> Option<tree_sitter::Node<'_>> {
        let tree = self.tree.as_ref()?;
        let target = target_id as usize;
        let mut cursor = tree.walk();
        let mut depth = 0i32;
        loop {
            let node = cursor.node();
            if node.id() == target {
                return Some(node);
            }
            if cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if depth == 0 {
                    return None;
                }
                cursor.goto_parent();
                depth -= 1;
            }
        }
    }
    /// Return rich metadata for the node identified by `node_id`.
    pub fn get_node(&self, node_id: NodeId) -> anyhow::Result<NodeInfo> {
        let tree = self.tree.as_ref()
            .ok_or_else(|| anyhow::anyhow!("plain-text files do not have a parse tree"))?;
        let node = if (tree.root_node().id() as NodeId) == node_id {
            tree.root_node()
        } else {
            self.find_node(node_id)
                .ok_or_else(|| anyhow::anyhow!(
                    "node_id {} not found — the file may have been edited since you last read it \
                     (version {})",
                    node_id, self.version
                ))?
        };
        Ok(self.node_to_info(node))
    }
    // -----------------------------------------------------------------------
    // Tree skeleton
    // -----------------------------------------------------------------------
    /// Return a hierarchical JSON representation of the parse tree.
    ///
    /// * `root_id` — start from this node (default: file root).
    /// * `max_depth` — maximum recursion depth (default: 3).
    /// * `named_only` — when `true`, omit anonymous punctuation/keyword nodes.
    pub fn get_tree_skeleton(
        &self,
        root_id: Option<NodeId>,
        max_depth: Option<u32>,
        named_only: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let tree = self.tree.as_ref()
            .ok_or_else(|| anyhow::anyhow!("plain-text files do not have a parse tree"))?;
        let root_node = match root_id {
            None => tree.root_node(),
            Some(id) => {
                if (tree.root_node().id() as NodeId) == id {
                    tree.root_node()
                } else {
                    self.find_node(id).ok_or_else(|| {
                        anyhow::anyhow!("node_id {} not found (version {})", id, self.version)
                    })?
                }
            }
        };
        let max_d = max_depth.unwrap_or(3);
        Ok(self.node_to_skeleton(root_node, 0, max_d, named_only))
    }
    fn node_to_skeleton(
        &self,
        node: tree_sitter::Node<'_>,
        depth: u32,
        max_depth: u32,
        named_only: bool,
    ) -> serde_json::Value {
        let text = &self.source[node.start_byte()..node.end_byte()];
        let start = node.start_position();
        let mut obj = serde_json::json!({
            "node_id": node.id() as NodeId,
            "kind": node.kind(),
            "named": node.is_named(),
            "text_preview": make_preview(text),
            "row": start.row,
            "col": start.column,
            "named_child_count": node.named_child_count(),
        });
        if node.has_error() {
            obj["has_error"] = serde_json::Value::Bool(true);
        }
        if depth < max_depth && node.child_count() > 0 {
            // Collect children first, then recurse (avoids borrow overlap).
            let child_nodes: Vec<tree_sitter::Node<'_>> = {
                let mut cur = node.walk();
                node.children(&mut cur)
                    .filter(|c| !named_only || c.is_named())
                    .collect()
            };
            if !child_nodes.is_empty() {
                let children: Vec<serde_json::Value> = child_nodes
                    .into_iter()
                    .map(|c| self.node_to_skeleton(c, depth + 1, max_depth, named_only))
                    .collect();
                obj["children"] = serde_json::Value::Array(children);
            }
        }
        obj
    }
    // -----------------------------------------------------------------------
    // Children
    // -----------------------------------------------------------------------
    /// Return the direct children of a node, with optional filtering to named
    /// nodes only and field-name annotation.
    pub fn get_children(
        &self,
        node_id: NodeId,
        named_only: bool,
    ) -> anyhow::Result<Vec<ChildInfo>> {
        let tree = self.tree.as_ref()
            .ok_or_else(|| anyhow::anyhow!("plain-text files do not have a parse tree"))?;
        let node = if (tree.root_node().id() as NodeId) == node_id {
            tree.root_node()
        } else {
            self.find_node(node_id)
                .ok_or_else(|| anyhow::anyhow!("node_id {} not found", node_id))?
        };
        let mut children = Vec::new();
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if !named_only || child.is_named() {
                    let field_name = cursor.field_name().map(|s| s.to_string());
                    let text = &self.source[child.start_byte()..child.end_byte()];
                    let start = child.start_position();
                    children.push(ChildInfo {
                        node_id: child.id() as NodeId,
                        kind: child.kind().to_string(),
                        field_name,
                        text_preview: make_preview(text),
                        start_row: start.row as u32,
                        start_col: start.column as u32,
                        named_child_count: child.named_child_count() as u32,
                        has_error: child.has_error(),
                    });
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        Ok(children)
    }
    // -----------------------------------------------------------------------
    // Edit operations
    // -----------------------------------------------------------------------
    /// Replace a node's source span with `new_text`, re-parse, and return a
    /// new `CstFile` with an incremented version.
    pub fn replace_node(
        &self,
        node_id: NodeId,
        new_text: &str,
        expected_version: Option<u64>,
    ) -> anyhow::Result<CstFile> {
        self.check_version(expected_version)?;
        let node = self.require_node(node_id)?;
        let new_source = splice(&self.source, node.start_byte(), node.end_byte(), new_text);
        Ok(Self::parse_with_version(self.path.clone(), &new_source, self.version + 1))
    }
    /// Insert `text` immediately before a node (before its first byte).
    pub fn insert_before_node(
        &self,
        node_id: NodeId,
        text: &str,
        expected_version: Option<u64>,
    ) -> anyhow::Result<CstFile> {
        self.check_version(expected_version)?;
        let node = self.require_node(node_id)?;
        let new_source = splice(&self.source, node.start_byte(), node.start_byte(), text);
        Ok(Self::parse_with_version(self.path.clone(), &new_source, self.version + 1))
    }
    /// Insert `text` immediately after a node (after its last byte).
    pub fn insert_after_node(
        &self,
        node_id: NodeId,
        text: &str,
        expected_version: Option<u64>,
    ) -> anyhow::Result<CstFile> {
        self.check_version(expected_version)?;
        let node = self.require_node(node_id)?;
        let new_source = splice(&self.source, node.end_byte(), node.end_byte(), text);
        Ok(Self::parse_with_version(self.path.clone(), &new_source, self.version + 1))
    }
    /// Insert `text` inside a node — either at its very start (`at_start =
    /// true`) or at its very end (`at_start = false`).
    ///
    /// Typical usage: insert a new statement at the end of a function body.
    pub fn insert_into_node(
        &self,
        node_id: NodeId,
        text: &str,
        at_start: bool,
        expected_version: Option<u64>,
    ) -> anyhow::Result<CstFile> {
        self.check_version(expected_version)?;
        let node = self.require_node(node_id)?;
        let byte = if at_start { node.start_byte() } else { node.end_byte() };
        let new_source = splice(&self.source, byte, byte, text);
        Ok(Self::parse_with_version(self.path.clone(), &new_source, self.version + 1))
    }
    /// Delete a node's source span and re-parse.
    pub fn delete_node(
        &self,
        node_id: NodeId,
        expected_version: Option<u64>,
    ) -> anyhow::Result<CstFile> {
        self.check_version(expected_version)?;
        let node = self.require_node(node_id)?;
        let new_source = splice(&self.source, node.start_byte(), node.end_byte(), "");
        Ok(Self::parse_with_version(self.path.clone(), &new_source, self.version + 1))
    }
    // -----------------------------------------------------------------------
    // Tree-sitter query
    // -----------------------------------------------------------------------
    /// Run a tree-sitter s-expression query and return capture matches.
    ///
    /// Example query: `"(function_item name: (identifier) @fn_name)"`
    pub fn query_ts(
        &self,
        ts_query: &str,
        max_matches: Option<usize>,
    ) -> anyhow::Result<Vec<QueryMatchResult>> {
        let tree = self.tree.as_ref()
            .ok_or_else(|| anyhow::anyhow!("plain-text files do not support tree-sitter queries"))?;
        let lang = self.language.ts_language()
            .ok_or_else(|| anyhow::anyhow!("no language for query"))?;
        let query = tree_sitter::Query::new(&lang, ts_query)?;
        let cap_names: Vec<String> = query.capture_names().iter().map(|n| n.to_string()).collect();
        let limit = max_matches.unwrap_or(usize::MAX);
        let mut qcursor = tree_sitter::QueryCursor::new();
        let mut results = Vec::new();
        let mut matches = qcursor.matches(&query, tree.root_node(), self.source.as_bytes());
        'outer: while let Some(m) = matches.next() {
            for cap in m.captures {
                if results.len() >= limit {
                    break 'outer;
                }
                let node = cap.node;
                let capture_name = cap_names
                    .get(cap.index as usize)
                    .cloned()
                    .unwrap_or_default();
                let text = &self.source[node.start_byte()..node.end_byte()];
                let start = node.start_position();
                let end = node.end_position();
                results.push(QueryMatchResult {
                    capture_name,
                    node_id: node.id() as NodeId,
                    kind: node.kind().to_string(),
                    text_preview: make_preview(text),
                    start_row: start.row as u32,
                    start_col: start.column as u32,
                    end_row: end.row as u32,
                    end_col: end.column as u32,
                });
            }
        }
        Ok(results)
    }
    // -----------------------------------------------------------------------
    // Error detection
    // -----------------------------------------------------------------------
    /// Collect all ERROR and MISSING nodes from the parse tree.
    pub fn get_errors(&self) -> Vec<serde_json::Value> {
        let Some(tree) = &self.tree else { return Vec::new() };
        let mut errors = Vec::new();
        let mut cursor = tree.walk();
        let mut depth = 0i32;
        loop {
            let node = cursor.node();
            if node.is_error() || node.is_missing() {
                let start = node.start_position();
                let end = node.end_position();
                let text = &self.source[node.start_byte()..node.end_byte()];
                errors.push(serde_json::json!({
                    "kind": if node.is_error() { "ERROR" } else { "MISSING" },
                    "start_row": start.row,
                    "start_col": start.column,
                    "end_row": end.row,
                    "end_col": end.column,
                    "text_preview": make_preview(text),
                }));
            }
            if cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if depth == 0 {
                    return errors;
                }
                cursor.goto_parent();
                depth -= 1;
            }
        }
    }
    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------
    fn check_version(&self, expected: Option<u64>) -> anyhow::Result<()> {
        if let Some(v) = expected {
            if self.version != v {
                anyhow::bail!(
                    "conflict: file is at version {} but caller expected {} — \
                     re-read the file and retry",
                    self.version, v
                );
            }
        }
        Ok(())
    }
    fn require_node(&self, node_id: NodeId) -> anyhow::Result<tree_sitter::Node<'_>> {
        let tree = self.tree.as_ref()
            .ok_or_else(|| anyhow::anyhow!("plain-text files do not have a parse tree"))?;
        if (tree.root_node().id() as NodeId) == node_id {
            return Ok(tree.root_node());
        }
        self.find_node(node_id).ok_or_else(|| {
            anyhow::anyhow!(
                "node_id {} not found — the file may have been edited (version {})",
                node_id, self.version
            )
        })
    }
    fn node_to_info(&self, node: tree_sitter::Node<'_>) -> NodeInfo {
        let start = node.start_position();
        let end = node.end_position();
        let text = &self.source[node.start_byte()..node.end_byte()];
        NodeInfo {
            node_id: node.id() as NodeId,
            kind: node.kind().to_string(),
            text_preview: make_preview(text),
            start_row: start.row as u32,
            start_col: start.column as u32,
            end_row: end.row as u32,
            end_col: end.column as u32,
            start_byte: node.start_byte() as u32,
            end_byte: node.end_byte() as u32,
            is_named: node.is_named(),
            has_error: node.has_error(),
            named_child_count: node.named_child_count() as u32,
        }
    }
}
// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------
/// Return a single-line preview of at most 80 chars from `text`.
pub fn make_preview(text: &str) -> String {
    let first = text.lines().next().unwrap_or("");
    let trimmed = first.trim_end();
    let multi = text.contains('\n') && !text.trim_end_matches('\n').is_empty()
        && text.trim_end_matches('\n').contains('\n');
    if trimmed.len() > 80 || multi {
        let cut = trimmed.char_indices().nth(80).map(|(i, _)| i).unwrap_or(trimmed.len());
        format!("{}…", &trimmed[..cut])
    } else {
        trimmed.to_string()
    }
}
/// Splice `replacement` into `source` at byte range `[start, end)`.
fn splice(source: &str, start: usize, end: usize, replacement: &str) -> String {
    format!("{}{}{}", &source[..start], replacement, &source[end..])
}
// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    fn rust_file(src: &str) -> CstFile {
        CstFile::parse(PathBuf::from("test.rs"), src)
    }
    fn js_file(src: &str) -> CstFile {
        CstFile::parse(PathBuf::from("test.js"), src)
    }
    fn ts_file(src: &str) -> CstFile {
        CstFile::parse(PathBuf::from("test.ts"), src)
    }
    fn css_file(src: &str) -> CstFile {
        CstFile::parse(PathBuf::from("test.css"), src)
    }
    fn html_file(src: &str) -> CstFile {
        CstFile::parse(PathBuf::from("test.html"), src)
    }
    fn plain_file(src: &str) -> CstFile {
        CstFile::parse(PathBuf::from("test.txt"), src)
    }
    // --- language detection ---
    #[test]
    fn rs_extension_detected_as_rust() {
        assert_eq!(rust_file("fn f() {}").language(), FileLanguage::Rust);
    }
    #[test]
    fn js_extension_detected_as_javascript() {
        assert_eq!(js_file("const x = 1;").language(), FileLanguage::JavaScript);
    }
    #[test]
    fn ts_extension_detected_as_typescript() {
        assert_eq!(ts_file("const x: number = 1;").language(), FileLanguage::TypeScript);
    }
    #[test]
    fn tsx_extension_detected_as_tsx() {
        let f = CstFile::parse(PathBuf::from("App.tsx"), "<div/>");
        assert_eq!(f.language(), FileLanguage::Tsx);
    }
    #[test]
    fn css_extension_detected_as_css() {
        assert_eq!(css_file("body {}").language(), FileLanguage::Css);
    }
    #[test]
    fn html_extension_detected_as_html() {
        assert_eq!(html_file("<html/>").language(), FileLanguage::Html);
    }
    #[test]
    fn txt_extension_detected_as_plain() {
        assert_eq!(plain_file("hello").language(), FileLanguage::Plain);
    }
    // --- roundtrip ---
    #[test]
    fn parse_roundtrip_rust() {
        let src = "fn main() {\n    println!(\"hello\");\n}\n";
        let f = rust_file(src);
        assert_eq!(f.to_text(), src);
    }
    #[test]
    fn parse_roundtrip_js() {
        let src = "const x = 1;\nfunction foo() { return x; }\n";
        assert_eq!(js_file(src).to_text(), src);
    }
    #[test]
    fn parse_roundtrip_plain() {
        let src = "hello world\n";
        assert_eq!(plain_file(src).to_text(), src);
    }
    // --- tree-sitter parse tree ---
    #[test]
    fn rust_has_parse_tree() {
        let f = rust_file("fn f() {}");
        assert!(f.root_node_id().is_some());
    }
    #[test]
    fn plain_has_no_parse_tree() {
        let f = plain_file("hello");
        assert!(f.root_node_id().is_none());
    }
    #[test]
    fn get_root_node_works() {
        let f = rust_file("fn f() {}");
        let root_id = f.root_node_id().unwrap();
        let info = f.get_node(root_id).unwrap();
        assert_eq!(info.node_id, root_id);
        assert_eq!(info.kind, "source_file");
    }
    #[test]
    fn find_child_node_works() {
        let f = rust_file("fn hello() {}");
        let root_id = f.root_node_id().unwrap();
        let children = f.get_children(root_id, true).unwrap();
        assert!(!children.is_empty(), "source_file should have named children");
        let fn_child = &children[0];
        assert_eq!(fn_child.kind, "function_item");
        let info = f.get_node(fn_child.node_id).unwrap();
        assert_eq!(info.kind, "function_item");
    }
    #[test]
    fn tree_skeleton_returns_hierarchical_json() {
        let f = rust_file("fn foo() {}\nfn bar() {}\n");
        let skeleton = f.get_tree_skeleton(None, Some(2), true).unwrap();
        assert_eq!(skeleton["kind"], "source_file");
        let children = skeleton["children"].as_array().unwrap();
        assert_eq!(children.len(), 2, "two top-level function_item nodes");
        assert_eq!(children[0]["kind"], "function_item");
    }
    #[test]
    fn get_children_with_named_only_false_includes_punctuation() {
        let f = rust_file("fn f() {}");
        let root_id = f.root_node_id().unwrap();
        let children_all = f.get_children(root_id, false).unwrap();
        let children_named = f.get_children(root_id, true).unwrap();
        assert!(children_all.len() >= children_named.len());
    }
    // --- initial version ---
    #[test]
    fn initial_version_is_zero() {
        assert_eq!(rust_file("fn f() {}").version, 0);
    }
    // --- replace_node ---
    #[test]
    fn replace_node_increments_version() {
        let f = rust_file("fn foo() {}\n");
        let root_id = f.root_node_id().unwrap();
        let fn_child_id = f.get_children(root_id, true).unwrap()[0].node_id;
        let f2 = f.replace_node(fn_child_id, "fn bar() {}\n", None).unwrap();
        assert_eq!(f2.version, 1);
    }
    #[test]
    fn replace_node_changes_text() {
        let f = rust_file("fn foo() {}\n");
        let root_id = f.root_node_id().unwrap();
        let fn_child_id = f.get_children(root_id, true).unwrap()[0].node_id;
        let f2 = f.replace_node(fn_child_id, "fn bar() {}\n", None).unwrap();
        assert!(f2.to_text().contains("bar"));
        assert!(!f2.to_text().contains("foo"));
    }
    #[test]
    fn replace_node_conflict_detection() {
        let f = rust_file("fn f() {}\n");
        let root_id = f.root_node_id().unwrap();
        let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
        let result = f.replace_node(fn_id, "fn g() {}\n", Some(99));
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("conflict"));
    }
    // --- insert_before_node / insert_after_node ---
    #[test]
    fn insert_before_node_prepends_text() {
        let f = rust_file("fn foo() {}\n");
        let root_id = f.root_node_id().unwrap();
        let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
        let f2 = f.insert_before_node(fn_id, "// comment\n", None).unwrap();
        assert!(f2.to_text().starts_with("// comment\nfn foo()"));
        assert_eq!(f2.version, 1);
    }
    #[test]
    fn insert_after_node_appends_text() {
        let f = rust_file("fn foo() {}\n");
        let root_id = f.root_node_id().unwrap();
        let fn_id = f.get_children(root_id, true).unwrap()[0].node_id;
        let f2 = f.insert_after_node(fn_id, "\nfn bar() {}\n", None).unwrap();
        assert!(f2.to_text().contains("fn bar()"));
    }
    // --- delete_node ---
    #[test]
    fn delete_node_removes_text() {
        let f = rust_file("fn foo() {}\nfn bar() {}\n");
        let root_id = f.root_node_id().unwrap();
        let children = f.get_children(root_id, true).unwrap();
        assert_eq!(children.len(), 2);
        let foo_id = children[0].node_id;
        let f2 = f.delete_node(foo_id, None).unwrap();
        assert!(!f2.to_text().contains("foo"));
        assert!(f2.to_text().contains("bar"));
        assert_eq!(f2.version, 1);
    }
    // --- successive mutations ---
    #[test]
    fn successive_mutations_increment_version() {
        let f = rust_file("fn a() {}\nfn b() {}\n");
        let root_id = f.root_node_id().unwrap();
        let children = f.get_children(root_id, true).unwrap();
        let id0 = children[0].node_id;
        let f1 = f.replace_node(id0, "fn aa() {}\n", None).unwrap();
        assert_eq!(f1.version, 1);
        // After edit, tree is re-parsed; must re-query IDs.
        let root_id2 = f1.root_node_id().unwrap();
        let children2 = f1.get_children(root_id2, true).unwrap();
        let id1 = children2[1].node_id;
        let f2 = f1.replace_node(id1, "fn bb() {}\n", None).unwrap();
        assert_eq!(f2.version, 2);
    }
    // --- query_ts ---
    #[test]
    fn query_ts_finds_function_names() {
        let f = rust_file("fn alpha() {}\nfn beta() {}\n");
        let matches = f
            .query_ts("(function_item name: (identifier) @fn_name)", None)
            .unwrap();
        let names: Vec<&str> = matches.iter().map(|m| m.text_preview.as_str()).collect();
        assert!(names.contains(&"alpha"), "should find 'alpha'");
        assert!(names.contains(&"beta"), "should find 'beta'");
    }
    #[test]
    fn query_ts_respects_max_matches() {
        let f = rust_file("fn a() {}\nfn b() {}\nfn c() {}\n");
        let matches = f
            .query_ts("(function_item name: (identifier) @n)", Some(2))
            .unwrap();
        assert!(matches.len() <= 2);
    }
    #[test]
    fn query_ts_js_finds_const_declarations() {
        let f = js_file("const x = 1;\nconst y = 2;\n");
        let matches = f
            .query_ts("(lexical_declaration (variable_declarator name: (identifier) @name))", None)
            .unwrap();
        let names: Vec<&str> = matches.iter().map(|m| m.text_preview.as_str()).collect();
        assert!(names.contains(&"x"), "should find 'x'");
        assert!(names.contains(&"y"), "should find 'y'");
    }
    // --- get_errors ---
    #[test]
    fn get_errors_empty_for_valid_code() {
        let f = rust_file("fn f() {}\n");
        assert!(f.get_errors().is_empty());
    }
    #[test]
    fn get_errors_does_not_panic_on_invalid_code() {
        let f = rust_file("fn broken( {\n");
        let _ = f.get_errors(); // must not panic
    }
    // --- make_preview ---
    #[test]
    fn preview_truncates_at_80_chars() {
        let long = "a".repeat(100);
        let p = make_preview(&long);
        assert!(p.ends_with('…'));
    }
    #[test]
    fn preview_takes_first_line_only() {
        let p = make_preview("first line\nsecond line");
        assert!(!p.contains("second"));
    }
}
