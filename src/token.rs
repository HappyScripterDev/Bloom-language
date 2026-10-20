use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub offset: usize,
    pub line: u32,
    pub col: u32,
}

impl Pos {
    pub fn new(offset: usize, line: u32, col: u32) -> Self {
        Pos { offset, line, col }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Span {
    pub fn new(start: Pos, end: Pos) -> Self {
        Span { start, end }
    }

    pub fn merge(&self, other: &Span) -> Span {
        Span {
            start: if self.start.offset <= other.start.offset {
                self.start
            } else {
                other.start
            },
            end: if self.end.offset >= other.end.offset {
                self.end
            } else {
                other.end
            },
        }
    }

    pub fn point(line: u32, col: u32) -> Span {
        let p = Pos::new(0, line, col);
        Span::new(p, p)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    CharLiteral(char),

    /// Only produced by `lex_with_comments` (formatter): `// ...`.
    LineComment(String),
    /// Only produced by `lex_with_comments` (formatter): `/* ... */`.
    BlockComment(String),

    Fn,
    Let,
    Var,
    Const,
    If,
    Else,
    While,
    For,
    In,
    Return,
    Break,
    Continue,
    True,
    False,

    Use,
    Class,
    Struct,
    Enum,
    Import,
    Impl,
    Pub,
    Match,
    SelfValue,
    Extern,
    Unsafe,

    Ident(String),

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    ColonColon,
    Dot,
    DotDot,
    Arrow,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    FatArrow,
    Bang,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AmpAmp,
    PipePipe,
    Pipe,
    Question,

    Eof,
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            TokenKind::IntLiteral(v) => format!("integer literal `{}`", v),
            TokenKind::FloatLiteral(v) => format!("float literal `{}`", v),
            TokenKind::StringLiteral(_) => "string literal".to_string(),
            TokenKind::CharLiteral(_) => "character literal".to_string(),
            TokenKind::LineComment(_) => "line comment".to_string(),
            TokenKind::BlockComment(_) => "block comment".to_string(),
            TokenKind::Fn => "`fn`".to_string(),
            TokenKind::Let => "`let`".to_string(),
            TokenKind::Var => "`var`".to_string(),
            TokenKind::Const => "`const`".to_string(),
            TokenKind::If => "`if`".to_string(),
            TokenKind::Else => "`else`".to_string(),
            TokenKind::While => "`while`".to_string(),
            TokenKind::For => "`for`".to_string(),
            TokenKind::In => "`in`".to_string(),
            TokenKind::Return => "`return`".to_string(),
            TokenKind::Break => "`break`".to_string(),
            TokenKind::Continue => "`continue`".to_string(),
            TokenKind::True => "`true`".to_string(),
            TokenKind::False => "`false`".to_string(),
            TokenKind::Use => "`use`".to_string(),
            TokenKind::Class => "`class`".to_string(),
            TokenKind::Struct => "`struct`".to_string(),
            TokenKind::Enum => "`enum`".to_string(),
            TokenKind::Import => "`import`".to_string(),
            TokenKind::Impl => "`impl`".to_string(),
            TokenKind::Pub => "`pub`".to_string(),
            TokenKind::Match => "`match`".to_string(),
            TokenKind::SelfValue => "`self`".to_string(),
            TokenKind::Extern => "`extern`".to_string(),
            TokenKind::Unsafe => "`unsafe`".to_string(),
            TokenKind::Ident(name) => format!("identifier `{}`", name),
            TokenKind::LParen => "`(`".to_string(),
            TokenKind::RParen => "`)`".to_string(),
            TokenKind::LBrace => "`{`".to_string(),
            TokenKind::RBrace => "`}`".to_string(),
            TokenKind::LBracket => "`[`".to_string(),
            TokenKind::RBracket => "`]`".to_string(),
            TokenKind::Comma => "`,`".to_string(),
            TokenKind::Semicolon => "`;`".to_string(),
            TokenKind::Colon => "`:`".to_string(),
            TokenKind::ColonColon => "`::`".to_string(),
            TokenKind::Dot => "`.`".to_string(),
            TokenKind::DotDot => "`..`".to_string(),
            TokenKind::Arrow => "`->`".to_string(),
            TokenKind::Plus => "`+`".to_string(),
            TokenKind::Minus => "`-`".to_string(),
            TokenKind::Star => "`*`".to_string(),
            TokenKind::Slash => "`/`".to_string(),
            TokenKind::Percent => "`%`".to_string(),
            TokenKind::Eq => "`=`".to_string(),
            TokenKind::EqEq => "`==`".to_string(),
            TokenKind::FatArrow => "`=>`".to_string(),
            TokenKind::Bang => "`!`".to_string(),
            TokenKind::BangEq => "`!=`".to_string(),
            TokenKind::Lt => "`<`".to_string(),
            TokenKind::LtEq => "`<=`".to_string(),
            TokenKind::Gt => "`>`".to_string(),
            TokenKind::GtEq => "`>=`".to_string(),
            TokenKind::AmpAmp => "`&&`".to_string(),
            TokenKind::PipePipe => "`||`".to_string(),
            TokenKind::Pipe => "`|`".to_string(),
            TokenKind::Question => "`?`".to_string(),
            TokenKind::Eof => "end of file".to_string(),
        }
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.describe())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token { kind, span }
    }
}

pub fn keyword_kind(text: &str) -> Option<TokenKind> {
    Some(match text {
        "fn" => TokenKind::Fn,
        "let" => TokenKind::Let,
        "var" => TokenKind::Var,
        "const" => TokenKind::Const,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "return" => TokenKind::Return,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "use" => TokenKind::Use,
        "class" => TokenKind::Class,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "import" => TokenKind::Import,
        "impl" => TokenKind::Impl,
        "pub" => TokenKind::Pub,
        "match" => TokenKind::Match,
        "self" => TokenKind::SelfValue,
        "extern" => TokenKind::Extern,
        "unsafe" => TokenKind::Unsafe,
        _ => return None,
    })
}
