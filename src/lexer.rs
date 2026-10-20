use crate::diagnostics::{codes, Diagnostic};
use crate::token::{keyword_kind, Pos, Span, Token, TokenKind};

pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &str) -> LexOutput {
    lex_impl(source, false)
}

/// Lexes including comment tokens (`TokenKind::LineComment`/`BlockComment`).
/// Used by the formatter, which must preserve every comment. The compiler
/// itself uses `lex`, which skips comments.
pub fn lex_with_comments(source: &str) -> LexOutput {
    lex_impl(source, true)
}

fn lex_impl(source: &str, keep_comments: bool) -> LexOutput {
    let chars: Vec<char> = source.chars().collect();
    let mut offsets = Vec::with_capacity(chars.len() + 1);
    let mut off = 0usize;
    for c in &chars {
        offsets.push(off);
        off += c.len_utf8();
    }
    offsets.push(off);
    let mut lexer = Lexer {
        src_len: source.len(),
        chars,
        offsets,
        i: 0,
        line: 1,
        col: 1,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
        keep_comments,
    };
    lexer.run();
    LexOutput {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer {
    src_len: usize,
    chars: Vec<char>,
    offsets: Vec<usize>,
    i: usize,
    line: u32,
    col: u32,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    keep_comments: bool,
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.chars.get(self.i + k).copied()
    }

    fn pos(&self) -> Pos {
        let offset = self.offsets.get(self.i).copied().unwrap_or(self.src_len);
        Pos::new(offset, self.line, self.col)
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn span_from(&self, start: Pos) -> Span {
        Span::new(start, self.pos())
    }

    fn push(&mut self, kind: TokenKind, start: Pos) {
        let span = self.span_from(start);
        self.tokens.push(Token::new(kind, span));
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(code, message, span));
    }

    fn run(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\u{FEFF}' {
                self.i += 1;
                continue;
            }
            if c == '\r' {
                self.advance();
                continue;
            }
            if c.is_whitespace() {
                self.advance();
                continue;
            }
            let start = self.pos();
            match c {
                '/' => {
                    if self.peek_at(1) == Some('/') {
                        let start_i = self.i;
                        while let Some(ch) = self.peek() {
                            if ch == '\n' {
                                break;
                            }
                            self.advance();
                        }
                        if self.keep_comments {
                            let text: String = self.chars[start_i..self.i].iter().collect();
                            self.push(TokenKind::LineComment(text), start);
                        }
                    } else if self.peek_at(1) == Some('*') {
                        let start_i = self.i;
                        self.advance();
                        self.advance();
                        let mut closed = false;
                        while let Some(ch) = self.advance() {
                            if ch == '*' && self.peek() == Some('/') {
                                self.advance();
                                closed = true;
                                break;
                            }
                        }
                        if !closed {
                            self.error(
                                codes::UNTERMINATED_COMMENT,
                                "unterminated block comment",
                                self.span_from(start),
                            );
                        }
                        if self.keep_comments {
                            let text: String = self.chars[start_i..self.i].iter().collect();
                            self.push(TokenKind::BlockComment(text), start);
                        }
                    } else {
                        self.advance();
                        self.push(TokenKind::Slash, start);
                    }
                }
                '"' => self.string(start),
                '\'' => self.char_literal(start),
                '0'..='9' => self.number(start),
                _ if is_ident_start(c) => self.identifier(start),
                '(' => {
                    self.advance();
                    self.push(TokenKind::LParen, start);
                }
                ')' => {
                    self.advance();
                    self.push(TokenKind::RParen, start);
                }
                '{' => {
                    self.advance();
                    self.push(TokenKind::LBrace, start);
                }
                '}' => {
                    self.advance();
                    self.push(TokenKind::RBrace, start);
                }
                '[' => {
                    self.advance();
                    self.push(TokenKind::LBracket, start);
                }
                ']' => {
                    self.advance();
                    self.push(TokenKind::RBracket, start);
                }
                ',' => {
                    self.advance();
                    self.push(TokenKind::Comma, start);
                }
                ';' => {
                    self.advance();
                    self.push(TokenKind::Semicolon, start);
                }
                ':' => {
                    self.advance();
                    if self.peek() == Some(':') {
                        self.advance();
                        self.push(TokenKind::ColonColon, start);
                    } else {
                        self.push(TokenKind::Colon, start);
                    }
                }
                '.' => {
                    self.advance();
                    if self.peek() == Some('.') {
                        self.advance();
                        self.push(TokenKind::DotDot, start);
                    } else {
                        self.push(TokenKind::Dot, start);
                    }
                }
                '+' => {
                    self.advance();
                    self.push(TokenKind::Plus, start);
                }
                '-' => {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        self.push(TokenKind::Arrow, start);
                    } else {
                        self.push(TokenKind::Minus, start);
                    }
                }
                '*' => {
                    self.advance();
                    self.push(TokenKind::Star, start);
                }
                '%' => {
                    self.advance();
                    self.push(TokenKind::Percent, start);
                }
                '=' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.push(TokenKind::EqEq, start);
                    } else if self.peek() == Some('>') {
                        self.advance();
                        self.push(TokenKind::FatArrow, start);
                    } else {
                        self.push(TokenKind::Eq, start);
                    }
                }
                '!' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.push(TokenKind::BangEq, start);
                    } else {
                        self.push(TokenKind::Bang, start);
                    }
                }
                '<' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.push(TokenKind::LtEq, start);
                    } else {
                        self.push(TokenKind::Lt, start);
                    }
                }
                '>' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.push(TokenKind::GtEq, start);
                    } else {
                        self.push(TokenKind::Gt, start);
                    }
                }
                '&' => {
                    self.advance();
                    if self.peek() == Some('&') {
                        self.advance();
                        self.push(TokenKind::AmpAmp, start);
                    } else {
                        let span = self.span_from(start);
                        self.error(codes::UNEXPECTED_CHAR, "unexpected character `&`", span);
                        self.diagnostics.last_mut().unwrap().help =
                            Some("use `&&` for logical and".to_string());
                        self.tokens.push(Token::new(TokenKind::AmpAmp, span));
                    }
                }
                '|' => {
                    self.advance();
                    if self.peek() == Some('|') {
                        self.advance();
                        self.push(TokenKind::PipePipe, start);
                    } else {
                        self.push(TokenKind::Pipe, start);
                    }
                }
                '?' => {
                    self.advance();
                    self.push(TokenKind::Question, start);
                }
                _ => {
                    self.advance();
                    self.error(
                        codes::UNEXPECTED_CHAR,
                        format!("unexpected character `{}`", c),
                        self.span_from(start),
                    );
                }
            }
        }
        let end = self.pos();
        self.tokens
            .push(Token::new(TokenKind::Eof, Span::new(end, end)));
    }

    fn identifier(&mut self, start: Pos) {
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                text.push(c);
                self.advance();
            } else {
                break;
            }
        }
        let kind = keyword_kind(&text).unwrap_or(TokenKind::Ident(text));
        self.push(kind, start);
    }

    fn number(&mut self, start: Pos) {
        let mut digits = String::new();
        let mut is_float = false;
        let first = self.peek().unwrap();
        if first == '0' {
            if let Some(prefix) = self.peek_at(1) {
                let radix = match prefix {
                    'x' | 'X' => Some(16),
                    'b' | 'B' => Some(2),
                    'o' | 'O' => Some(8),
                    _ => None,
                };
                if let Some(radix) = radix {
                    self.advance();
                    self.advance();
                    let mut any = false;
                    while let Some(c) = self.peek() {
                        if c == '_' {
                            self.advance();
                        } else if c.is_digit(radix) {
                            digits.push(c);
                            any = true;
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let span = self.span_from(start);
                    if !any {
                        self.error(
                            codes::INVALID_NUMBER,
                            "missing digits after integer base prefix",
                            span,
                        );
                        self.push(TokenKind::IntLiteral(0), start);
                        return;
                    }
                    match i64::from_str_radix(&digits, radix) {
                        Ok(v) => self.push(TokenKind::IntLiteral(v), start),
                        Err(_) => {
                            self.error(
                                codes::INVALID_NUMBER,
                                "integer literal is out of range for `int` (64-bit signed)",
                                span,
                            );
                            self.push(TokenKind::IntLiteral(0), start);
                        }
                    }
                    return;
                }
            }
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                self.advance();
            } else if c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        if self.peek() == Some('.') {
            if let Some(next) = self.peek_at(1) {
                if next.is_ascii_digit() {
                    is_float = true;
                    digits.push('.');
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() {
                            digits.push(c);
                            self.advance();
                        } else if c == '_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        if let Some(c) = self.peek() {
            if c == 'e' || c == 'E' {
                let has_exp = match self.peek_at(1) {
                    Some('+') | Some('-') => {
                        self.peek_at(2).map(|c| c.is_ascii_digit()).unwrap_or(false)
                    }
                    Some(c) if c.is_ascii_digit() => true,
                    _ => false,
                };
                if has_exp {
                    is_float = true;
                    digits.push('e');
                    self.advance();
                    if let Some(sign) = self.peek() {
                        if sign == '+' || sign == '-' {
                            digits.push(sign);
                            self.advance();
                        }
                    }
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() {
                            digits.push(c);
                            self.advance();
                        } else if c == '_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        let span = self.span_from(start);
        if is_float {
            match digits.parse::<f64>() {
                Ok(v) if v.is_finite() => self.push(TokenKind::FloatLiteral(v), start),
                _ => {
                    self.error(
                        codes::INVALID_NUMBER,
                        "float literal is out of range for `float` (64-bit)",
                        span,
                    );
                    self.push(TokenKind::FloatLiteral(0.0), start);
                }
            }
        } else {
            match digits.parse::<i64>() {
                Ok(v) => self.push(TokenKind::IntLiteral(v), start),
                Err(_) => {
                    self.error(
                        codes::INVALID_NUMBER,
                        "integer literal is out of range for `int` (64-bit signed); \
                         note: the smallest representable literal is -9223372036854775807 - 1",
                        span,
                    );
                    self.push(TokenKind::IntLiteral(0), start);
                }
            }
        }
    }

    fn escape(&mut self, start: Pos) -> Option<char> {
        self.advance();
        let c = match self.advance() {
            Some(c) => c,
            None => {
                self.error(
                    codes::UNTERMINATED_STRING,
                    "unterminated escape sequence",
                    self.span_from(start),
                );
                return None;
            }
        };
        let value = match c {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            'u' => {
                if self.peek() != Some('{') {
                    self.error(
                        codes::INVALID_ESCAPE,
                        "expected `{` after `\\u` (form: `\\u{1F600}`)",
                        self.span_from(start),
                    );
                    return None;
                }
                self.advance();
                let mut hex = String::new();
                while let Some(h) = self.peek() {
                    if h == '}' {
                        break;
                    }
                    if h.is_ascii_hexdigit() {
                        hex.push(h);
                        self.advance();
                    } else {
                        self.error(
                            codes::INVALID_ESCAPE,
                            format!("invalid character `{}` in unicode escape", h),
                            self.span_from(start),
                        );
                        return None;
                    }
                }
                if self.peek() != Some('}') {
                    self.error(
                        codes::INVALID_ESCAPE,
                        "unterminated unicode escape",
                        self.span_from(start),
                    );
                    return None;
                }
                self.advance();
                if hex.is_empty() || hex.len() > 6 {
                    self.error(
                        codes::INVALID_ESCAPE,
                        "unicode escape must contain 1 to 6 hexadecimal digits",
                        self.span_from(start),
                    );
                    return None;
                }
                let code = u32::from_str_radix(&hex, 16).ok()?;
                match char::from_u32(code) {
                    Some(ch) => ch,
                    None => {
                        self.error(
                            codes::INVALID_ESCAPE,
                            format!("`\\u{{{}}}` is not a valid unicode scalar value", hex),
                            self.span_from(start),
                        );
                        return None;
                    }
                }
            }
            other => {
                self.error(
                    codes::INVALID_ESCAPE,
                    format!("unknown escape sequence `\\{}`", other),
                    self.span_from(start),
                );
                return None;
            }
        };
        Some(value)
    }

    fn string(&mut self, start: Pos) {
        self.advance();
        let mut text = String::new();
        loop {
            match self.peek() {
                None => {
                    self.error(
                        codes::UNTERMINATED_STRING,
                        "unterminated string literal",
                        self.span_from(start),
                    );
                    break;
                }
                Some('\n') => {
                    self.error(
                        codes::UNTERMINATED_STRING,
                        "unterminated string literal (strings cannot span multiple lines)",
                        self.span_from(start),
                    );
                    break;
                }
                Some('"') => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    if let Some(c) = self.escape(start) {
                        text.push(c);
                    }
                }
                Some(c) => {
                    text.push(c);
                    self.advance();
                }
            }
        }
        self.push(TokenKind::StringLiteral(text), start);
    }

    fn char_literal(&mut self, start: Pos) {
        self.advance();
        let value = match self.peek() {
            None | Some('\n') => {
                self.error(
                    codes::INVALID_CHAR_LITERAL,
                    "unterminated character literal",
                    self.span_from(start),
                );
                self.push(TokenKind::CharLiteral('\0'), start);
                return;
            }
            Some('\'') => {
                self.error(
                    codes::INVALID_CHAR_LITERAL,
                    "empty character literal",
                    self.span_from(start),
                );
                self.advance();
                self.push(TokenKind::CharLiteral('\0'), start);
                return;
            }
            Some('\\') => self.escape(start).unwrap_or('\0'),
            Some(c) => {
                self.advance();
                c
            }
        };
        if self.peek() == Some('\'') {
            self.advance();
            self.push(TokenKind::CharLiteral(value), start);
        } else {
            while let Some(c) = self.peek() {
                if c == '\'' || c == '\n' {
                    break;
                }
                self.advance();
            }
            if self.peek() == Some('\'') {
                self.advance();
            }
            self.error(
                codes::INVALID_CHAR_LITERAL,
                "character literal must contain exactly one character",
                self.span_from(start),
            );
            self.push(TokenKind::CharLiteral(value), start);
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::TokenKind;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src)
            .tokens
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokenKind::Eof)
            .collect()
    }

    fn errors(src: &str) -> Vec<Diagnostic> {
        lex(src).diagnostics
    }

    #[test]
    fn lexes_basic_program() {
        let src = "fn main() -> int { return 0; }";
        let ks = kinds(src);
        assert_eq!(ks[0], TokenKind::Fn);
        assert_eq!(ks[1], TokenKind::Ident("main".to_string()));
        assert_eq!(ks[2], TokenKind::LParen);
        assert_eq!(ks[3], TokenKind::RParen);
        assert_eq!(ks[4], TokenKind::Arrow);
        assert_eq!(ks[5], TokenKind::Ident("int".to_string()));
        assert_eq!(ks[6], TokenKind::LBrace);
        assert_eq!(ks[7], TokenKind::Return);
        assert_eq!(ks[8], TokenKind::IntLiteral(0));
        assert_eq!(ks[9], TokenKind::Semicolon);
        assert_eq!(ks[10], TokenKind::RBrace);
    }

    #[test]
    fn lexes_numbers() {
        assert_eq!(kinds("42")[0], TokenKind::IntLiteral(42));
        assert_eq!(kinds("1_000")[0], TokenKind::IntLiteral(1000));
        assert_eq!(kinds("0xFF")[0], TokenKind::IntLiteral(255));
        assert_eq!(kinds("0b1010")[0], TokenKind::IntLiteral(10));
        assert_eq!(kinds("0o17")[0], TokenKind::IntLiteral(15));
        assert_eq!(kinds("3.5")[0], TokenKind::FloatLiteral(3.5));
        assert_eq!(kinds("2e3")[0], TokenKind::FloatLiteral(2000.0));
        assert_eq!(kinds("1.5e-2")[0], TokenKind::FloatLiteral(0.015));
    }

    #[test]
    fn lexes_range_versus_float() {
        let ks = kinds("0..10");
        assert_eq!(
            ks,
            vec![
                TokenKind::IntLiteral(0),
                TokenKind::DotDot,
                TokenKind::IntLiteral(10)
            ]
        );
    }

    #[test]
    fn lexes_strings_and_chars() {
        let ks = kinds("\"a\\nb\"");
        assert_eq!(ks[0], TokenKind::StringLiteral("a\nb".to_string()));
        let ks = kinds("'x'");
        assert_eq!(ks[0], TokenKind::CharLiteral('x'));
        let ks = kinds("'\\u{1F600}'");
        assert_eq!(ks[0], TokenKind::CharLiteral('\u{1F600}'));
    }

    #[test]
    fn lexes_operators() {
        let ks = kinds("a == b != c <= d >= e && f || g !h");
        assert_eq!(ks[1], TokenKind::EqEq);
        assert_eq!(ks[3], TokenKind::BangEq);
        assert_eq!(ks[5], TokenKind::LtEq);
        assert_eq!(ks[7], TokenKind::GtEq);
        assert_eq!(ks[9], TokenKind::AmpAmp);
        assert_eq!(ks[11], TokenKind::PipePipe);
        assert_eq!(ks[13], TokenKind::Bang);
    }

    #[test]
    fn skips_comments() {
        let ks = kinds("1 // comment\n2 /* block\ncomment */ 3");
        assert_eq!(
            ks,
            vec![
                TokenKind::IntLiteral(1),
                TokenKind::IntLiteral(2),
                TokenKind::IntLiteral(3)
            ]
        );
    }

    #[test]
    fn reports_positions() {
        let tokens = lex("let x = 1;\nlet y = 2;").tokens;
        assert_eq!(tokens[0].span.start.line, 1);
        assert_eq!(tokens[0].span.start.col, 1);
        let y = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Let && t.span.start.line == 2)
            .unwrap();
        assert_eq!(y.span.start.col, 1);
        let two = tokens
            .iter()
            .find(|t| t.kind == TokenKind::IntLiteral(2))
            .unwrap();
        assert_eq!(two.span.start.line, 2);
        assert_eq!(two.span.start.col, 9);
    }

    #[test]
    fn reports_errors() {
        assert!(!errors("@").is_empty());
        assert!(!errors("\"unterminated").is_empty());
        assert!(!errors("/* open").is_empty());
        assert!(!errors("'ab'").is_empty());
        assert!(!errors("9999999999999999999999").is_empty());
        assert!(!errors("\"bad \\q escape\"").is_empty());
    }

    #[test]
    fn lexes_unicode_identifiers() {
        let ks = kinds("let größe = 1;");
        assert_eq!(ks[1], TokenKind::Ident("größe".to_string()));
    }
}
