//! `bloom fmt` — a comment-preserving, token-based source formatter.
//!
//! Architecture: the source is lexed with `lex_with_comments` (which keeps
//! comment tokens); every token is re-emitted from its exact source slice
//! (so literals, escapes and Unicode are preserved byte for byte), with
//! canonical whitespace inserted between tokens. No AST round-trip is done,
//! so no construct can be dropped; after formatting the result is re-lexed
//! and re-parsed with the real parser and only accepted if it parses.
//!
//! Canonical style (documented in docs/LANGUAGE_SPEC.md):
//!   - 4 spaces per nesting level (braces), no tabs.
//!   - One statement per line; `{` stays on the header line; `}` dedents.
//!   - Statement/match braces (containing `;` or `=>`) put every comma-
//!     separated item on its own line. Data braces (struct literals, named
//!     enum fields, named patterns — no `;`/`=>` inside) keep the author's
//!     layout: inline if the source had them on one line, expanded
//!     otherwise. Commas inside `()`/`[]` stay inline.
//!   - Single spaces around binary operators, after `,` and `:`; none around
//!     `.`, `::` and `..`; none before `,` `;` `)` `]` `?`.
//!   - `<`/`>` are treated as generics (no spaces) when the source had no
//!     whitespace before them, otherwise as comparisons (spaced).
//!   - At most one blank line is kept where the source had a blank line.
//!   - Line comments stay on their line (trailing comments keep one space
//!     before them); block comments are preserved verbatim.
//!   - The output ends with exactly one newline. Line endings (LF/CRLF) and
//!     a leading UTF-8 BOM are preserved from the input.

use crate::diagnostics::Diagnostic;
use crate::lexer::lex_with_comments;
use crate::token::{Token, TokenKind};

/// Pre-computes which `{` tokens are "data braces" that stay inline: the
/// matching `}` is on the same source line and the group contains no `;` or
/// `=>` (which mark statement lists and match arms).
fn inline_braces(tokens: &[Token]) -> Vec<bool> {
    let mut result = vec![false; tokens.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        match tok.kind {
            TokenKind::LBrace => stack.push(i),
            TokenKind::RBrace => {
                if let Some(open) = stack.pop() {
                    let same_line = tokens[open].span.start.line == tok.span.start.line;
                    if same_line {
                        let mut has_statement = false;
                        let mut depth = 0usize;
                        for t in &tokens[open + 1..i] {
                            match t.kind {
                                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                                    depth += 1;
                                }
                                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                                    depth = depth.saturating_sub(1);
                                }
                                TokenKind::Semicolon | TokenKind::FatArrow => {
                                    has_statement = true;
                                }
                                _ => {}
                            }
                        }
                        let _ = depth;
                        result[open] = !has_statement;
                    }
                }
            }
            _ => {}
        }
    }
    result
}

/// Marks tokens that lie inside a generic angle group (`Box<int, string>`).
/// Angles are recognized by source adjacency (the same heuristic as the
/// spacing rules), so comparisons (`a < b`) are not treated as generics.
fn generic_regions(tokens: &[Token]) -> Vec<bool> {
    let mut inside = vec![false; tokens.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        let mut is_angle = false;
        match tok.kind {
            TokenKind::Lt => {
                let adjacent = i > 0 && tokens[i - 1].span.end.offset == tok.span.start.offset;
                if adjacent {
                    stack.push(i);
                    is_angle = true;
                }
            }
            TokenKind::Gt => {
                if !stack.is_empty() {
                    stack.pop();
                }
                is_angle = true;
            }
            _ => {}
        }
        inside[i] = !stack.is_empty() && !is_angle;
    }
    inside
}

