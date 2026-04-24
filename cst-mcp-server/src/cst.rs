use std::path::{Path, PathBuf};

use rowan::{GreenNode, GreenNodeBuilder, GreenToken, Language, NodeOrToken, SyntaxNode};
use serde::{Deserialize, Serialize};

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
    /// Uses rowan's native `replace_child` to surgically swap only the
    /// affected Line green node in the tree, leaving all sibling nodes shared
    /// (O(1) clone via `Arc`).  Returns a new `CstFile` with an incremented
    /// version on success.
    pub fn replace_node(&self, node_id: NodeId, new_text: &str) -> anyhow::Result<CstFile> {
        let idx = node_id as usize;
        let existing_count = self.root.children().count();

        if idx >= existing_count {
            anyhow::bail!(
                "node_id {} is out of range (file has {} line nodes)",
                node_id,
                existing_count
            );
        }

        // Preserve the trailing newline of the original line so that the
        // lines below it are not shifted.
        let original_text = self.get_node(node_id)?.text;
        let effective: std::borrow::Cow<'_, str> =
            if original_text.ends_with('\n') && !new_text.ends_with('\n') {
                std::borrow::Cow::Owned(format!("{new_text}\n"))
            } else {
                std::borrow::Cow::Borrowed(new_text)
            };

        // Build only the replacement Line node — all other nodes are shared.
        let new_line = build_line_node(&effective, self.language);
        let new_root = self
            .root
            .replace_child(idx, NodeOrToken::Node(new_line));

        Ok(CstFile {
            path: self.path.clone(),
            root: new_root,
            version: self.version + 1,
            language: self.language,
        })
    }

    /// Insert one or more new lines into the file's CST.
    ///
    /// `insert_after` is the 0-based index of the line *after which* the new
    /// lines are inserted.  Pass `None` to prepend at the beginning of the
    /// file.  Each string in `lines` becomes one new Line node; a trailing
    /// `\n` is appended automatically when missing.
    ///
    /// Uses rowan's `splice_children` to perform the insertion without
    /// re-parsing unchanged lines.  Returns a new `CstFile` with an
    /// incremented version on success.
    pub fn insert_lines(
        &self,
        insert_after: Option<NodeId>,
        lines: &[String],
    ) -> anyhow::Result<CstFile> {
        if lines.is_empty() {
            anyhow::bail!("lines must not be empty");
        }

        let existing_count = self.root.children().count();

        let insert_idx = match insert_after {
            None => 0,
            Some(id) => {
                let idx = id as usize;
                if idx >= existing_count {
                    anyhow::bail!(
                        "insert_after {} is out of range (file has {} line nodes)",
                        id,
                        existing_count
                    );
                }
                idx + 1
            }
        };

        let new_nodes: Vec<NodeOrToken<GreenNode, GreenToken>> = lines
            .iter()
            .map(|text| {
                let effective: std::borrow::Cow<'_, str> = if text.ends_with('\n') {
                    std::borrow::Cow::Borrowed(text.as_str())
                } else {
                    std::borrow::Cow::Owned(format!("{text}\n"))
                };
                NodeOrToken::Node(build_line_node(&effective, self.language))
            })
            .collect();

        let new_root = self
            .root
            .splice_children(insert_idx..insert_idx, new_nodes);

        Ok(CstFile {
            path: self.path.clone(),
            root: new_root,
            version: self.version + 1,
            language: self.language,
        })
    }

    /// Delete `count` consecutive Line nodes starting at `node_id`.
    ///
    /// Uses rowan's `splice_children` to perform the deletion without
    /// re-parsing unchanged lines.  Returns a new `CstFile` with an
    /// incremented version on success.
    pub fn delete_lines(&self, node_id: NodeId, count: u32) -> anyhow::Result<CstFile> {
        if count == 0 {
            anyhow::bail!("count must be at least 1");
        }

        let existing_count = self.root.children().count();
        let idx = node_id as usize;
        let end = idx + count as usize;

        if idx >= existing_count {
            anyhow::bail!(
                "node_id {} is out of range (file has {} line nodes)",
                node_id,
                existing_count
            );
        }
        if end > existing_count {
            anyhow::bail!(
                "delete range {}..{} exceeds file length ({} line nodes)",
                idx,
                end,
                existing_count
            );
        }

        let new_root = self.root.splice_children(
            idx..end,
            std::iter::empty::<NodeOrToken<GreenNode, GreenToken>>(),
        );

        Ok(CstFile {
            path: self.path.clone(),
            root: new_root,
            version: self.version + 1,
            language: self.language,
        })
    }
}

