//! Language-specific lexers for the CST-MCP server.
//!
//! Each lexer produces a lossless sequence of `Token`s: concatenating every
//! token's `text` field reconstructs the original input verbatim.  This
//! property is required so that rowan's green tree can round-trip to text
//! without loss.

// ---------------------------------------------------------------------------
// TokenKind
// ---------------------------------------------------------------------------

/// Classification of a single lexical token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A reserved Rust keyword (`fn`, `let`, `pub`, …).
    Keyword,
    /// An identifier or lifetime.
    Identifier,
    /// A string, character, byte-string, raw-string, or numeric literal.
    Literal,
    /// A line comment (`// …`) or block comment (`/* … */`).
    Comment,
    /// Horizontal whitespace: one or more spaces or tabs (not newlines).
    Whitespace,
    /// A single newline character (`\n`).
    Newline,
    /// Everything else: punctuation, operators, or any unrecognised bytes.
    Punctuation,
}

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// A single lexed token referencing a slice of the original input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    /// The exact text of this token (a sub-slice of the original input).
    pub text: &'a str,
}

// ---------------------------------------------------------------------------
// Rust keyword table
// ---------------------------------------------------------------------------

/// Every reserved word in Rust (2021 edition).
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn",
    "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "union", "unsafe", "use", "where", "while",
];

// ---------------------------------------------------------------------------
// Rust lexer
// ---------------------------------------------------------------------------

/// Lex a slice of Rust source text into a lossless sequence of `Token`s.
///
/// The lexer is conservative by design: it does not parse nested block
/// comments, does not validate string escape sequences, and splits unknown
/// multi-byte sequences one codepoint at a time.  Its purpose is CST
/// navigation and display, not compilation.
pub fn lex_rust(input: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut rest = input;

    while !rest.is_empty() {
        // --- newline ---
        if rest.starts_with('\n') {
            tokens.push(Token { kind: TokenKind::Newline, text: &rest[..1] });
            rest = &rest[1..];
            continue;
        }

        // --- horizontal whitespace ---
        if rest.starts_with(|c: char| c == ' ' || c == '\t') {
            let end = rest
                .find(|c: char| c != ' ' && c != '\t')
                .unwrap_or(rest.len());
            tokens.push(Token { kind: TokenKind::Whitespace, text: &rest[..end] });
            rest = &rest[end..];
            continue;
        }

        // --- line comment ---
        if rest.starts_with("//") {
            // Consume up to (but not including) the newline.
            let end = rest.find('\n').unwrap_or(rest.len());
            tokens.push(Token { kind: TokenKind::Comment, text: &rest[..end] });
            rest = &rest[end..];
            continue;
        }

        // --- block comment (non-recursive, best-effort) ---
        if rest.starts_with("/*") {
            let end = rest.find("*/").map(|i| i + 2).unwrap_or(rest.len());
            tokens.push(Token { kind: TokenKind::Comment, text: &rest[..end] });
            rest = &rest[end..];
            continue;
        }

        // --- byte string / byte char: b"…" or b'…' ---
        if rest.starts_with("b\"") || rest.starts_with("b'") {
            let quote = rest.as_bytes()[1] as char;
            let inner_end = find_quoted_end(&rest[1..], quote);
            let end = 1 + inner_end;
            tokens.push(Token { kind: TokenKind::Literal, text: &rest[..end] });
            rest = &rest[end..];
            continue;
        }

        // --- raw string literal: r"…" or r#"…"# or r##"…"## etc. ---
        if rest.starts_with('r') {
            let hashes: usize = rest[1..].bytes().take_while(|&b| b == b'#').count();
            let start_len = 1 + hashes; // length of "r" + hashes before the opening quote
            if rest.len() > start_len && rest.as_bytes()[start_len] == b'"' {
                // Build the closing delimiter: '"' followed by `hashes` '#' chars.
                let mut closing = String::with_capacity(1 + hashes);
                closing.push('"');
                for _ in 0..hashes {
                    closing.push('#');
                }
                let search_from = start_len + 1; // skip past the opening quote
                let end = rest[search_from..]
                    .find(&*closing)
                    .map(|i| search_from + i + closing.len())
                    .unwrap_or(rest.len());
                tokens.push(Token { kind: TokenKind::Literal, text: &rest[..end] });
                rest = &rest[end..];
                continue;
            }
        }

        // --- double-quoted string literal ---
        if rest.starts_with('"') {
            let end = find_quoted_end(rest, '"');
            tokens.push(Token { kind: TokenKind::Literal, text: &rest[..end] });
            rest = &rest[end..];
            continue;
        }

        // --- single-quoted char / lifetime ---
        if rest.starts_with('\'') {
            let end = find_quoted_end(rest, '\'');
            tokens.push(Token { kind: TokenKind::Literal, text: &rest[..end] });
            rest = &rest[end..];
            continue;
        }

        // --- numeric literal: decimal, hex (0x…), octal (0o…), binary (0b…) ---
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            let end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
                .unwrap_or(rest.len());
            tokens.push(Token { kind: TokenKind::Literal, text: &rest[..end] });
            rest = &rest[end..];
            continue;
        }

        // --- identifier or keyword: starts with a letter or underscore ---
        if rest.starts_with(|c: char| c.is_alphabetic() || c == '_') {
            let end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let word = &rest[..end];
            let kind = if RUST_KEYWORDS.contains(&word) {
                TokenKind::Keyword
            } else {
                TokenKind::Identifier
            };
            tokens.push(Token { kind, text: word });
            rest = &rest[end..];
            continue;
        }

        // --- punctuation / operator: one codepoint at a time ---
        let char_len = rest
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        tokens.push(Token {
            kind: TokenKind::Punctuation,
            text: &rest[..char_len],
        });
        rest = &rest[char_len..];
    }

    tokens
}