/// Formats Bloom source code. Returns the formatted text, or lexer
/// diagnostics when the input cannot be lexed. The caller should use
/// `format_and_verify` to also require that the result parses.
pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let lexed = lex_with_comments(source);
    if !lexed.diagnostics.is_empty() {
        return Err(lexed.diagnostics);
    }
    let tokens = &lexed.tokens;
    let inline = inline_braces(tokens);
    let in_generic = generic_regions(tokens);
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::new();
    if source.starts_with('\u{FEFF}') {
        out.push('\u{FEFF}');
    }
    let mut indent: usize = 0;
    // Bracket stack: (kind, inline). Only braces use `inline`.
    let mut brackets: Vec<(u8, bool)> = Vec::new();
    let mut prev: Option<&Token> = None;
    let mut prev_code: Option<&Token> = None;
    let mut prev_code_index: usize = 0;
    let mut prev_unary = false;
    let mut line_open = false;
    let mut pending_break = false;
    let mut pending_blank = false;

    for (i, tok) in tokens.iter().enumerate() {
        let text = &source[tok.span.start.offset..tok.span.end.offset];
        let is_comment = matches!(
            tok.kind,
            TokenKind::LineComment(_) | TokenKind::BlockComment(_)
        );
        let multiline_block = matches!(&tok.kind, TokenKind::BlockComment(t) if t.contains('\n'));
        let closing_inline =
            matches!(tok.kind, TokenKind::RBrace) && matches!(brackets.last(), Some((b'{', true)));

        // --- decide whether a line break must precede this token ----------
        let mut want_break = false;
        if let Some(p) = prev {
            if is_comment {
                // A comment starts its own line when the source had it on a
                // separate line (or when it spans multiple lines).
                want_break = multiline_block || p.span.end.line < tok.span.start.line;
            } else if let Some(pc) = prev_code {
                let top_brace = matches!(brackets.last(), Some((b'{', false)));
                let comma_in_generic = in_generic[prev_code_index];
                want_break = match &tok.kind {
                    TokenKind::RBrace => !closing_inline && !matches!(pc.kind, TokenKind::LBrace),
                    _ => {
                        matches!(pc.kind, TokenKind::Semicolon)
                            || (matches!(pc.kind, TokenKind::LBrace) && top_brace)
                            || (matches!(pc.kind, TokenKind::Comma)
                                && top_brace
                                && !comma_in_generic)
                            || (matches!(pc.kind, TokenKind::RBrace)
                                && !closing_inline
                                && !matches!(
                                    tok.kind,
                                    TokenKind::Else
                                        | TokenKind::RParen
                                        | TokenKind::RBracket
                                        | TokenKind::Comma
                                        | TokenKind::Semicolon
                                        | TokenKind::Dot
                                        | TokenKind::Question
                                        | TokenKind::FatArrow
                                        | TokenKind::Plus
                                        | TokenKind::Minus
                                        | TokenKind::Star
                                        | TokenKind::Slash
                                        | TokenKind::Percent
                                        | TokenKind::EqEq
                                        | TokenKind::BangEq
                                        | TokenKind::LtEq
                                        | TokenKind::GtEq
                                        | TokenKind::Lt
                                        | TokenKind::Gt
                                        | TokenKind::AmpAmp
                                        | TokenKind::PipePipe
                                ))
                    }
                };
            }
            // Keep at most one blank line where the source had one.
            if tok.span.start.line > p.span.end.line + 1 {
                want_break = true;
                pending_blank = true;
            }
        }
        if want_break {
            pending_break = true;
        }

        // --- closing braces dedent before the break is materialized -------
        if matches!(tok.kind, TokenKind::RBrace) && !closing_inline {
            indent = indent.saturating_sub(1);
        }

        // --- materialize the pending line break (with the current indent) --
        let first_on_line = if pending_break {
            if line_open {
                out.push_str(newline);
            }
            if pending_blank {
                out.push_str(newline);
            }
            for _ in 0..indent {
                out.push_str("    ");
            }
            pending_break = false;
            pending_blank = false;
            true
        } else {
            !line_open
        };

        // --- spacing and the token itself ---------------------------------
        if !first_on_line {
            if let Some(p) = prev {
                if space_before(p, tok, prev_unary, closing_inline) {
                    out.push(' ');
                }
            }
        }
        out.push_str(text);
        line_open = true;

        // --- state updates -------------------------------------------------
        let unary = is_unary(tokens, i);
        match &tok.kind {
            TokenKind::LBrace => {
                let brace_inline = inline[i];
                brackets.push((b'{', brace_inline));
                if !brace_inline {
                    indent += 1;
                    if !matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::RBrace)) {
                        pending_break = true;
                    }
                }
            }
            TokenKind::LBracket => brackets.push((b'[', false)),
            TokenKind::LParen => brackets.push((b'(', false)),
            TokenKind::RBrace | TokenKind::RBracket | TokenKind::RParen => {
                brackets.pop();
            }
            TokenKind::LineComment(_) => {
                pending_break = true;
            }
            TokenKind::BlockComment(_) if multiline_block => {
                pending_break = true;
            }
            _ => {}
        }
        if !is_comment {
            prev_code = Some(tok);
            prev_code_index = i;
        }
        prev_unary = unary;
        prev = Some(tok);
    }

    // Exactly one trailing newline, no trailing indentation.
    while out.ends_with(' ') || out.ends_with('\t') {
        out.pop();
    }
    while out.ends_with(newline) {
        out.truncate(out.len() - newline.len());
    }
    out.push_str(newline);
    Ok(out)
}

