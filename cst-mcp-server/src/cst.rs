use std::path::PathBuf;

use rowan::{GreenNode, GreenNodeBuilder, Language, SyntaxNode};

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

    // Handle a trailing non-newline segment (last line without \n).
    // `split_inclusive` already handles this correctly, so no extra work needed.

    builder.finish_node();
    builder.finish()
}
