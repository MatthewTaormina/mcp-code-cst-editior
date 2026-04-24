use std::path::PathBuf;

use rowan::{GreenNode, GreenNodeBuilder, Language, SyntaxNode};
use serde::Serialize;

// ---------------------------------------------------------------------------
// SyntaxKind — the discriminant enum for every node/token in our generic CST.
// ---------------------------------------------------------------------------

/// Every node and token in the CST has one of these kinds.
///
/// The variants are deliberately coarse: this server treats files as opaque
/// text that is split into lines.  Future phases can introduce richer grammars
/// (e.g. Rust, Python) by adding more variants and a proper parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    /// The root node — wraps the entire file.
    Root = 0,
    /// One logical line of text (including its trailing `\n`, if any).
    Line = 1,
    /// Raw text token inside a Line node.
    Text = 2,
    /// Catch-all for anything not otherwise classified.
    Error = 3,
}

// ---------------------------------------------------------------------------
// rowan Language glue
// ---------------------------------------------------------------------------

/// Marker type that binds our `SyntaxKind` to rowan's generic machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}

impl Language for Lang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        match raw.0 {
            0 => SyntaxKind::Root,
            1 => SyntaxKind::Line,
            2 => SyntaxKind::Text,
            _ => SyntaxKind::Error,
        }
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type LangSyntaxNode = SyntaxNode<Lang>;

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// Stable identifier for a CST node.
///
/// For the line-oriented CST, the `NodeId` is the 0-based line index within
/// the file, which is stable across edits to *other* lines.
pub type NodeId = u32;

// ---------------------------------------------------------------------------
// NodeInfo — rich metadata for a single CST node
// ---------------------------------------------------------------------------

/// Serialisable metadata describing a single Line node in the CST.
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    /// 0-based index of this Line node within the file.
    pub node_id: NodeId,
    /// Human-readable kind label (always `"Line"` for the current grammar).
    pub kind: &'static str,
    /// Full text content of the line (including any trailing `\n`).
    pub text: String,
    /// Byte offset of the first character of this node within the file.
    pub span_start: u32,
    /// Byte offset one past the last character of this node.
    pub span_end: u32,
}

impl NodeInfo {
    /// A shortened preview of the node's text, suitable for skeleton listings.
    ///
    /// Strips the trailing newline and truncates to 80 characters.
    pub fn text_preview(&self) -> String {
        let raw = self.text.trim_end_matches('\n');
        if raw.len() > 80 {
            format!("{}…", &raw[..80])
        } else {
            raw.to_owned()
        }
    }
}

// ---------------------------------------------------------------------------
// CstFile
// ---------------------------------------------------------------------------

/// An in-memory representation of a parsed source file.
pub struct CstFile {
    pub path: PathBuf,
    /// The rowan green tree root — immutable, shared, and cheaply cloneable.
    pub root: GreenNode,
    /// Monotonically increasing version counter.  Incremented on every reload
    /// or successful mutation.
    pub version: u64,
}

impl CstFile {
    /// Parse `content` into a line-oriented rowan CST.
    pub fn parse(path: PathBuf, content: &str) -> Self {
        let root = build_tree(content);
        Self {
            path,
            root,
            version: 0,
        }
    }

    /// Reconstruct the file's text from the CST (lossless round-trip).
    pub fn to_text(&self) -> String {
        LangSyntaxNode::new_root(self.root.clone())
            .text()
            .to_string()
    }

    /// Return rich metadata for the Line node identified by `node_id`.
    ///
    /// Returns an error if `node_id` is out of range.
    pub fn get_node(&self, node_id: NodeId) -> anyhow::Result<NodeInfo> {
        let root = LangSyntaxNode::new_root(self.root.clone());
        let children: Vec<_> = root.children().collect();

        let idx = node_id as usize;
        if idx >= children.len() {
            anyhow::bail!(
                "node_id {} is out of range (file has {} line nodes)",
                node_id,
                children.len()
            );
        }

        let node = &children[idx];
        let range = node.text_range();

        Ok(NodeInfo {
            node_id,
            kind: "Line",
            text: node.text().to_string(),
            span_start: u32::from(range.start()),
            span_end: u32::from(range.end()),
        })
    }

    /// Return metadata for every Line node in the file, in document order.
    pub fn tree_skeleton(&self) -> Vec<NodeInfo> {
        let root = LangSyntaxNode::new_root(self.root.clone());
        root.children()
            .enumerate()
            .map(|(i, node)| {
                let range = node.text_range();
                NodeInfo {
                    node_id: i as NodeId,
                    kind: "Line",
                    text: node.text().to_string(),
                    span_start: u32::from(range.start()),
                    span_end: u32::from(range.end()),
                }
            })
            .collect()
    }