/// Whether a unary operator stands at position `i` (i.e. the previous token
/// cannot end an expression).
fn is_unary(tokens: &[Token], i: usize) -> bool {
    let tok = &tokens[i];
    if !matches!(tok.kind, TokenKind::Minus | TokenKind::Bang) {
        return false;
    }
    match tokens.get(i.wrapping_sub(1)) {
        None => true,
        Some(t) => matches!(
            t.kind,
            TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::Comma
                | TokenKind::Semicolon
                | TokenKind::Colon
                | TokenKind::Eq
                | TokenKind::EqEq
                | TokenKind::BangEq
                | TokenKind::Lt
                | TokenKind::LtEq
                | TokenKind::Gt
                | TokenKind::GtEq
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::AmpAmp
                | TokenKind::PipePipe
                | TokenKind::Pipe
                | TokenKind::FatArrow
                | TokenKind::Arrow
                | TokenKind::Return
        ),
    }
}

fn is_keyword_before_paren(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::If | TokenKind::While | TokenKind::For | TokenKind::Match | TokenKind::Return
    )
}

/// Canonical spacing between two adjacent tokens on the same line.
fn space_before(p: &Token, cur: &Token, prev_unary: bool, closing_inline: bool) -> bool {
    use TokenKind::*;
    let no_space_after_prev = matches!(p.kind, LParen | LBracket | ColonColon | Dot | DotDot);
    match &cur.kind {
        RBrace => closing_inline && !matches!(p.kind, LBrace),
        RParen | RBracket | Comma | Semicolon | Colon | ColonColon | Dot | DotDot | Question => {
            false
        }
        LParen => is_keyword_before_paren(&p.kind),
        LBracket => !no_space_after_prev && !matches!(p.kind, Ident(_) | RParen | RBracket),
        LBrace => !matches!(p.kind, LParen | LBracket),
        LineComment(_) => true,
        BlockComment(_) => !no_space_after_prev,
        FatArrow | Arrow | Eq | EqEq | BangEq | LtEq | GtEq | Plus | Minus | Star | Slash
        | Percent | AmpAmp | PipePipe | Pipe | Bang => !no_space_after_prev,
        Lt | Gt => {
            // Generics are written adjacent (`Box<int>`); comparisons are
            // spaced. Preserve the source's choice (idempotent).
            let adjacent = p.span.end.offset == cur.span.start.offset;
            !adjacent
        }
        _ => {
            if prev_unary {
                return false;
            }
            if matches!(p.kind, Lt | Gt) {
                // Keep generic brackets adjacent on both sides.
                return p.span.end.offset != cur.span.start.offset;
            }
            !no_space_after_prev
        }
    }
}