// ---------------------------------------------------------------------------
// Query — structured search across the CST
// ---------------------------------------------------------------------------

/// A structured query expression that filters Line nodes or individual tokens.
///
/// All filter fields are optional.  An expression with no fields matches
/// everything at the selected depth.  Multiple fields are AND-ed together.
///
/// ## Quick examples
///
/// All function definitions at the top level of a Rust file:
/// `{"semantic":"fn_def","scope_depth_max":0}`
///
/// All uses of a specific identifier anywhere in the file:
/// `{"identifier_name":"my_var"}`
///
/// Lines containing a TODO comment:
/// `{"text_contains":"TODO"}`
///
/// All keyword tokens on lines 0–20:
/// `{"depth":"token","kind":"Keyword","node_id_to":20}`
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct QueryExpr {
    // ── text / kind filters ──────────────────────────────────────────────────

    /// Filter by kind name (case-insensitive).
    ///
    /// At `depth="line"` the only valid value is `"Line"`.
    /// At `depth="token"` valid values for Rust files are: `"Keyword"`,
    /// `"Identifier"`, `"Literal"`, `"Comment"`, `"Whitespace"`,
    /// `"Punctuation"`, `"Newline"`.  For plain files: `"Text"`.
    pub kind: Option<String>,

    /// Require the line/token text to contain this substring (case-sensitive).
    pub text_contains: Option<String>,

    /// Require the line/token text to match this glob pattern.
    ///
    /// `*` matches any sequence of characters (including none).
    /// `?` matches exactly one character.
    /// Neither metacharacter restricts path separators — this is text matching.
    pub text_glob: Option<String>,

    // ── line-range filter ────────────────────────────────────────────────────

    /// Only consider lines whose 0-based `node_id` is ≥ this value.
    pub node_id_from: Option<u32>,

    /// Only consider lines whose 0-based `node_id` is ≤ this value (inclusive).
    pub node_id_to: Option<u32>,

    // ── depth ────────────────────────────────────────────────────────────────

    /// The level at which to match: `"line"` (default) or `"token"`.
    ///
    /// `"line"` — each Line node is tested as a whole; results include the
    /// full line text.
    ///
    /// `"token"` — each token inside every Line node is tested individually;
    /// results include the token text and `token_idx`.
    ///
    /// Overridden to token-depth when `identifier_name` is set; overridden to
    /// line-depth (with capture) when `semantic` is set.
    pub depth: Option<String>,

    // ── code-native (semantic) patterns ─────────────────────────────────────

    /// Match lines that contain a specific syntactic construct (Rust files only).
    ///
    /// | Value | What it matches | `capture` field |
    /// |---|---|---|
    /// | `"fn_def"` | `fn <name>(…)` | function name |
    /// | `"struct_def"` | `struct <name>` | struct name |
    /// | `"enum_def"` | `enum <name>` | enum name |
    /// | `"trait_def"` | `trait <name>` | trait name |
    /// | `"impl_block"` | `impl [Trait for] Type` | first identifier after `impl` |
    /// | `"type_def"` | `type <name> =` | alias name |
    /// | `"variable_def"` | `let [mut] <name>` / `const <name>` / `static [mut] <name>` | variable name |
    /// | `"use_stmt"` | `use <path>;` | the imported path |
    /// | `"macro_call"` | `<name>!(…)` | macro name |
    ///
    /// This field implies line-depth output even when `depth="token"` is set.
    /// Each matching line produces one result with a non-null `capture`.
    /// Can be combined with `scope_depth_min`/`scope_depth_max` to restrict
    /// to a particular nesting level (e.g. only top-level `fn` definitions).
    pub semantic: Option<String>,

    /// Find every occurrence of a specific identifier by exact name.
    ///
    /// Operates at token depth — each matching `Identifier` token becomes its
    /// own result entry with a `token_idx`.  Can be combined with
    /// `scope_depth_min`/`scope_depth_max` or `node_id_from`/`node_id_to` to
    /// narrow the search to a particular scope or region of the file.
    pub identifier_name: Option<String>,

    // ── scope-depth filters ──────────────────────────────────────────────────

    /// Only include results where the brace-nesting depth at the line's start
    /// is ≥ this value.  Depth 0 = top-level code, 1 = inside one `{…}`
    /// block, and so on.
    ///
    /// Depth is computed by counting `{` and `}` `Punctuation` tokens as we
    /// scan the file.  Because punctuation tokens are never generated inside
    /// string literals or comments (those are whole `Literal`/`Comment`
    /// tokens), depth is accurate for typical Rust code.  Multi-line string
    /// literals spanning more than one source line are a known limitation.
    pub scope_depth_min: Option<u32>,

    /// Only include results where the brace-nesting depth at the line's start
    /// is ≤ this value.
    pub scope_depth_max: Option<u32>,
}