// ---------------------------------------------------------------------------
// Plain-text fallback lexer
// ---------------------------------------------------------------------------

/// Fallback lexer for file types without a dedicated lexer.
///
/// Emits the non-newline portion of the input as a single
/// `Punctuation` token (reusing the catch-all kind) followed by a `Newline`
/// token, preserving the lossless round-trip invariant.
pub fn lex_plain(input: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    for part in input.split_inclusive('\n') {
        let (text, newline) = if part.ends_with('\n') {
            (&part[..part.len() - 1], Some(&part[part.len() - 1..]))
        } else {
            (part, None)
        };
        if !text.is_empty() {
            tokens.push(Token { kind: TokenKind::Punctuation, text });
        }
        if let Some(nl) = newline {
            tokens.push(Token { kind: TokenKind::Newline, text: nl });
        }
    }
    tokens
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Return the exclusive end index of a quoted span that begins at `s[0]`.
///
/// The opening quote character is `quote`.  Backslash-escaped characters
/// are skipped over.  If the closing quote is never found, returns
/// `s.len()` (the span covers the rest of the input).
fn find_quoted_end(s: &str, quote: char) -> usize {
    let mut iter = s.char_indices().peekable();
    iter.next(); // skip opening quote

    loop {
        match iter.next() {
            None => return s.len(),
            Some((_, '\\')) => {
                iter.next(); // skip escaped char
            }
            Some((i, c)) if c == quote => return i + quote.len_utf8(),
            Some((i, '\n')) => return i, // unterminated — stop at newline
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(tokens: &[Token<'_>]) -> Vec<TokenKind> {
        tokens.iter().map(|t| t.kind).collect()
    }

    fn texts<'a>(tokens: &'a [Token<'a>]) -> Vec<&'a str> {
        tokens.iter().map(|t| t.text).collect()
    }

    #[test]
    fn roundtrip_simple_function() {
        let src = "fn main() {\n    let x = 42;\n}\n";
        let toks = lex_rust(src);
        let reconstructed: String = toks.iter().map(|t| t.text).collect();
        assert_eq!(reconstructed, src, "lexer must be lossless");
    }

    #[test]
    fn roundtrip_with_string_literal() {
        let src = "let s = \"hello \\\"world\\\"\";\n";
        let toks = lex_rust(src);
        let reconstructed: String = toks.iter().map(|t| t.text).collect();
        assert_eq!(reconstructed, src);
    }

    #[test]
    fn roundtrip_with_block_comment() {
        let src = "/* top\n   doc */\nfn f() {}\n";
        let toks = lex_rust(src);
        let reconstructed: String = toks.iter().map(|t| t.text).collect();
        assert_eq!(reconstructed, src);
    }

    #[test]
    fn keywords_are_classified() {
        let toks = lex_rust("fn let mut");
        let k = kinds(&toks);
        assert_eq!(k[0], TokenKind::Keyword); // fn
        assert_eq!(k[2], TokenKind::Keyword); // let
        assert_eq!(k[4], TokenKind::Keyword); // mut
    }

    #[test]
    fn identifiers_distinguished_from_keywords() {
        let toks = lex_rust("foo bar baz");
        for tok in &toks {
            if tok.kind != TokenKind::Whitespace {
                assert_eq!(tok.kind, TokenKind::Identifier, "expected Identifier for {:?}", tok.text);
            }
        }
    }

    #[test]
    fn numeric_literals() {
        let toks = lex_rust("42 3.14 0xFF 0b1010");
        let lits: Vec<&str> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Literal)
            .map(|t| t.text)
            .collect();
        assert_eq!(lits, ["42", "3.14", "0xFF", "0b1010"]);
    }

    #[test]
    fn line_comment_stops_at_newline() {
        let toks = lex_rust("x // hello\ny");
        let comment = toks.iter().find(|t| t.kind == TokenKind::Comment).unwrap();
        assert_eq!(comment.text, "// hello");
    }

    #[test]
    fn string_with_escaped_quote() {
        let toks = lex_rust("\"a\\\"b\"");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Literal);
        assert_eq!(toks[0].text, "\"a\\\"b\"");
    }

    #[test]
    fn raw_string_literal() {
        let toks = lex_rust("r#\"hello\"#");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Literal);
    }

    #[test]
    fn byte_string_literal() {
        let toks = lex_rust("b\"bytes\"");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Literal);
    }

    #[test]
    fn plain_lexer_roundtrip() {
        let src = "hello world\nsecond line\n";
        let toks = lex_plain(src);
        let reconstructed: String = toks.iter().map(|t| t.text).collect();
        assert_eq!(reconstructed, src);
    }

    #[test]
    fn plain_lexer_no_newline_at_end() {
        let src = "no newline";
        let toks = lex_plain(src);
        let reconstructed: String = toks.iter().map(|t| t.text).collect();
        assert_eq!(reconstructed, src);
    }

    #[test]
    fn whitespace_and_punctuation_classified() {
        let toks = lex_rust("  ;,{}");
        assert_eq!(toks[0].kind, TokenKind::Whitespace);
        assert!(toks[1..].iter().all(|t| t.kind == TokenKind::Punctuation));
        assert_eq!(texts(&toks), ["  ", ";", ",", "{", "}"]);
    }
}
