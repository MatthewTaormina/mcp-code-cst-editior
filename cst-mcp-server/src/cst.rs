use std::path::{Path, PathBuf};

use rowan::{GreenNode, GreenNodeBuilder, Language, NodeOrToken, SyntaxNode};
use serde::Serialize;

use crate::lexer::{self, TokenKind};

// ---------------------------------------------------------------------------
// SyntaxKind — the discriminant enum for every node/token in our generic CST.
// ---------------------------------------------------------------------------

/// Every node and token in the CST has one of these kinds.
///
/// Variants 0–3 are structural (Root, Line, plain Text, Error).
/// Variants 4–10 are token-level kinds emitted by language-specific lexers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    /// The root node — wraps the entire file.
    Root = 0,
    /// One logical line of text (including its trailing `\n`, if any).
    Line = 1,
    /// Raw text token used by the plain-text (non-language-aware) grammar.
    Text = 2,
    /// Catch-all for anything not otherwise classified.
    Error = 3,

    // -----------------------------------------------------------------------
    // Token-level kinds (emitted by language-specific lexers)
    // -----------------------------------------------------------------------
    /// A reserved keyword (`fn`, `let`, `pub`, …).
    Keyword = 4,
    /// An identifier.
    Identifier = 5,
    /// A string, char, byte-string, raw-string, or numeric literal.
    Literal = 6,
    /// A line comment or block comment.
    Comment = 7,
    /// Horizontal whitespace (spaces / tabs, not newlines).
    Whitespace = 8,
    /// Punctuation, operators, or any other single character.
    Punctuation = 9,
    /// A single newline character (`\n`).
    Newline = 10,
}

impl SyntaxKind {
    /// Return a human-readable name for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            SyntaxKind::Root => "Root",
            SyntaxKind::Line => "Line",
            SyntaxKind::Text => "Text",
            SyntaxKind::Error => "Error",
            SyntaxKind::Keyword => "Keyword",
            SyntaxKind::Identifier => "Identifier",
            SyntaxKind::Literal => "Literal",
            SyntaxKind::Comment => "Comment",
            SyntaxKind::Whitespace => "Whitespace",
            SyntaxKind::Punctuation => "Punctuation",
            SyntaxKind::Newline => "Newline",
        }
    }
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
            4 => SyntaxKind::Keyword,
            5 => SyntaxKind::Identifier,
            6 => SyntaxKind::Literal,
            7 => SyntaxKind::Comment,
            8 => SyntaxKind::Whitespace,
            9 => SyntaxKind::Punctuation,
            10 => SyntaxKind::Newline,
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
// FileLanguage — selects the lexer for a file
// ---------------------------------------------------------------------------

/// The grammar/lexer to use when building the CST for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLanguage {
    /// Rust source code: sub-line token-level grammar.
    Rust,
    /// Everything else: plain line-oriented grammar (one `Text` token per line).
    Plain,
}

impl FileLanguage {
    /// Infer the file language from a file path extension.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => FileLanguage::Rust,
            _ => FileLanguage::Plain,
        }
    }
}

// ---------------------------------------------------------------------------
// NodeInfo — rich metadata for a single Line node
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
// TokenInfo — metadata for a single token within a Line node
// ---------------------------------------------------------------------------

/// Serialisable metadata describing a single token inside a Line node.
#[derive(Debug, Clone, Serialize)]
pub struct TokenInfo {
    /// 0-based index of this token among its Line node's children.
    pub token_idx: u32,
    /// Human-readable kind label (e.g. `"Keyword"`, `"Identifier"`, …).
    pub kind: &'static str,
    /// The exact text of this token (lossless).
    pub text: String,
    /// Byte offset of the first character within the file.
    pub span_start: u32,
    /// Byte offset one past the last character.
    pub span_end: u32,
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
    /// Which lexer was used to build `root`.
    language: FileLanguage,
}

impl CstFile {
    /// Parse `content` into a CST using the lexer appropriate for `path`'s
    /// file extension.  `.rs` files get a token-level Rust grammar; all
    /// others get the plain line-oriented grammar.
    pub fn parse(path: PathBuf, content: &str) -> Self {
        let language = FileLanguage::from_path(&path);
        let root = build_tree(content, language);
        Self {
            path,
            root,
            version: 0,
            language,
        }
    }