/// A single item returned by [`CstFile::query`].
#[derive(Debug, Clone, Serialize)]
pub struct QueryMatch {
    /// 0-based index of the Line node that produced this match.
    pub node_id: u32,
    /// Kind label: `"Line"` for line-depth matches, or a token kind name
    /// (`"Keyword"`, `"Identifier"`, …) for token-depth matches.
    pub kind: String,
    /// Full text of the matched line or token (including any trailing `\n`).
    pub text: String,
    /// Byte offset of the first character within the file.
    pub span_start: u32,
    /// Byte offset one past the last character of this match.
    pub span_end: u32,
    /// For token-depth matches: 0-based index of the token within its Line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_idx: Option<u32>,
    /// For semantic pattern matches: the primary name extracted from the
    /// matched construct (function name, variable name, import path, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture: Option<String>,
    /// Heuristic brace-nesting depth at the start of this line.
    /// 0 = top-level code, 1 = inside one `{…}` block, etc.
    pub scope_depth: u32,
}

// ---------------------------------------------------------------------------
// CstFile::query — extension impl
// ---------------------------------------------------------------------------

impl CstFile {
    /// Run `expr` against this file's CST and return all matching items in
    /// document order.
    ///
    /// See [`QueryExpr`] for the full description of each filter field.
    /// Filters are applied in the following order:
    ///
    /// 1. `node_id_from` / `node_id_to` — skip lines outside the range.
    /// 2. `scope_depth_min` / `scope_depth_max` — skip lines at the wrong
    ///    brace-nesting depth.
    /// 3. `semantic` (line-depth, returns capture) *or* `identifier_name`
    ///    (token-depth, exact identifier) *or* generic `kind`/`text_*`/`depth`
    ///    filters.
    pub fn query(&self, expr: &QueryExpr) -> Vec<QueryMatch> {
        let root = LangSyntaxNode::new_root(self.root.clone());
        let mut results = Vec::new();
        let mut depth: u32 = 0; // heuristic brace-nesting depth

        let is_semantic = expr.semantic.is_some();
        let is_ident = expr.identifier_name.is_some();
        // token_level is true for generic token searches and for identifier_name;
        // overridden by is_semantic (always line-level).
        let token_level = !is_semantic
            && (is_ident || expr.depth.as_deref() == Some("token"));

        for (line_idx, line_node) in root.children().enumerate() {
            let node_id = line_idx as u32;

            // Collect (kind, text, span_start, span_end) for all tokens in
            // this line.  Needed for semantic matching, scope updates, and
            // token-level filtering.
            let raw_tokens: Vec<(String, String, u32, u32)> = line_node
                .children_with_tokens()
                .filter_map(|elem| {
                    if let NodeOrToken::Token(tok) = elem {
                        let r = tok.text_range();
                        Some((
                            tok.kind().as_str().to_owned(),
                            tok.text().to_owned(),
                            u32::from(r.start()),
                            u32::from(r.end()),
                        ))
                    } else {
                        None
                    }
                })
                .collect();

            // Record the depth at the START of this line, then advance depth
            // by counting net `{`/`}` in the line's tokens.
            let line_depth = depth;
            for (kind, text, _, _) in &raw_tokens {
                if kind == "Punctuation" {
                    match text.as_str() {
                        "{" => depth += 1,
                        "}" => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                }
            }

            // ── node_id range ───────────────────────────────────────────────
            if let Some(from) = expr.node_id_from {
                if node_id < from {
                    continue;
                }
            }
            if let Some(to) = expr.node_id_to {
                if node_id > to {
                    break;
                }
            }

            // ── scope-depth filter ──────────────────────────────────────────
            if let Some(min) = expr.scope_depth_min {
                if line_depth < min {
                    continue;
                }
            }
            if let Some(max) = expr.scope_depth_max {
                if line_depth > max {
                    continue;
                }
            }

            // ── semantic pattern search (line-depth, produces capture) ───────
            if is_semantic {
                let pattern = expr.semantic.as_deref().unwrap();
                if let Some(capture) =
                    detect_semantic_pattern(&raw_tokens, pattern)
                {
                    let line_text = line_node.text().to_string();
                    if basic_text_kind_matches(expr, "Line", &line_text) {
                        let lr = line_node.text_range();
                        results.push(QueryMatch {
                            node_id,
                            kind: "Line".to_owned(),
                            text: line_text,
                            span_start: u32::from(lr.start()),
                            span_end: u32::from(lr.end()),
                            token_idx: None,
                            capture: Some(capture),
                            scope_depth: line_depth,
                        });
                    }
                }
                continue; // semantic is always line-level — skip other paths
            }

            // ── identifier name search (token-depth) ─────────────────────────
            if is_ident {
                let target = expr.identifier_name.as_deref().unwrap();
                for (tidx, (kind, text, start, end)) in
                    raw_tokens.iter().enumerate()
                {
                    if kind == "Identifier" && text == target {
                        if basic_text_kind_matches(expr, kind, text) {
                            results.push(QueryMatch {
                                node_id,
                                kind: kind.clone(),
                                text: text.clone(),
                                span_start: *start,
                                span_end: *end,
                                token_idx: Some(tidx as u32),
                                capture: None,
                                scope_depth: line_depth,
                            });
                        }
                    }
                }
                continue; // identifier search is always token-level
            }

            // ── generic token-depth search ───────────────────────────────────
            if token_level {
                for (tidx, (kind, text, start, end)) in
                    raw_tokens.iter().enumerate()
                {
                    if basic_text_kind_matches(expr, kind, text) {
                        results.push(QueryMatch {
                            node_id,
                            kind: kind.clone(),
                            text: text.clone(),
                            span_start: *start,
                            span_end: *end,
                            token_idx: Some(tidx as u32),
                            capture: None,
                            scope_depth: line_depth,
                        });
                    }
                }
            } else {
                // ── generic line-depth search ────────────────────────────────
                let line_text = line_node.text().to_string();
                if basic_text_kind_matches(expr, "Line", &line_text) {
                    let lr = line_node.text_range();
                    results.push(QueryMatch {
                        node_id,
                        kind: "Line".to_owned(),
                        text: line_text,
                        span_start: u32::from(lr.start()),
                        span_end: u32::from(lr.end()),
                        token_idx: None,
                        capture: None,
                        scope_depth: line_depth,
                    });
                }
            }
        }

        results
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Check whether `kind` and `text` satisfy the text/kind filter fields of
/// `expr`.  Does **not** check `semantic`, `identifier_name`, `node_id_*`,
/// or `scope_*` — those are handled directly in [`CstFile::query`].
fn basic_text_kind_matches(expr: &QueryExpr, kind: &str, text: &str) -> bool {
    if let Some(ref k) = expr.kind {
        if !k.eq_ignore_ascii_case(kind) {
            return false;
        }
    }
    if let Some(ref needle) = expr.text_contains {
        if !text.contains(needle.as_str()) {
            return false;
        }
    }
    if let Some(ref pattern) = expr.text_glob {
        if !text_glob_matches(pattern, text) {
            return false;
        }
    }
    true
}

/// Detect a named semantic pattern in a line's raw token list and return the
/// captured name on success, or `None` when the pattern does not match.
///
/// `tokens` is the raw `(kind_name, text, span_start, span_end)` vector for
/// the whole line, including whitespace and newline tokens.
fn detect_semantic_pattern(
    tokens: &[(String, String, u32, u32)],
    pattern: &str,
) -> Option<String> {
    // Build a compact, whitespace-free view for easier positional matching.
    let code: Vec<(&str, &str)> = tokens
        .iter()
        .filter(|(k, _, _, _)| k != "Whitespace" && k != "Newline")
        .map(|(k, t, _, _)| (k.as_str(), t.as_str()))
        .collect();

    match pattern {
        "fn_def"     => keyword_then_ident(&code, "fn"),
        "struct_def" => keyword_then_ident(&code, "struct"),
        "enum_def"   => keyword_then_ident(&code, "enum"),
        "trait_def"  => keyword_then_ident(&code, "trait"),
        "type_def"   => keyword_then_ident(&code, "type"),

        // impl [Generics for] Type  →  first Identifier after `impl`
        "impl_block" => keyword_then_ident(&code, "impl"),

        // use <path>;  →  everything between `use` and `;`
        "use_stmt" => {
            if code.first().map(|&(k, t)| k == "Keyword" && t == "use") != Some(true) {
                return None;
            }
            let path: String = code[1..]
                .iter()
                .take_while(|&&(_, t)| t != ";")
                .map(|&(_, t)| t)
                .collect::<Vec<_>>()
                .join("");
            let trimmed = path.trim().to_owned();
            if trimmed.is_empty() { None } else { Some(trimmed) }
        }

        // let [mut] <name>  /  const <name>  /  static [mut] <name>
        "variable_def" => {
            let &(first_k, first_t) = code.first()?;
            if first_k != "Keyword"
                || !matches!(first_t, "let" | "const" | "static")
            {
                return None;
            }
            let mut iter = code[1..].iter().peekable();
            // Skip one optional `mut` keyword.
            if iter.peek().map(|&&(k, t)| k == "Keyword" && t == "mut") == Some(true) {
                iter.next();
            }
            iter.find(|&&(k, _)| k == "Identifier")
                .map(|&(_, t)| t.to_owned())
        }

        // <name>!(…)
        "macro_call" => {
            for i in 0..code.len().saturating_sub(1) {
                let (k0, t0) = code[i];
                let (k1, t1) = code[i + 1];
                if k0 == "Identifier" && k1 == "Punctuation" && t1 == "!" {
                    return Some(t0.to_owned());
                }
            }
            None
        }

        _ => None,
    }
}

/// Return the text of the first `Identifier` token that appears after the
/// given `keyword` in a whitespace-stripped token list.
fn keyword_then_ident(code: &[(&str, &str)], keyword: &str) -> Option<String> {
    let pos = code
        .iter()
        .position(|&(k, t)| k == "Keyword" && t == keyword)?;
    code[pos + 1..]
        .iter()
        .find(|&&(k, _)| k == "Identifier")
        .map(|&(_, t)| t.to_owned())
}

/// Glob matching for arbitrary text (no path-separator semantics).
///
/// - `*` matches any sequence of characters (including empty).
/// - `?` matches exactly one character.
fn text_glob_matches(pattern: &str, text: &str) -> bool {
    text_glob_inner(pattern.as_bytes(), text.as_bytes())
}

fn text_glob_inner(pat: &[u8], txt: &[u8]) -> bool {
    match pat.split_first() {
        None => txt.is_empty(),
        Some((&b'*', rest)) => {
            // Collapse consecutive `*`s for efficiency.
            let mut rest = rest;
            while rest.first() == Some(&b'*') {
                rest = &rest[1..];
            }
            for i in 0..=txt.len() {
                if text_glob_inner(rest, &txt[i..]) {
                    return true;
                }
            }
            false
        }
        Some((&b'?', rest)) => match txt.split_first() {
            Some((_, rest_t)) => text_glob_inner(rest, rest_t),
            None => false,
        },
        Some((&pc, rest_p)) => match txt.split_first() {
            Some((&tc, rest_t)) if tc == pc => text_glob_inner(rest_p, rest_t),
            _ => false,
        },
    }
}
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

/// Build a single rowan Line green node from raw line text.
///
/// Uses the language-appropriate lexer so the resulting Line node has the
/// same token-level structure as lines built by `build_tree`.  A trailing
/// `\n` must be included in `text` if required.
///
/// The builder's root is the Line node itself — callers can pass the result
/// directly to `GreenNodeData::replace_child` / `splice_children`.
fn build_line_node(text: &str, language: FileLanguage) -> GreenNode {
    let mut b = GreenNodeBuilder::new();
    let raw = |k: SyntaxKind| rowan::SyntaxKind(k as u16);

    b.start_node(raw(SyntaxKind::Line));
    match language {
        FileLanguage::Rust => {
            for tok in lexer::lex_rust(text) {
                b.token(raw(token_kind_to_syntax(tok.kind)), tok.text);
            }
        }
        FileLanguage::Plain => {
            b.token(raw(SyntaxKind::Text), text);
        }
    }
    b.finish_node();
    b.finish()
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

    // --- insert_lines ---

    #[test]
    fn insert_lines_at_beginning() {
        let file = sample_file();
        let inserted = file
            .insert_lines(None, &["// new first line\n".to_owned()])
            .unwrap();
        assert_eq!(inserted.version, 1);
        assert_eq!(inserted.tree_skeleton().len(), 4);
        assert_eq!(inserted.get_node(0).unwrap().text, "// new first line\n");
        // Original lines shifted down by one.
        assert_eq!(inserted.get_node(1).unwrap().text, "fn main() {\n");
    }

    #[test]
    fn insert_lines_after_last() {
        let file = sample_file(); // 3 lines, last idx = 2
        let inserted = file
            .insert_lines(Some(2), &["// appended\n".to_owned()])
            .unwrap();
        assert_eq!(inserted.tree_skeleton().len(), 4);
        assert_eq!(inserted.get_node(3).unwrap().text, "// appended\n");
    }

    #[test]
    fn insert_lines_in_middle() {
        let file = sample_file(); // lines: 0=fn, 1=println, 2=}
        let inserted = file
            .insert_lines(Some(0), &["    // inserted\n".to_owned()])
            .unwrap();
        assert_eq!(inserted.tree_skeleton().len(), 4);
        assert_eq!(inserted.get_node(0).unwrap().text, "fn main() {\n");
        assert_eq!(inserted.get_node(1).unwrap().text, "    // inserted\n");
        assert_eq!(inserted.get_node(2).unwrap().text, "    println!(\"hello\");\n");
    }

    #[test]
    fn insert_lines_auto_appends_newline() {
        let file = sample_file();
        // Text without trailing \n → server must add one.
        let inserted = file
            .insert_lines(None, &["// no newline".to_owned()])
            .unwrap();
        assert!(
            inserted.get_node(0).unwrap().text.ends_with('\n'),
            "inserted line must end with \\n"
        );
    }

    #[test]
    fn insert_lines_multiple() {
        let file = sample_file();
        let lines = vec!["// a\n".to_owned(), "// b\n".to_owned()];
        let inserted = file.insert_lines(Some(0), &lines).unwrap();
        assert_eq!(inserted.tree_skeleton().len(), 5);
        assert_eq!(inserted.get_node(1).unwrap().text, "// a\n");
        assert_eq!(inserted.get_node(2).unwrap().text, "// b\n");
    }

    #[test]
    fn insert_lines_roundtrip() {
        let file = sample_file();
        let lines = vec!["    let z = 99;\n".to_owned()];
        let inserted = file.insert_lines(Some(0), &lines).unwrap();
        let text = inserted.to_text();
        assert_eq!(
            text,
            "fn main() {\n    let z = 99;\n    println!(\"hello\");\n}\n"
        );
    }

    #[test]
    fn insert_lines_out_of_range() {
        let file = sample_file();
        assert!(file.insert_lines(Some(99), &["x\n".to_owned()]).is_err());
    }

    #[test]
    fn insert_lines_empty_input() {
        let file = sample_file();
        assert!(file.insert_lines(None, &[]).is_err());
    }

    #[test]
    fn insert_lines_preserves_language() {
        let file = sample_file();
        let inserted = file
            .insert_lines(Some(0), &["    let y = 2;\n".to_owned()])
            .unwrap();
        assert_eq!(inserted.language(), FileLanguage::Rust);
        let tokens = inserted.get_line_tokens(1).unwrap();
        assert!(tokens.iter().any(|t| t.kind == "Keyword" && t.text == "let"));
    }

    // --- delete_lines ---

    #[test]
    fn delete_single_line() {
        let file = sample_file(); // lines: fn / println / }
        let deleted = file.delete_lines(1, 1).unwrap();
        assert_eq!(deleted.version, 1);
        assert_eq!(deleted.tree_skeleton().len(), 2);
        assert_eq!(deleted.get_node(0).unwrap().text, "fn main() {\n");
        assert_eq!(deleted.get_node(1).unwrap().text, "}\n");
    }

    #[test]
    fn delete_first_line() {
        let file = sample_file();
        let deleted = file.delete_lines(0, 1).unwrap();
        assert_eq!(deleted.tree_skeleton().len(), 2);
        assert_eq!(deleted.get_node(0).unwrap().text, "    println!(\"hello\");\n");
    }

    #[test]
    fn delete_last_line() {
        let file = sample_file();
        let deleted = file.delete_lines(2, 1).unwrap();
        assert_eq!(deleted.tree_skeleton().len(), 2);
        assert_eq!(deleted.get_node(1).unwrap().text, "    println!(\"hello\");\n");
    }

    #[test]
    fn delete_all_lines() {
        let file = sample_file();
        let deleted = file.delete_lines(0, 3).unwrap();
        assert_eq!(deleted.tree_skeleton().len(), 0);
        assert_eq!(deleted.to_text(), "");
    }

    #[test]
    fn delete_multiple_lines_roundtrip() {
        // File: fn / println / } — delete lines 0 and 1 (keep only })
        let file = sample_file();
        let deleted = file.delete_lines(0, 2).unwrap();
        assert_eq!(deleted.to_text(), "}\n");
    }

    #[test]
    fn delete_lines_out_of_range_start() {
        let file = sample_file();
        assert!(file.delete_lines(99, 1).is_err());
    }

    #[test]
    fn delete_lines_exceeds_length() {
        let file = sample_file(); // 3 lines, start at 2 with count 2 → end index 4 > 3
        assert!(file.delete_lines(2, 2).is_err());
    }

    #[test]
    fn delete_lines_zero_count() {
        let file = sample_file();
        assert!(file.delete_lines(0, 0).is_err());
    }

    #[test]
    fn delete_preserves_language() {
        let file = sample_file();
        let deleted = file.delete_lines(1, 1).unwrap();
        assert_eq!(deleted.language(), FileLanguage::Rust);
    }

    // --- surgical rebuild correctness ---

    #[test]
    fn replace_node_is_surgical_lossless() {
        let file = sample_file();
        let updated = file.replace_node(1, "    let x = 42;").unwrap();
        // Verify full round-trip text.
        assert_eq!(
            updated.to_text(),
            "fn main() {\n    let x = 42;\n}\n"
        );
        // Verify token kinds on edited line.
        let tokens = updated.get_line_tokens(1).unwrap();
        assert!(tokens.iter().any(|t| t.kind == "Keyword" && t.text == "let"));
    }

    #[test]
    fn successive_mutations_increment_version() {
        let file = sample_file();
        let v1 = file.replace_node(0, "fn foo() {").unwrap();
        let v2 = v1
            .insert_lines(Some(0), &["    // comment\n".to_owned()])
            .unwrap();
        let v3 = v2.delete_lines(2, 1).unwrap();
        assert_eq!(file.version, 0);
        assert_eq!(v1.version, 1);
        assert_eq!(v2.version, 2);
        assert_eq!(v3.version, 3);
    }

    // ── query: basic text / kind filters ────────────────────────────────────

    fn no_filter() -> QueryExpr {
        QueryExpr {
            kind: None,
            text_contains: None,
            text_glob: None,
            node_id_from: None,
            node_id_to: None,
            depth: None,
            semantic: None,
            identifier_name: None,
            scope_depth_min: None,
            scope_depth_max: None,
        }
    }

    #[test]
    fn query_no_filter_returns_all_lines() {
        let file = sample_file();
        let matches = file.query(&no_filter());
        // CONTENT has 3 lines
        assert_eq!(matches.len(), 3);
        assert!(matches.iter().all(|m| m.kind == "Line"));
    }

    #[test]
    fn query_line_by_text_contains() {
        let file = sample_file();
        let expr = QueryExpr {
            text_contains: Some("println".to_owned()),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].text.contains("println"));
    }

    #[test]
    fn query_line_by_text_glob() {
        let file = sample_file();
        let expr = QueryExpr {
            text_glob: Some("fn *".to_owned()),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].text.starts_with("fn "));
    }

    #[test]
    fn query_line_by_node_id_range() {
        let file = sample_file();
        let expr = QueryExpr {
            node_id_from: Some(1),
            node_id_to: Some(1),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].node_id, 1);
    }