/// Formats `source` and verifies that the result lexes and parses cleanly.
/// Returns diagnostics instead of producing broken output.
pub fn format_and_verify(source: &str) -> Result<String, Vec<Diagnostic>> {
    let formatted = format_source(source)?;
    let lexed = crate::lexer::lex(&formatted);
    if !lexed.diagnostics.is_empty() {
        return Err(lexed.diagnostics);
    }
    let parsed = crate::parser::parse(lexed.tokens);
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics);
    }
    Ok(formatted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        format_and_verify(src).expect("format ok")
    }

    #[test]
    fn formats_simple_function() {
        let out = fmt("fn  main( )->int{let x=1+2;return x;}");
        assert_eq!(
            out,
            "fn main() -> int {\n    let x = 1 + 2;\n    return x;\n}\n"
        );
    }

    #[test]
    fn is_idempotent() {
        let cases = [
            "fn main() -> int { return 0; }",
            "struct P { x: int, y: int }\nfn main() -> int { let p = P { x: 1, y: 2 }; return p.x; }",
            "fn f(x: int) -> int {\n    if x > 0 {\n        return 1;\n    } else {\n        return 0;\n    }\n}",
            "fn main() -> int {\n    match 1 {\n        1 | 2 => println(\"low\"),\n        n if n > 5 => println(\"high\"),\n        _ => println(\"other\")\n    };\n    return 0;\n}",
            "enum E { A, B(int), C { x: int } }\nfn main() -> int { return 0; }",
            "fn main() -> int {\n    var i = 0;\n    while i < 10 {\n        i = i + 1;\n    }\n    for j in 0..5 {\n        println(j);\n    }\n    return 0;\n}",
        ];
        for src in cases {
            let once = fmt(src);
            let twice = fmt(&once);
            assert_eq!(once, twice, "not idempotent for:\n{src}");
        }
    }

    #[test]
    fn preserves_comments() {
        let src = "// header\nfn main() -> int {\n    // body comment\n    let x = 1; // trailing\n    /* block */\n    return x;\n}\n// end\n";
        let out = fmt(src);
        assert!(out.contains("// header"), "{out}");
        assert!(out.contains("// body comment"), "{out}");
        assert!(out.contains("// trailing"), "{out}");
        assert!(out.contains("/* block */"), "{out}");
        assert!(out.contains("// end"), "{out}");
        assert_eq!(fmt(&out), out);
    }

    #[test]
    fn preserves_literal_text() {
        let src = "fn main() -> int { let s = \"a\\tb\\u{1F600}\"; let c = '\\n'; let f = 1.50; return 0; }";
        let out = fmt(src);
        assert!(out.contains("\"a\\tb\\u{1F600}\""), "{out}");
        assert!(out.contains("'\\n'"), "{out}");
        assert!(out.contains("1.50"), "{out}");
    }

    #[test]
    fn keeps_generics_adjacent() {
        let out = fmt("fn main() -> int { let b: Box<int> = Box { value: 1 }; return 0; }");
        assert!(out.contains("Box<int>"), "{out}");
        let out2 = fmt("fn main() -> int { let c = a < b; return 0; }");
        assert!(out2.contains("a < b"), "{out2}");
    }

    #[test]
    fn unary_operators() {
        let out = fmt("fn main() -> int { let a = -1; let b = !true; let c = 3 - 2; return 0; }");
        assert!(out.contains("= -1"), "{out}");
        assert!(out.contains("= !true"), "{out}");
        assert!(out.contains("3 - 2"), "{out}");
    }

    #[test]
    fn ranges_stay_adjacent() {
        let out = fmt("fn main() -> int { for i in 0..5 { println(i); } return 0; }");
        assert!(out.contains("0..5"), "{out}");
    }

    #[test]
    fn empty_source() {
        assert_eq!(fmt(""), "\n");
    }

    #[test]
    fn crlf_is_preserved() {
        let out = fmt("fn main() -> int {\r\n    return 0;\r\n}\r\n");
        assert!(out.contains("\r\n"), "{out:?}");
        assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
    }

    #[test]
    fn struct_literals_follow_source_layout() {
        // Inline in the source -> stays inline.
        let inline_src = "fn main() -> int { let p = Point { x: 1, y: 2 }; return 0; }";
        let inline_out = fmt(inline_src);
        assert!(inline_out.contains("Point { x: 1, y: 2 }"), "{inline_out}");
        assert_eq!(fmt(&inline_out), inline_out);
        // Multi-line in the source -> stays multi-line.
        let multi_src = "fn main() -> int {\n    let p = Point {\n        x: 1,\n        y: 2\n    };\n    return 0;\n}\n";
        let multi_out = fmt(multi_src);
        assert_eq!(multi_out, multi_src);
        assert_eq!(fmt(&multi_out), multi_out);
        // Named enum fields and patterns keep the same rule.
        let enums = fmt("enum E { A { x: int, y: int }, B }\nfn main() -> int { let e = E::A { x: 1, y: 2 }; return 0; }");
        assert!(enums.contains("A { x: int, y: int }"), "{enums}");
        assert!(enums.contains("E::A { x: 1, y: 2 }"), "{enums}");
        assert_eq!(fmt(&enums), enums);
    }
}