    /// The language/lexer used for this file.
    pub fn language(&self) -> FileLanguage {
        self.language
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

    /// Return all token-level children of the Line node identified by
    /// `node_id`.
    ///
    /// For `.rs` files each token carries a semantic kind (Keyword,
    /// Identifier, Literal, …).  For plain-text files the single token is
    /// classified as `Text`.  Returns an error if `node_id` is out of range.
    pub fn get_line_tokens(&self, node_id: NodeId) -> anyhow::Result<Vec<TokenInfo>> {
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

        let line_node = &children[idx];
        let mut token_infos = Vec::new();
        let mut token_idx: u32 = 0;

        for elem in line_node.children_with_tokens() {
            if let NodeOrToken::Token(tok) = elem {
                let range = tok.text_range();
                let kind = tok.kind();
                token_infos.push(TokenInfo {
                    token_idx,
                    kind: kind.as_str(),
                    text: tok.text().to_owned(),
                    span_start: u32::from(range.start()),
                    span_end: u32::from(range.end()),
                });
                token_idx += 1;
            }
        }

        Ok(token_infos)
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
        let root = build_tree(&new_content, self.language);

        Ok(CstFile {
            path: self.path.clone(),
            root,
            version: self.version + 1,
            language: self.language,
        })
    }
}

// ---------------------------------------------------------------------------
// Internal builder
// ---------------------------------------------------------------------------

/// Map a `lexer::TokenKind` to the corresponding `SyntaxKind`.
fn token_kind_to_syntax(tk: TokenKind) -> SyntaxKind {
    match tk {
        TokenKind::Keyword => SyntaxKind::Keyword,
        TokenKind::Identifier => SyntaxKind::Identifier,
        TokenKind::Literal => SyntaxKind::Literal,
        TokenKind::Comment => SyntaxKind::Comment,
        TokenKind::Whitespace => SyntaxKind::Whitespace,
        TokenKind::Newline => SyntaxKind::Newline,
        TokenKind::Punctuation => SyntaxKind::Punctuation,
    }
}

/// Build a rowan green tree from raw file text.
///
/// When `language` is `Rust`, each Line node contains token-level children
/// (Keyword, Identifier, …, Newline).  When `language` is `Plain`, each Line
/// node contains a single `Text` token, producing the same line-oriented tree
/// as Phase 1/2.
///
/// In both cases the tree is lossless: `to_text()` reconstructs the original
/// content byte-for-byte.
fn build_tree(content: &str, language: FileLanguage) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    let raw = |k: SyntaxKind| rowan::SyntaxKind(k as u16);

    builder.start_node(raw(SyntaxKind::Root));

    match language {
        FileLanguage::Rust => {
            // Split into lines first; then lex each line individually.
            // `split_inclusive('\n')` keeps the '\n' attached to its line.
            for line in content.split_inclusive('\n') {
                builder.start_node(raw(SyntaxKind::Line));
                for token in lexer::lex_rust(line) {
                    let sk = token_kind_to_syntax(token.kind);
                    builder.token(raw(sk), token.text);
                }
                builder.finish_node();
            }
        }
        FileLanguage::Plain => {
            for line in content.split_inclusive('\n') {
                builder.start_node(raw(SyntaxKind::Line));
                builder.token(raw(SyntaxKind::Text), line);
                builder.finish_node();
            }
        }
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

    fn plain_file() -> CstFile {
        CstFile::parse(PathBuf::from("test.txt"), "line one\nline two\n")
    }

    // --- round-trip ---

    #[test]
    fn parse_roundtrip() {
        let file = sample_file();
        assert_eq!(file.to_text(), CONTENT);
    }

    #[test]
    fn parse_roundtrip_plain() {
        let file = plain_file();
        assert_eq!(file.to_text(), "line one\nline two\n");
    }

    // --- version ---

    #[test]
    fn initial_version_is_zero() {
        assert_eq!(sample_file().version, 0);
    }

    // --- language detection ---

    #[test]
    fn rs_extension_detected_as_rust() {
        let file = CstFile::parse(PathBuf::from("a.rs"), "");
        assert_eq!(file.language(), FileLanguage::Rust);
    }

    #[test]
    fn txt_extension_detected_as_plain() {
        let file = CstFile::parse(PathBuf::from("a.txt"), "");
        assert_eq!(file.language(), FileLanguage::Plain);
    }

    // --- tree skeleton ---

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
        for window in nodes.windows(2) {
            assert_eq!(window[0].span_end, window[1].span_start);
        }
        let last = nodes.last().unwrap();
        assert_eq!(last.span_end as usize, CONTENT.len());
    }

    // --- get_node ---

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

    // --- get_line_tokens ---

    #[test]
    fn get_line_tokens_rust_file() {
        let file = sample_file();
        // Line 0 is "fn main() {\n"
        let tokens = file.get_line_tokens(0).unwrap();
        let kinds: Vec<&str> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&"Keyword"), "should contain a Keyword token");
        assert!(kinds.contains(&"Identifier"), "should contain an Identifier token");
    }

    #[test]
    fn get_line_tokens_lossless() {
        let file = sample_file();
        let tokens = file.get_line_tokens(0).unwrap();
        let reconstructed: String = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(reconstructed, "fn main() {\n");
    }

    #[test]
    fn get_line_tokens_spans_are_within_line() {
        let file = sample_file();
        let line_info = file.get_node(0).unwrap();
        let tokens = file.get_line_tokens(0).unwrap();
        for tok in &tokens {
            assert!(tok.span_start >= line_info.span_start);
            assert!(tok.span_end <= line_info.span_end);
        }
    }

    #[test]
    fn get_line_tokens_out_of_range() {
        let file = sample_file();
        assert!(file.get_line_tokens(99).is_err());
    }

    #[test]
    fn get_line_tokens_plain_file() {
        let file = plain_file();
        let tokens = file.get_line_tokens(0).unwrap();
        // Plain grammar: one Text token + Newline
        assert!(!tokens.is_empty());
        let text: String = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(text, "line one\n");
    }

    // --- replace_node ---

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
    fn replace_node_preserves_language() {
        let file = sample_file();
        let updated = file.replace_node(0, "fn foo() {").unwrap();
        assert_eq!(updated.language(), FileLanguage::Rust);
        // Token-level structure preserved after edit.
        let tokens = updated.get_line_tokens(0).unwrap();
        assert!(tokens.iter().any(|t| t.kind == "Keyword" && t.text == "fn"));
    }

    // --- empty file ---

    #[test]
    fn empty_file_produces_no_line_nodes() {
        let file = CstFile::parse(PathBuf::from("empty.rs"), "");
        assert_eq!(file.tree_skeleton().len(), 0);
    }
}