    #[test]
    fn query_no_matches_returns_empty() {
        let file = sample_file();
        let expr = QueryExpr {
            text_contains: Some("XYZNOTHERE".to_owned()),
            ..no_filter()
        };
        assert!(file.query(&expr).is_empty());
    }

    // ── query: token-depth ───────────────────────────────────────────────────

    #[test]
    fn query_token_by_kind_keyword() {
        let file = sample_file();
        let expr = QueryExpr {
            depth: Some("token".to_owned()),
            kind: Some("Keyword".to_owned()),
            ..no_filter()
        };
        let matches = file.query(&expr);
        // "fn main() {\n" contains `fn`; body contains none; "}" contains none
        assert!(!matches.is_empty());
        assert!(matches.iter().all(|m| m.kind == "Keyword"));
        assert!(matches.iter().all(|m| m.token_idx.is_some()));
    }

    #[test]
    fn query_identifier_name() {
        let file = sample_file();
        let expr = QueryExpr {
            identifier_name: Some("main".to_owned()),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].text, "main");
        assert_eq!(matches[0].kind, "Identifier");
        assert_eq!(matches[0].node_id, 0); // on first line
    }

    // ── query: semantic patterns ─────────────────────────────────────────────

    #[test]
    fn query_semantic_fn_def() {
        let file = sample_file();
        let expr = QueryExpr {
            semantic: Some("fn_def".to_owned()),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].capture.as_deref(), Some("main"));
        assert_eq!(matches[0].node_id, 0);
    }

    #[test]
    fn query_semantic_macro_call() {
        let file = sample_file();
        let expr = QueryExpr {
            semantic: Some("macro_call".to_owned()),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].capture.as_deref(), Some("println"));
    }

    #[test]
    fn query_semantic_variable_def() {
        let src = "fn f() {\n    let mut x = 1;\n    const Y: u32 = 2;\n    static Z: i32 = 0;\n}\n";
        let file = CstFile::parse(PathBuf::from("v.rs"), src);
        let expr = QueryExpr {
            semantic: Some("variable_def".to_owned()),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 3);
        let names: Vec<_> = matches.iter().map(|m| m.capture.as_deref().unwrap()).collect();
        assert!(names.contains(&"x"));
        assert!(names.contains(&"Y"));
        assert!(names.contains(&"Z"));
    }

    // ── query: scope depth ───────────────────────────────────────────────────

    #[test]
    fn query_scope_depth_top_level_only() {
        let file = sample_file();
        // Only matches lines at brace depth 0 (top-level).
        let expr = QueryExpr {
            scope_depth_max: Some(0),
            ..no_filter()
        };
        let matches = file.query(&expr);
        // Line 0: "fn main() {\n" is at depth 0 ✓
        // Line 1: "    println!...\n" is at depth 1 ✗
        // Line 2: "}\n" is at depth 1 at its start ✗ (depth increases after `{`)
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].node_id, 0);
    }

    #[test]
    fn query_scope_depth_inside_block() {
        let file = sample_file();
        // Only matches lines inside at least one block.
        let expr = QueryExpr {
            scope_depth_min: Some(1),
            ..no_filter()
        };
        let matches = file.query(&expr);
        // Lines 1 and 2 are at depth ≥ 1.
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn query_semantic_fn_def_at_top_level_only() {
        // Two functions: one top-level, one nested (unusual but valid Rust).
        let src = "fn outer() {\n    fn inner() {}\n}\n";
        let file = CstFile::parse(PathBuf::from("nested.rs"), src);
        let expr = QueryExpr {
            semantic: Some("fn_def".to_owned()),
            scope_depth_max: Some(0),
            ..no_filter()
        };
        let matches = file.query(&expr);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].capture.as_deref(), Some("outer"));
    }
}