    /// Replace the content of the line identified by `node_id` (0-based line
    /// index) with `new_text`, preserving all other lines verbatim.
    ///
    /// Returns a new `CstFile` with an incremented version on success.
    pub fn replace_node(&self, node_id: NodeId, new_text: &str) -> anyhow::Result<CstFile> {
        let current_text = self.to_text();

        // Collect into owned Strings so we can mutate one element freely
        // without any borrowing or memory-management complications.
        let mut lines: Vec<String> = current_text
            .split_inclusive('\n')
            .map(String::from)
            .collect();

        let idx = node_id as usize;
        if idx >= lines.len() {
            anyhow::bail!(
                "node_id {} is out of range (file has {} lines)",
                node_id,
                lines.len()
            );
        }

        // Preserve the trailing newline of the original line so that lines
        // below it are not shifted.
        let original_had_newline = lines[idx].ends_with('\n');
        lines[idx] = if original_had_newline && !new_text.ends_with('\n') {
            format!("{new_text}\n")
        } else {
            new_text.to_owned()
        };

        let new_content = lines.concat();
        let root = build_tree(&new_content);

        Ok(CstFile {
            path: self.path.clone(),
            root,
            version: self.version + 1,
        })
    }
}

// ---------------------------------------------------------------------------
// Internal builder
// ---------------------------------------------------------------------------

/// Build a rowan green tree from raw file text using a line-oriented grammar.
///
/// The resulting tree is:
/// ```text
/// Root
///   Line
///     Text("first line\n")
///   Line
///     Text("second line\n")
///   ...
/// ```
fn build_tree(content: &str) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(rowan::SyntaxKind(SyntaxKind::Root as u16));

    for line in content.split_inclusive('\n') {
        builder.start_node(rowan::SyntaxKind(SyntaxKind::Line as u16));
        builder.token(rowan::SyntaxKind(SyntaxKind::Text as u16), line);
        builder.finish_node();
    }

    builder.finish_node();
    builder.finish()
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: &str = "fn main() {\n    println!(\"hello\");\n}\n";

    fn sample_file() -> CstFile {
        CstFile::parse(PathBuf::from("test.rs"), CONTENT)
    }

    #[test]
    fn parse_roundtrip() {
        let file = sample_file();
        assert_eq!(file.to_text(), CONTENT);
    }

    #[test]
    fn initial_version_is_zero() {
        assert_eq!(sample_file().version, 0);
    }

    #[test]
    fn tree_skeleton_line_count() {
        let file = sample_file();
        let nodes = file.tree_skeleton();
        // CONTENT has 3 lines (each ends with \n)
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn tree_skeleton_spans_are_contiguous() {
        let file = sample_file();
        let nodes = file.tree_skeleton();
        // Each node's span_end should equal the next node's span_start.
        for window in nodes.windows(2) {
            assert_eq!(window[0].span_end, window[1].span_start);
        }
        // Last node's span_end should equal the total byte length.
        let last = nodes.last().unwrap();
        assert_eq!(last.span_end as usize, CONTENT.len());
    }

    #[test]
    fn get_node_returns_correct_line() {
        let file = sample_file();
        let info = file.get_node(1).unwrap();
        assert_eq!(info.node_id, 1);
        assert_eq!(info.kind, "Line");
        assert_eq!(info.text, "    println!(\"hello\");\n");
    }

    #[test]
    fn get_node_out_of_range() {
        let file = sample_file();
        assert!(file.get_node(99).is_err());
    }

    #[test]
    fn text_preview_strips_newline_and_truncates() {
        let file = sample_file();
        let info = file.get_node(0).unwrap();
        let preview = info.text_preview();
        assert!(!preview.ends_with('\n'));
        assert!(preview.len() <= 80);
    }

    #[test]
    fn replace_node_increments_version() {
        let file = sample_file();
        let updated = file.replace_node(1, "    println!(\"world\");").unwrap();
        assert_eq!(updated.version, 1);
    }

    #[test]
    fn replace_node_preserves_other_lines() {
        let file = sample_file();
        let updated = file.replace_node(1, "    println!(\"world\");").unwrap();
        let text = updated.to_text();
        assert!(text.starts_with("fn main() {\n"));
        assert!(text.contains("println!(\"world\");"));
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn replace_node_out_of_range() {
        let file = sample_file();
        assert!(file.replace_node(99, "x").is_err());
    }

    #[test]
    fn empty_file_produces_no_line_nodes() {
        let file = CstFile::parse(PathBuf::from("empty.rs"), "");
        // split_inclusive on an empty string yields no segments.
        assert_eq!(file.tree_skeleton().len(), 0);
    }
}
