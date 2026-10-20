use crate::ast::*;
use crate::diagnostics::{codes, Diagnostic};
use crate::token::{Pos, Span, Token, TokenKind};

pub struct ParseOutput {
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
    pub next_expr_id: ExprId,
    pub next_decl_id: DeclId,
}

#[allow(dead_code)]
pub fn parse(tokens: Vec<Token>) -> ParseOutput {
    parse_with_offset(tokens, 0, 0)
}

pub fn parse_with_offset(
    tokens: Vec<Token>,
    expr_offset: ExprId,
    decl_offset: DeclId,
) -> ParseOutput {
    let mut parser = Parser {
        tokens,
        pos: 0,
        diagnostics: Vec::new(),
        next_expr_id: expr_offset,
        next_decl_id: decl_offset,
        panic_mode: false,
        struct_lits_allowed: true,
    };
    let program = parser.parse_program();
    ParseOutput {
        program,
        diagnostics: parser.diagnostics,
        next_expr_id: parser.next_expr_id,
        next_decl_id: parser.next_decl_id,
    }
}

type ParsedParams = (Option<(DeclId, ReceiverKind)>, Vec<Param>);

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    next_expr_id: ExprId,
    next_decl_id: DeclId,
    panic_mode: bool,
    struct_lits_allowed: bool,
}

impl Parser {
    fn fresh_expr_id(&mut self) -> ExprId {
        let id = self.next_expr_id;
        self.next_expr_id += 1;
        id
    }

    fn fresh_decl_id(&mut self) -> DeclId {
        let id = self.next_decl_id;
        self.next_decl_id += 1;
        id
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn peek_at(&self, k: usize) -> &TokenKind {
        let i = (self.pos + k).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error_at(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        if !self.panic_mode {
            self.panic_mode = true;
            if self.diagnostics.len() < 200 {
                self.diagnostics.push(Diagnostic::new(code, message, span));
            }
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, ()> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            let tok = self.peek().clone();
            let msg = format!("expected {}, found {}", what, tok.kind.describe());
            self.error_at(codes::UNEXPECTED_TOKEN, msg, tok.span);
            Err(())
        }
    }

    fn expect_semicolon(&mut self) -> Result<(), ()> {
        if self.at(&TokenKind::Semicolon) {
            self.advance();
            return Ok(());
        }
        let before = self.diagnostics.len();
        let tok = self.peek().clone();
        self.error_at(
            codes::UNEXPECTED_TOKEN,
            format!("expected `;`, found {}", tok.kind.describe()),
            tok.span,
        );
        if self.diagnostics.len() > before {
            if let Some(d) = self.diagnostics.last_mut() {
                d.help = Some("statements must end with `;`".to_string());
            }
        }
        Err(())
    }

    fn synchronize(&mut self) {
        self.panic_mode = false;
        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::Semicolon => {
                    self.advance();
                    return;
                }
                TokenKind::RBrace => return,
                TokenKind::Fn
                | TokenKind::Let
                | TokenKind::Var
                | TokenKind::Const
                | TokenKind::If
                | TokenKind::While
                | TokenKind::For
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Struct
                | TokenKind::Impl
                | TokenKind::Pub => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_program(&mut self) -> Program {
        let mut items = Vec::new();
        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::Fn => match self.parse_function(false) {
                    Ok(f) => items.push(Item::Function(f)),
                    Err(()) => self.synchronize(),
                },
                TokenKind::Struct => match self.parse_struct_decl(false) {
                    Ok(s) => items.push(Item::Struct(s)),
                    Err(()) => self.synchronize(),
                },
                TokenKind::Enum => match self.parse_enum_decl(false) {
                    Ok(e) => items.push(Item::Enum(e)),
                    Err(()) => self.synchronize(),
                },
                TokenKind::Impl => match self.parse_impl_block() {
                    Ok(b) => items.push(Item::Impl(b)),
                    Err(()) => self.synchronize(),
                },
                TokenKind::Extern => match self.parse_extern_block(false) {
                    Ok(b) => items.push(Item::Extern(b)),
                    Err(()) => self.synchronize(),
                },
                TokenKind::Pub => {
                    let pub_span = self.advance().span;
                    match self.peek_kind() {
                        TokenKind::Fn => match self.parse_function(true) {
                            Ok(f) => items.push(Item::Function(f)),
                            Err(()) => self.synchronize(),
                        },
                        TokenKind::Struct => match self.parse_struct_decl(true) {
                            Ok(s) => items.push(Item::Struct(s)),
                            Err(()) => self.synchronize(),
                        },
                        TokenKind::Enum => match self.parse_enum_decl(true) {
                            Ok(e) => items.push(Item::Enum(e)),
                            Err(()) => self.synchronize(),
                        },
                        TokenKind::Use => match self.parse_use_decl(true) {
                            Ok(u) => items.push(Item::Use(u)),
                            Err(()) => self.synchronize(),
                        },
                        TokenKind::Extern => match self.parse_extern_block(true) {
                            Ok(b) => items.push(Item::Extern(b)),
                            Err(()) => self.synchronize(),
                        },
                        other => {
                            let tok = self.peek().clone();
                            self.error_at(
                                codes::UNEXPECTED_TOKEN,
                                format!(
                                    "expected `fn`, `struct`, `enum` or `use` after `pub`, found {}",
                                    other.describe()
                                ),
                                tok.span,
                            );
                            let _ = pub_span;
                            self.synchronize();
                        }
                    }
                }
                TokenKind::Let | TokenKind::Var | TokenKind::Const => {
                    match self.parse_var_decl(false) {
                        Ok(decl) => items.push(Item::Global(decl)),
                        Err(()) => self.synchronize(),
                    }
                }
                TokenKind::Use | TokenKind::Import => match self.parse_use_decl(false) {
                    Ok(u) => items.push(Item::Use(u)),
                    Err(()) => self.synchronize(),
                },
                TokenKind::Class => {
                    let tok = self.advance();
                    self.error_at(
                        codes::UNSUPPORTED,
                        format!(
                            "{} declarations are not supported in Bloom 0.4",
                            tok.kind.describe()
                        ),
                        tok.span,
                    );
                    if let Some(d) = self.diagnostics.last_mut() {
                        d.help = Some(
                            "classes are not planned; use `struct` with `impl` methods instead"
                                .to_string(),
                        );
                    }
                    self.recover_statement_or_block();
                }
                other => {
                    let tok = self.peek().clone();
                    let msg = format!(
                        "expected a function (`fn`), a struct (`struct`) or a top-level declaration, found {}",
                        other.describe()
                    );
                    self.error_at(codes::UNEXPECTED_TOKEN, msg, tok.span);
                    self.advance();
                    self.synchronize();
                }
            }
        }
        Program { items }
    }

    fn recover_statement_or_block(&mut self) {
        loop {
            match self.peek_kind() {
                TokenKind::Eof => return,
                TokenKind::Semicolon => {
                    self.advance();
                    return;
                }
                TokenKind::LBrace => {
                    self.advance();
                    let mut depth = 1;
                    while depth > 0 && !self.at_eof() {
                        match self.peek_kind() {
                            TokenKind::LBrace => depth += 1,
                            TokenKind::RBrace => depth -= 1,
                            _ => {}
                        }
                        self.advance();
                    }
                    return;
                }
                TokenKind::Fn
                | TokenKind::Let
                | TokenKind::Var
                | TokenKind::Const
                | TokenKind::If
                | TokenKind::While
                | TokenKind::For
                | TokenKind::Return
                | TokenKind::Struct => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_use_decl(&mut self, is_pub: bool) -> Result<UseDecl, ()> {
        let start_tok = self.advance();
        let _start = start_tok.span.start;
        let mut path = Vec::new();
        let first = match self.peek_kind() {
            TokenKind::Ident(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!(
                        "expected a module name after `{}`, found {}",
                        start_tok.kind.describe(),
                        tok.kind.describe()
                    ),
                    tok.span,
                );
                return Err(());
            }
        };
        let mut path_span = first.span;
        if let TokenKind::Ident(n) = &first.kind {
            path.push(n.clone());
        }
        while self.at(&TokenKind::ColonColon) {
            self.advance();
            let seg = match self.peek_kind() {
                TokenKind::Ident(_) => self.advance(),
                _ => {
                    let tok = self.peek().clone();
                    self.error_at(
                        codes::UNEXPECTED_TOKEN,
                        format!("expected a name after `::`, found {}", tok.kind.describe()),
                        tok.span,
                    );
                    return Err(());
                }
            };
            if let TokenKind::Ident(n) = &seg.kind {
                path.push(n.clone());
            }
            path_span = path_span.merge(&seg.span);
        }
        self.expect_semicolon()?;
        let _ = self.prev_end();
        Ok(UseDecl {
            path,
            path_span,
            is_pub,
        })
    }

    fn parse_extern_block(&mut self, is_pub: bool) -> Result<ExternBlock, ()> {
        let start = self.expect(&TokenKind::Extern, "`extern`")?.span.start;
        let abi_tok = match self.peek_kind() {
            TokenKind::StringLiteral(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!(
                        "expected an ABI string (\"C\" or \"system\") after `extern`, found {}",
                        tok.kind.describe()
                    ),
                    tok.span,
                );
                return Err(());
            }
        };
        let abi = match &abi_tok.kind {
            TokenKind::StringLiteral(s) => s.clone(),
            _ => unreachable!(),
        };
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut functions = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let mut fn_pub = is_pub;
            if self.eat(&TokenKind::Pub) {
                fn_pub = true;
            }
            let fn_start = match self.peek_kind() {
                TokenKind::Fn => self.advance().span.start,
                _ => {
                    let tok = self.peek().clone();
                    self.error_at(
                        codes::UNEXPECTED_TOKEN,
                        format!(
                            "expected `fn` or `}}` in the extern block, found {}",
                            tok.kind.describe()
                        ),
                        tok.span,
                    );
                    return Err(());
                }
            };
            let name_tok = match self.peek_kind() {
                TokenKind::Ident(_) => self.advance(),
                _ => {
                    let tok = self.peek().clone();
                    self.error_at(
                        codes::UNEXPECTED_TOKEN,
                        format!(
                            "expected an external function name, found {}",
                            tok.kind.describe()
                        ),
                        tok.span,
                    );
                    return Err(());
                }
            };
            let name = match &name_tok.kind {
                TokenKind::Ident(n) => n.clone(),
                _ => unreachable!(),
            };
            let (receiver, params) = self.parse_params(false)?;
            debug_assert!(receiver.is_none());
            let ret = if self.eat(&TokenKind::Arrow) {
                self.parse_type()?
            } else {
                TypeExpr {
                    kind: TypeExprKind::Named("void".to_string()),
                    span: Span::new(self.peek().span.start, self.peek().span.start),
                }
            };
            let end = self
                .expect(&TokenKind::Semicolon, "`;` after the external declaration")?
                .span
                .end;
            functions.push(ExternFn {
                name: name.clone(),
                name_span: name_tok.span,
                symbol: name,
                params,
                ret,
                is_pub: fn_pub,
                span: Span::new(fn_start, end),
                canonical: String::new(),
                is_system_abi: false,
            });
        }
        let end = self.expect(&TokenKind::RBrace, "`}`")?.span.end;
        let _ = (is_pub, start, end);
        let _ = (is_pub, start, end);
        Ok(ExternBlock {
            abi,
            abi_span: abi_tok.span,
            functions,
        })
    }

    fn parse_function(&mut self, is_pub: bool) -> Result<Function, ()> {
        let start = self.expect(&TokenKind::Fn, "`fn`")?.span.start;
        let name_tok = match self.peek_kind() {
            TokenKind::Ident(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!("expected a function name, found {}", tok.kind.describe()),
                    tok.span,
                );
                return Err(());
            }
        };
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            _ => unreachable!(),
        };
        let type_params = self.parse_type_params()?;
        let (receiver, params) = self.parse_params(false)?;
        debug_assert!(receiver.is_none());
        let ret = if self.eat(&TokenKind::Arrow) {
            self.parse_type()?
        } else {
            TypeExpr {
                kind: TypeExprKind::Named("void".to_string()),
                span: Span::new(self.peek().span.start, self.peek().span.start),
            }
        };
        if !self.at(&TokenKind::LBrace) {
            let tok = self.peek().clone();
            self.error_at(
                codes::UNEXPECTED_TOKEN,
                format!("expected function body `{{`, found {}", tok.kind.describe()),
                tok.span,
            );
            return Err(());
        }
        let body = self.parse_block()?;
        let span = Span::new(start, body.span.end);
        Ok(Function {
            name,
            name_span: name_tok.span,
            type_params,
            params,
            ret,
            body,
            span,
            is_pub,
            c_name: String::new(),
        })
    }

    fn parse_type_params(&mut self) -> Result<Vec<String>, ()> {
        let mut params = Vec::new();
        if self.at(&TokenKind::Lt) {
            self.advance();
            loop {
                let p = match self.peek_kind() {
                    TokenKind::Ident(_) => self.advance(),
                    _ => {
                        let tok = self.peek().clone();
                        self.error_at(
                            codes::UNEXPECTED_TOKEN,
                            format!(
                                "expected a type parameter name, found {}",
                                tok.kind.describe()
                            ),
                            tok.span,
                        );
                        return Err(());
                    }
                };
                if let TokenKind::Ident(n) = &p.kind {
                    params.push(n.clone());
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::Gt, "`>` to close the type parameters")?;
        }
        Ok(params)
    }

    fn parse_params(&mut self, allow_receiver: bool) -> Result<ParsedParams, ()> {
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut receiver: Option<(DeclId, ReceiverKind)> = None;
        let mut params: Vec<Param> = Vec::new();
        let mut first = true;
        loop {
            if self.at(&TokenKind::RParen) {
                break;
            }
            if allow_receiver && first {
                if self.at(&TokenKind::SelfValue) {
                    self.advance();
                    receiver = Some((self.fresh_decl_id(), ReceiverKind::Value));
                } else if self.at(&TokenKind::Var)
                    && matches!(self.peek_at(1), TokenKind::SelfValue)
                {
                    self.advance();
                    self.advance();
                    receiver = Some((self.fresh_decl_id(), ReceiverKind::Mutable));
                }
            }
            first = false;
            if receiver.is_some() && params.is_empty() {
                if self.at(&TokenKind::RParen) {
                    break;
                }
                self.expect(&TokenKind::Comma, "`,` after `self`")?;
                if self.at(&TokenKind::RParen) {
                    break;
                }
            }
            let param_tok = match self.peek_kind() {
                TokenKind::Ident(_) => self.advance(),
                _ => {
                    let tok = self.peek().clone();
                    self.error_at(
                        codes::UNEXPECTED_TOKEN,
                        format!("expected a parameter name, found {}", tok.kind.describe()),
                        tok.span,
                    );
                    return Err(());
                }
            };
            let pname = match &param_tok.kind {
                TokenKind::Ident(n) => n.clone(),
                _ => unreachable!(),
            };
            self.expect(&TokenKind::Colon, "`:` after parameter name")?;
            let ty = self.parse_type()?;
            params.push(Param {
                id: self.fresh_decl_id(),
                name: pname,
                span: param_tok.span,
                ty,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok((receiver, params))
    }

    fn parse_impl_block(&mut self) -> Result<ImplBlock, ()> {
        let _start = self.expect(&TokenKind::Impl, "`impl`")?.span.start;
        let params = self.parse_type_params()?;
        let target = self.parse_type()?;
        let ty_span = target.span;
        self.expect(&TokenKind::LBrace, "`{` after the impl type")?;
        let mut methods = Vec::new();
        loop {
            if self.at(&TokenKind::RBrace) {
                break;
            }
            if self.at_eof() {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    "expected `}` to close the impl block, found end of file",
                    tok.span,
                );
                break;
            }
            let is_pub = self.eat(&TokenKind::Pub);
            if !self.at(&TokenKind::Fn) {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!(
                        "expected `fn` inside the impl block, found {}",
                        tok.kind.describe()
                    ),
                    tok.span,
                );
                self.skip_to_impl_item();
                continue;
            }
            match self.parse_method(is_pub) {
                Ok(m) => methods.push(m),
                Err(()) => self.skip_to_impl_item(),
            }
        }
        let _end = self.expect(&TokenKind::RBrace, "`}`")?.span.end;
        Ok(ImplBlock {
            params,
            target,
            ty_span,
            canonical_type: String::new(),
            methods,
        })
    }

    fn skip_to_impl_item(&mut self) {
        self.panic_mode = false;
        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::RBrace | TokenKind::Fn | TokenKind::Pub => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_method(&mut self, is_pub: bool) -> Result<Method, ()> {
        let start = self.expect(&TokenKind::Fn, "`fn`")?.span.start;
        let name_tok = match self.peek_kind() {
            TokenKind::Ident(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!("expected a method name, found {}", tok.kind.describe()),
                    tok.span,
                );
                return Err(());
            }
        };
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            _ => unreachable!(),
        };
        let (receiver, params) = self.parse_params(true)?;
        let (self_id, receiver) = match receiver {
            Some(r) => (r.0, Some(r.1)),
            None => (self.fresh_decl_id(), None),
        };
        let ret = if self.eat(&TokenKind::Arrow) {
            self.parse_type()?
        } else {
            TypeExpr {
                kind: TypeExprKind::Named("void".to_string()),
                span: Span::new(self.peek().span.start, self.peek().span.start),
            }
        };
        if !self.at(&TokenKind::LBrace) {
            let tok = self.peek().clone();
            self.error_at(
                codes::UNEXPECTED_TOKEN,
                format!("expected method body `{{`, found {}", tok.kind.describe()),
                tok.span,
            );
            return Err(());
        }
        let body = self.parse_block()?;
        let _span = Span::new(start, body.span.end);
        Ok(Method {
            name,
            name_span: name_tok.span,
            self_id,
            receiver,
            params,
            ret,
            body,
            is_pub,
            c_name: String::new(),
        })
    }

    fn parse_struct_decl(&mut self, is_pub: bool) -> Result<StructDecl, ()> {
        let start = self.expect(&TokenKind::Struct, "`struct`")?.span.start;
        let name_tok = match self.peek_kind() {
            TokenKind::Ident(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!("expected a struct name, found {}", tok.kind.describe()),
                    tok.span,
                );
                return Err(());
            }
        };
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            _ => unreachable!(),
        };
        let params = self.parse_type_params()?;
        self.expect(&TokenKind::LBrace, "`{` after the struct name")?;
        let mut fields = Vec::new();
        if !self.at(&TokenKind::RBrace) {
            loop {
                let field_tok = match self.peek_kind() {
                    TokenKind::Ident(_) => self.advance(),
                    _ => {
                        let tok = self.peek().clone();
                        self.error_at(
                            codes::UNEXPECTED_TOKEN,
                            format!("expected a field name, found {}", tok.kind.describe()),
                            tok.span,
                        );
                        return Err(());
                    }
                };
                let field_name = match &field_tok.kind {
                    TokenKind::Ident(n) => n.clone(),
                    _ => unreachable!(),
                };
                self.expect(&TokenKind::Colon, "`:` after the field name")?;
                let ty = self.parse_type()?;
                fields.push(StructFieldDecl {
                    name: field_name,
                    name_span: field_tok.span,
                    ty,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                if self.at(&TokenKind::RBrace) {
                    break;
                }
            }
        }
        let end = self.expect(&TokenKind::RBrace, "`}`")?.span.end;
        Ok(StructDecl {
            name,
            name_span: name_tok.span,
            params,
            fields,
            span: Span::new(start, end),
            is_pub,
            canonical: String::new(),
        })
    }

    fn parse_enum_decl(&mut self, is_pub: bool) -> Result<EnumDecl, ()> {
        let start = self.expect(&TokenKind::Enum, "`enum`")?.span.start;
        let name_tok = match self.peek_kind() {
            TokenKind::Ident(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!("expected an enum name, found {}", tok.kind.describe()),
                    tok.span,
                );
                return Err(());
            }
        };
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            _ => unreachable!(),
        };
        let mut params = Vec::new();
        if self.at(&TokenKind::Lt) {
            self.advance();
            loop {
                let p = match self.peek_kind() {
                    TokenKind::Ident(_) => self.advance(),
                    _ => {
                        let tok = self.peek().clone();
                        self.error_at(
                            codes::UNEXPECTED_TOKEN,
                            format!(
                                "expected a type parameter name, found {}",
                                tok.kind.describe()
                            ),
                            tok.span,
                        );
                        return Err(());
                    }
                };
                if let TokenKind::Ident(n) = &p.kind {
                    params.push(n.clone());
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::Gt, "`>` to close the type parameters")?;
        }
        self.expect(&TokenKind::LBrace, "`{` after the enum name")?;
        let mut variants = Vec::new();
        if !self.at(&TokenKind::RBrace) {
            loop {
                let var_tok = match self.peek_kind() {
                    TokenKind::Ident(_) => self.advance(),
                    _ => {
                        let tok = self.peek().clone();
                        self.error_at(
                            codes::UNEXPECTED_TOKEN,
                            format!("expected a variant name, found {}", tok.kind.describe()),
                            tok.span,
                        );
                        return Err(());
                    }
                };
                let variant_name = match &var_tok.kind {
                    TokenKind::Ident(n) => n.clone(),
                    _ => unreachable!(),
                };
                let payload = if self.at(&TokenKind::LParen) {
                    self.advance();
                    let mut tys = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            tys.push(self.parse_type()?);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen, "`)`")?;
                    EnumPayload::Tuple(tys)
                } else if self.at(&TokenKind::LBrace) {
                    self.advance();
                    let mut fields = Vec::new();
                    if !self.at(&TokenKind::RBrace) {
                        loop {
                            let field_tok = match self.peek_kind() {
                                TokenKind::Ident(_) => self.advance(),
                                _ => {
                                    let tok = self.peek().clone();
                                    self.error_at(
                                        codes::UNEXPECTED_TOKEN,
                                        format!(
                                            "expected a field name, found {}",
                                            tok.kind.describe()
                                        ),
                                        tok.span,
                                    );
                                    return Err(());
                                }
                            };
                            let field_name = match &field_tok.kind {
                                TokenKind::Ident(n) => n.clone(),
                                _ => unreachable!(),
                            };
                            self.expect(&TokenKind::Colon, "`:` after the field name")?;
                            let ty = self.parse_type()?;
                            fields.push(StructFieldDecl {
                                name: field_name,
                                name_span: field_tok.span,
                                ty,
                            });
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                            if self.at(&TokenKind::RBrace) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RBrace, "`}`")?;
                    EnumPayload::Named(fields)
                } else {
                    EnumPayload::Unit
                };
                variants.push(EnumVariantDecl {
                    name: variant_name,
                    name_span: var_tok.span,
                    payload,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                if self.at(&TokenKind::RBrace) {
                    break;
                }
            }
        }
        let end = self.expect(&TokenKind::RBrace, "`}`")?.span.end;
        Ok(EnumDecl {
            name,
            name_span: name_tok.span,
            params,
            variants,
            span: Span::new(start, end),
            is_pub,
            canonical: String::new(),
        })
    }

    fn parse_type(&mut self) -> Result<TypeExpr, ()> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Ident(name) => {
                self.advance();
                let mut full = name;
                let mut span = tok.span;
                while self.at(&TokenKind::ColonColon) {
                    self.advance();
                    let seg = match self.peek_kind() {
                        TokenKind::Ident(_) => self.advance(),
                        _ => {
                            let t = self.peek().clone();
                            self.error_at(
                                codes::UNEXPECTED_TOKEN,
                                format!("expected a name after `::`, found {}", t.kind.describe()),
                                t.span,
                            );
                            return Err(());
                        }
                    };
                    if let TokenKind::Ident(n) = &seg.kind {
                        full.push_str("::");
                        full.push_str(n);
                        span = span.merge(&seg.span);
                    }
                }
                if self.at(&TokenKind::Lt) {
                    self.advance();
                    let mut args = Vec::new();
                    loop {
                        args.push(self.parse_type()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let end = self
                        .expect(&TokenKind::Gt, "`>` to close the type arguments")?
                        .span
                        .end;
                    Ok(TypeExpr {
                        kind: TypeExprKind::Generic(full, args),
                        span: Span::new(span.start, end),
                    })
                } else {
                    Ok(TypeExpr {
                        kind: TypeExprKind::Named(full),
                        span,
                    })
                }
            }
            TokenKind::LBracket => {
                let start = self.advance().span.start;
                let inner = self.parse_type()?;
                let end = self.expect(&TokenKind::RBracket, "`]`")?.span.end;
                Ok(TypeExpr {
                    kind: TypeExprKind::Array(Box::new(inner)),
                    span: Span::new(start, end),
                })
            }
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!("expected a type, found {}", tok.kind.describe()),
                    tok.span,
                );
                Err(())
            }
        }
    }

    fn parse_block(&mut self) -> Result<Block, ()> {
        let start = self.expect(&TokenKind::LBrace, "`{`")?.span.start;
        let mut stmts = Vec::new();
        loop {
            if self.at(&TokenKind::RBrace) {
                break;
            }
            if self.at_eof() {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    "expected `}` to close the block, found end of file",
                    tok.span,
                );
                break;
            }
            match self.parse_stmt() {
                Ok(stmt) => stmts.push(stmt),
                Err(()) => {
                    self.synchronize();
                    if self.at(&TokenKind::RBrace) {
                        break;
                    }
                }
            }
        }
        let end = self.expect(&TokenKind::RBrace, "`}`")?.span.end;
        Ok(Block {
            stmts,
            span: Span::new(start, end),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ()> {
        match self.peek_kind() {
            TokenKind::Let | TokenKind::Var | TokenKind::Const => {
                let decl = self.parse_var_decl(true)?;
                Ok(Stmt::VarDecl(decl))
            }
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Unsafe => {
                self.advance();
                let block = self.parse_block()?;
                Ok(Stmt::Unsafe(block))
            }
            TokenKind::Return => {
                let start = self.advance().span.start;
                let value = if self.at(&TokenKind::Semicolon) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                let end = self
                    .expect_semicolon()
                    .map(|_| self.prev_end())
                    .unwrap_or_else(|_| {
                        value
                            .as_ref()
                            .map(|v| v.span.end)
                            .unwrap_or_else(|| self.peek().span.start)
                    });
                Ok(Stmt::Return {
                    value,
                    span: Span::new(start, end),
                })
            }
            TokenKind::Break => {
                let span = self.advance().span;
                self.expect_semicolon()?;
                Ok(Stmt::Break { span })
            }
            TokenKind::Continue => {
                let span = self.advance().span;
                self.expect_semicolon()?;
                Ok(Stmt::Continue { span })
            }
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                Ok(Stmt::Block(block))
            }
            TokenKind::Fn => {
                let tok = self.advance();
                self.error_at(
                    codes::UNSUPPORTED,
                    "nested functions are not supported in Bloom 0.4",
                    tok.span,
                );
                if let Some(d) = self.diagnostics.last_mut() {
                    d.help = Some("declare the function at the top level of the file".to_string());
                }
                self.recover_statement_or_block();
                Err(())
            }
            TokenKind::Struct => {
                let tok = self.advance();
                self.error_at(
                    codes::UNSUPPORTED,
                    "struct declarations are only allowed at the top level",
                    tok.span,
                );
                if let Some(d) = self.diagnostics.last_mut() {
                    d.help = Some("declare the struct outside of any function".to_string());
                }
                self.recover_statement_or_block();
                Err(())
            }
            TokenKind::Impl => {
                let tok = self.advance();
                self.error_at(
                    codes::UNSUPPORTED,
                    "impl blocks are only allowed at the top level",
                    tok.span,
                );
                if let Some(d) = self.diagnostics.last_mut() {
                    d.help = Some("declare the impl block outside of any function".to_string());
                }
                self.recover_statement_or_block();
                Err(())
            }
            TokenKind::Class | TokenKind::Enum | TokenKind::Use | TokenKind::Import => {
                let tok = self.advance();
                self.error_at(
                    codes::UNSUPPORTED,
                    format!("{} is not supported in Bloom 0.4", tok.kind.describe()),
                    tok.span,
                );
                self.recover_statement_or_block();
                Err(())
            }
            _ => {
                let expr = self.parse_expr()?;
                let end = expr.span.end;
                let start = expr.span.start;
                if matches!(expr.kind, ExprKind::Match { .. })
                    && !self.at(&TokenKind::Semicolon)
                    && (self.at(&TokenKind::RBrace) || self.at_eof())
                {
                    return Ok(Stmt::Expr {
                        expr,
                        span: Span::new(start, end),
                    });
                }
                self.expect_semicolon()?;
                Ok(Stmt::Expr {
                    expr,
                    span: Span::new(start, end),
                })
            }
        }
    }

    fn prev_end(&self) -> Pos {
        self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)]
            .span
            .end
    }

    fn parse_var_decl(&mut self, require_init: bool) -> Result<VarDecl, ()> {
        let kind_tok = self.advance();
        let kind = match kind_tok.kind {
            TokenKind::Let => DeclKind::Let,
            TokenKind::Var => DeclKind::Var,
            TokenKind::Const => DeclKind::Const,
            _ => unreachable!(),
        };
        let start = kind_tok.span.start;
        let name_tok = match self.peek_kind() {
            TokenKind::Ident(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!("expected a variable name, found {}", tok.kind.describe()),
                    tok.span,
                );
                return Err(());
            }
        };
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            _ => unreachable!(),
        };
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let init = if self.eat(&TokenKind::Eq) {
            Some(self.parse_expr()?)
        } else if require_init || ty.is_none() {
            let tok = self.peek().clone();
            self.error_at(
                codes::UNEXPECTED_TOKEN,
                format!(
                    "declaration of `{}` requires an initializer (`= value`)",
                    name
                ),
                tok.span,
            );
            if let Some(d) = self.diagnostics.last_mut() {
                d.help = Some(format!("write e.g. `let {}: int = 0;`", name));
            }
            return Err(());
        } else {
            None
        };
        let end = init
            .as_ref()
            .map(|e| e.span.end)
            .unwrap_or(name_tok.span.end);
        let _ = require_init;
        self.expect_semicolon()?;
        Ok(VarDecl {
            id: self.fresh_decl_id(),
            kind,
            name,
            name_span: name_tok.span,
            ty,
            init,
            span: Span::new(start, end),
        })
    }

    fn parse_if(&mut self) -> Result<Stmt, ()> {
        let start = self.expect(&TokenKind::If, "`if`")?.span.start;
        let cond = self.parse_expr_no_struct_lit()?;
        let then_block = self.parse_block()?;
        let else_branch = if self.eat(&TokenKind::Else) {
            if self.at(&TokenKind::If) {
                Some(ElseBranch::If(Box::new(self.parse_if()?)))
            } else if self.at(&TokenKind::LBrace) {
                Some(ElseBranch::Block(self.parse_block()?))
            } else {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!(
                        "expected `if` or `{{` after `else`, found {}",
                        tok.kind.describe()
                    ),
                    tok.span,
                );
                return Err(());
            }
        } else {
            None
        };
        let end = match &else_branch {
            Some(ElseBranch::If(s)) => stmt_end(s),
            Some(ElseBranch::Block(b)) => b.span.end,
            None => then_block.span.end,
        };
        Ok(Stmt::If {
            cond,
            then_block,
            else_branch,
            span: Span::new(start, end),
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, ()> {
        let start = self.expect(&TokenKind::While, "`while`")?.span.start;
        let cond = self.parse_expr_no_struct_lit()?;
        let body = self.parse_block()?;
        let span = Span::new(start, body.span.end);
        Ok(Stmt::While { cond, body, span })
    }

    fn parse_for(&mut self) -> Result<Stmt, ()> {
        let start = self.expect(&TokenKind::For, "`for`")?.span.start;
        let name_tok = match self.peek_kind() {
            TokenKind::Ident(_) => self.advance(),
            _ => {
                let tok = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!(
                        "expected a loop variable name, found {}",
                        tok.kind.describe()
                    ),
                    tok.span,
                );
                return Err(());
            }
        };
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            _ => unreachable!(),
        };
        self.expect(&TokenKind::In, "`in`")?;
        let range_start = self.parse_expr_no_struct_lit()?;
        if !self.eat(&TokenKind::DotDot) {
            let tok = self.peek().clone();
            self.error_at(
                codes::UNEXPECTED_TOKEN,
                format!(
                    "expected `..` in for-range (form: `for i in 0..10`), found {}",
                    tok.kind.describe()
                ),
                tok.span,
            );
            return Err(());
        }
        let range_end = self.parse_expr_no_struct_lit()?;
        let body = self.parse_block()?;
        let span = Span::new(start, body.span.end);
        let decl = VarDecl {
            id: self.fresh_decl_id(),
            kind: DeclKind::Let,
            name,
            name_span: name_tok.span,
            ty: None,
            init: None,
            span: name_tok.span,
        };
        Ok(Stmt::For {
            decl,
            range_start,
            range_end,
            body,
            span,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ()> {
        self.parse_assign()
    }

    fn parse_expr_no_struct_lit(&mut self) -> Result<Expr, ()> {
        let old = self.struct_lits_allowed;
        self.struct_lits_allowed = false;
        let result = self.parse_expr();
        self.struct_lits_allowed = old;
        result
    }

    fn parse_expr_with_struct_lit(&mut self) -> Result<Expr, ()> {
        let old = self.struct_lits_allowed;
        self.struct_lits_allowed = true;
        let result = self.parse_expr();
        self.struct_lits_allowed = old;
        result
    }

    fn parse_assign(&mut self) -> Result<Expr, ()> {
        let lhs = self.parse_or()?;
        if self.at(&TokenKind::Eq) {
            self.advance();
            let rhs = self.parse_assign()?;
            let span = lhs.span.merge(&rhs.span);
            let id = self.fresh_expr_id();
            return Ok(Expr {
                id,
                kind: ExprKind::Assign {
                    target: Box::new(lhs),
                    value: Box::new(rhs),
                },
                span,
            });
        }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> Result<Expr, ()> {
        let mut lhs = self.parse_and()?;
        while self.at(&TokenKind::PipePipe) {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = self.make_binary(BinOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, ()> {
        let mut lhs = self.parse_equality()?;
        while self.at(&TokenKind::AmpAmp) {
            self.advance();
            let rhs = self.parse_equality()?;
            lhs = self.make_binary(BinOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, ()> {
        let mut lhs = self.parse_comparison()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::BangEq => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_comparison()?;
            lhs = self.make_binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ()> {
        let mut lhs = self.parse_term()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::LtEq => BinOp::Le,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::GtEq => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_term()?;
            lhs = self.make_binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> Result<Expr, ()> {
        let mut lhs = self.parse_factor()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_factor()?;
            lhs = self.make_binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_factor(&mut self) -> Result<Expr, ()> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            lhs = self.make_binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ()> {
        match self.peek_kind() {
            TokenKind::Minus => {
                let start = self.advance().span.start;
                let operand = self.parse_unary()?;
                let span = Span::new(start, operand.span.end);
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Unary {
                        op: UnaryOp::Neg,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::Bang => {
                let start = self.advance().span.start;
                let operand = self.parse_unary()?;
                let span = Span::new(start, operand.span.end);
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ()> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokenKind::LParen => {
                    self.advance();
                    let type_args = match &expr.kind {
                        ExprKind::Path(_, targs) => targs.clone(),
                        _ => Vec::new(),
                    };
                    let mut args = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr_with_struct_lit()?);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    let end = self.expect(&TokenKind::RParen, "`)`")?.span.end;
                    let span = Span::new(expr.span.start, end);
                    let id = self.fresh_expr_id();
                    expr = Expr {
                        id,
                        kind: ExprKind::Call {
                            callee: Box::new(expr),
                            type_args,
                            args,
                        },
                        span,
                    };
                }
                TokenKind::Question => {
                    self.advance();
                    let span = expr.span;
                    let id = self.fresh_expr_id();
                    expr = Expr {
                        id,
                        kind: ExprKind::Try(Box::new(expr)),
                        span,
                    };
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr_with_struct_lit()?;
                    let end = self.expect(&TokenKind::RBracket, "`]`")?.span.end;
                    let span = Span::new(expr.span.start, end);
                    let id = self.fresh_expr_id();
                    expr = Expr {
                        id,
                        kind: ExprKind::Index {
                            base: Box::new(expr),
                            index: Box::new(index),
                        },
                        span,
                    };
                }
                TokenKind::Dot => {
                    self.advance();
                    let field_tok = match self.peek_kind() {
                        TokenKind::Ident(_) => self.advance(),
                        _ => {
                            let tok = self.peek().clone();
                            self.error_at(
                                codes::UNEXPECTED_TOKEN,
                                format!(
                                    "expected a field name after `.`, found {}",
                                    tok.kind.describe()
                                ),
                                tok.span,
                            );
                            return Err(());
                        }
                    };
                    let field_name = match &field_tok.kind {
                        TokenKind::Ident(n) => n.clone(),
                        _ => unreachable!(),
                    };
                    let span = Span::new(expr.span.start, field_tok.span.end);
                    let id = self.fresh_expr_id();
                    expr = Expr {
                        id,
                        kind: ExprKind::Field {
                            base: Box::new(expr),
                            name: field_name,
                            name_span: field_tok.span,
                        },
                        span,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ()> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::IntLiteral(v) => {
                self.advance();
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Int(v),
                    span: tok.span,
                })
            }
            TokenKind::FloatLiteral(v) => {
                self.advance();
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Float(v),
                    span: tok.span,
                })
            }
            TokenKind::StringLiteral(s) => {
                self.advance();
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Str(s),
                    span: tok.span,
                })
            }
            TokenKind::CharLiteral(c) => {
                self.advance();
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Char(c),
                    span: tok.span,
                })
            }
            TokenKind::True => {
                self.advance();
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Bool(true),
                    span: tok.span,
                })
            }
            TokenKind::False => {
                self.advance();
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Bool(false),
                    span: tok.span,
                })
            }
            TokenKind::Ident(name) => {
                if self.struct_lits_allowed && matches!(self.peek_at(1), TokenKind::LBrace) {
                    return self.parse_struct_lit();
                }
                self.advance();
                let mut segments = vec![name.clone()];
                let mut path_span = tok.span;
                let mut type_args: Vec<TypeExpr> = Vec::new();
                while self.at(&TokenKind::ColonColon) {
                    self.advance();
                    if self.at(&TokenKind::Lt) {
                        self.advance();
                        loop {
                            type_args.push(self.parse_type()?);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                        let end = self
                            .expect(&TokenKind::Gt, "`>` to close the type arguments")?
                            .span
                            .end;
                        path_span = Span::new(path_span.start, end);
                        break;
                    }
                    let seg_tok = match self.peek_kind() {
                        TokenKind::Ident(_) => self.advance(),
                        _ => {
                            let t = self.peek().clone();
                            self.error_at(
                                codes::UNEXPECTED_TOKEN,
                                format!("expected a name after `::`, found {}", t.kind.describe()),
                                t.span,
                            );
                            return Err(());
                        }
                    };
                    if let TokenKind::Ident(seg) = &seg_tok.kind {
                        segments.push(seg.clone());
                    }
                    path_span = path_span.merge(&seg_tok.span);
                }
                if !type_args.is_empty() {
                    let id = self.fresh_expr_id();
                    return Ok(Expr {
                        id,
                        kind: ExprKind::Path(segments, type_args),
                        span: path_span,
                    });
                }
                if segments.len() > 1 {
                    if self.struct_lits_allowed && self.at(&TokenKind::LBrace) {
                        return self.parse_struct_lit_body(segments.join("::"), path_span);
                    }
                    let id = self.fresh_expr_id();
                    return Ok(Expr {
                        id,
                        kind: ExprKind::Path(segments, Vec::new()),
                        span: path_span,
                    });
                }
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Ident(name),
                    span: tok.span,
                })
            }
            TokenKind::SelfValue => {
                self.advance();
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Ident("self".to_string()),
                    span: tok.span,
                })
            }
            TokenKind::Match => {
                let start = self.advance().span.start;
                let scrutinee = self.parse_expr_no_struct_lit()?;
                self.expect(&TokenKind::LBrace, "`{` after the match value")?;
                let mut arms = Vec::new();
                while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                    let pattern = self.parse_pattern()?;
                    let guard = if self.eat(&TokenKind::If) {
                        Some(self.parse_expr_with_struct_lit()?)
                    } else {
                        None
                    };
                    self.expect(&TokenKind::FatArrow, "`=>` after the pattern")?;
                    let body = self.parse_expr_with_struct_lit()?;
                    arms.push(MatchArm {
                        pattern,
                        guard,
                        body,
                    });
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                let end = self.expect(&TokenKind::RBrace, "`}`")?.span.end;
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::Match {
                        scrutinee: Box::new(scrutinee),
                        arms,
                    },
                    span: Span::new(start, end),
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr_with_struct_lit()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                let start = self.advance().span.start;
                let mut elems = Vec::new();
                if !self.at(&TokenKind::RBracket) {
                    loop {
                        elems.push(self.parse_expr_with_struct_lit()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                        if self.at(&TokenKind::RBracket) {
                            break;
                        }
                    }
                }
                let end = self.expect(&TokenKind::RBracket, "`]`")?.span.end;
                let id = self.fresh_expr_id();
                Ok(Expr {
                    id,
                    kind: ExprKind::ArrayLit(elems),
                    span: Span::new(start, end),
                })
            }
            _ => {
                let msg = format!("expected an expression, found {}", tok.kind.describe());
                self.error_at(codes::UNEXPECTED_TOKEN, msg, tok.span);
                Err(())
            }
        }
    }

    fn parse_struct_lit(&mut self) -> Result<Expr, ()> {
        let name_tok = self.advance();
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            _ => unreachable!(),
        };
        self.parse_struct_lit_body(name, name_tok.span)
    }

    fn parse_struct_lit_body(&mut self, name: String, name_span: Span) -> Result<Expr, ()> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        if !self.at(&TokenKind::RBrace) {
            loop {
                let field_tok = match self.peek_kind() {
                    TokenKind::Ident(_) => self.advance(),
                    _ => {
                        let tok = self.peek().clone();
                        self.error_at(
                            codes::UNEXPECTED_TOKEN,
                            format!("expected a field name, found {}", tok.kind.describe()),
                            tok.span,
                        );
                        return Err(());
                    }
                };
                let field_name = match &field_tok.kind {
                    TokenKind::Ident(n) => n.clone(),
                    _ => unreachable!(),
                };
                self.expect(&TokenKind::Colon, "`:` after the field name")?;
                let value = self.parse_expr_with_struct_lit()?;
                fields.push(StructLitField {
                    name: field_name,
                    name_span: field_tok.span,
                    value,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                if self.at(&TokenKind::RBrace) {
                    break;
                }
            }
        }
        let end = self.expect(&TokenKind::RBrace, "`}`")?.span.end;
        let id = self.fresh_expr_id();
        Ok(Expr {
            id,
            kind: ExprKind::StructLit {
                name,
                name_span,
                fields,
            },
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ()> {
        let first = self.parse_pattern_atom()?;
        if !self.at(&TokenKind::Pipe) {
            return Ok(first);
        }
        let start = match &first {
            Pattern::Wild(s) => s.start,
            Pattern::Binding(_, s, _) => s.start,
            Pattern::Literal(_, s) => s.start,
            Pattern::Or(_, s) => s.start,
            Pattern::Variant { span, .. } => span.start,
        };
        let mut alternatives = vec![first];
        let mut end = start;
        while self.eat(&TokenKind::Pipe) {
            let next = self.parse_pattern_atom()?;
            end = match &next {
                Pattern::Wild(s) => s.end,
                Pattern::Binding(_, s, _) => s.end,
                Pattern::Literal(_, s) => s.end,
                Pattern::Or(_, s) => s.end,
                Pattern::Variant { span, .. } => span.end,
            };
            alternatives.push(next);
        }
        Ok(Pattern::Or(alternatives, Span::new(start, end)))
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern, ()> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::IntLiteral(v) => {
                self.advance();
                Ok(Pattern::Literal(LiteralPattern::Int(v), tok.span))
            }
            TokenKind::StringLiteral(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(Pattern::Literal(LiteralPattern::Str(s), tok.span))
            }
            TokenKind::CharLiteral(c) => {
                self.advance();
                Ok(Pattern::Literal(LiteralPattern::Char(c), tok.span))
            }
            TokenKind::True => {
                self.advance();
                Ok(Pattern::Literal(LiteralPattern::Bool(true), tok.span))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pattern::Literal(LiteralPattern::Bool(false), tok.span))
            }
            TokenKind::Minus => {
                self.advance();
                match self.peek_kind() {
                    TokenKind::IntLiteral(v) => {
                        let v = *v;
                        self.advance();
                        Ok(Pattern::Literal(LiteralPattern::Int(-v), tok.span))
                    }
                    _ => {
                        let t = self.peek().clone();
                        self.error_at(
                            codes::UNEXPECTED_TOKEN,
                            format!(
                                "expected a number after `-` in pattern, found {}",
                                t.kind.describe()
                            ),
                            t.span,
                        );
                        Err(())
                    }
                }
            }
            TokenKind::Ident(name) => {
                self.advance();
                let mut segments = vec![name];
                let mut path_span = tok.span;
                while self.at(&TokenKind::ColonColon) {
                    self.advance();
                    let seg_tok = match self.peek_kind() {
                        TokenKind::Ident(_) => self.advance(),
                        _ => {
                            let t = self.peek().clone();
                            self.error_at(
                                codes::UNEXPECTED_TOKEN,
                                format!("expected a name after `::`, found {}", t.kind.describe()),
                                t.span,
                            );
                            return Err(());
                        }
                    };
                    if let TokenKind::Ident(seg) = &seg_tok.kind {
                        segments.push(seg.clone());
                    }
                    path_span = path_span.merge(&seg_tok.span);
                }
                if segments.len() == 1 {
                    if segments[0] == "_" {
                        return Ok(Pattern::Wild(tok.span));
                    }
                    return Ok(Pattern::Binding(
                        segments[0].clone(),
                        tok.span,
                        self.fresh_decl_id(),
                    ));
                }
                let mut args = Vec::new();
                let mut named = Vec::new();
                if self.at(&TokenKind::LParen) {
                    self.advance();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_pattern()?);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen, "`)`")?;
                } else if self.at(&TokenKind::LBrace) {
                    self.advance();
                    if !self.at(&TokenKind::RBrace) {
                        loop {
                            let field_tok = match self.peek_kind() {
                                TokenKind::Ident(_) => self.advance(),
                                _ => {
                                    let t = self.peek().clone();
                                    self.error_at(
                                        codes::UNEXPECTED_TOKEN,
                                        format!(
                                            "expected a field name, found {}",
                                            t.kind.describe()
                                        ),
                                        t.span,
                                    );
                                    return Err(());
                                }
                            };
                            let field_name = match &field_tok.kind {
                                TokenKind::Ident(n) => n.clone(),
                                _ => unreachable!(),
                            };
                            let sub = if self.eat(&TokenKind::Colon) {
                                self.parse_pattern()?
                            } else {
                                if field_name == "_" {
                                    Pattern::Wild(field_tok.span)
                                } else {
                                    Pattern::Binding(
                                        field_name.clone(),
                                        field_tok.span,
                                        self.fresh_decl_id(),
                                    )
                                }
                            };
                            named.push(NamedPatternField {
                                name: field_name,
                                name_span: field_tok.span,
                                pattern: sub,
                            });
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                            if self.at(&TokenKind::RBrace) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RBrace, "`}`")?;
                }
                let end = self.prev_end();
                Ok(Pattern::Variant {
                    path: segments,
                    name_span: path_span,
                    args,
                    named,
                    span: Span::new(path_span.start, end),
                })
            }
            _ => {
                let t = self.peek().clone();
                self.error_at(
                    codes::UNEXPECTED_TOKEN,
                    format!("expected a pattern, found {}", t.kind.describe()),
                    t.span,
                );
                Err(())
            }
        }
    }

    fn make_binary(&mut self, op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
        let span = lhs.span.merge(&rhs.span);
        let id = self.fresh_expr_id();
        Expr {
            id,
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            span,
        }
    }
}

fn stmt_end(stmt: &Stmt) -> Pos {
    match stmt {
        Stmt::VarDecl(d) => d.span.end,
        Stmt::Expr { span, .. } => span.end,
        Stmt::If { span, .. } => span.end,
        Stmt::While { span, .. } => span.end,
        Stmt::For { span, .. } => span.end,
        Stmt::Return { span, .. } => span.end,
        Stmt::Break { span } => span.end,
        Stmt::Continue { span } => span.end,
        Stmt::Block(b) => b.span.end,
        Stmt::Unsafe(b) => b.span.end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_src(src: &str) -> ParseOutput {
        parse(lex(src).tokens)
    }

    fn parse_ok(src: &str) -> Program {
        let out = parse_src(src);
        assert!(
            out.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            out.diagnostics
        );
        out.program
    }

    fn parse_errs(src: &str) -> Vec<Diagnostic> {
        let out = parse_src(src);
        assert!(!out.diagnostics.is_empty());
        out.diagnostics
    }

    #[test]
    fn parses_hello_world() {
        let program = parse_ok("fn main() -> int { println(\"hi\"); return 0; }");
        assert_eq!(program.items.len(), 1);
        match &program.items[0] {
            Item::Function(f) => {
                assert_eq!(f.name, "main");
                assert_eq!(f.body.stmts.len(), 2);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parses_precedence() {
        let program = parse_ok("fn main() -> int { let x = 1 + 2 * 3; return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        let init = decl.init.as_ref().unwrap();
        match &init.kind {
            ExprKind::Binary { op, lhs, rhs } => {
                assert_eq!(*op, BinOp::Add);
                assert!(matches!(lhs.kind, ExprKind::Int(1)));
                match &rhs.kind {
                    ExprKind::Binary { op, .. } => assert_eq!(*op, BinOp::Mul),
                    _ => panic!("expected multiplication"),
                }
            }
            _ => panic!("expected addition"),
        }
    }

    #[test]
    fn parses_comparison_and_logic() {
        let program =
            parse_ok("fn main() -> int { if a < b && c != d || e >= f { return 0; } return 1; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::If { cond, .. } = &f.body.stmts[0] else {
            panic!()
        };
        match &cond.kind {
            ExprKind::Binary { op, .. } => assert_eq!(*op, BinOp::Or),
            _ => panic!("expected or at the top"),
        }
    }

    #[test]
    fn parses_for_range() {
        let program = parse_ok("fn main() -> int { for i in 0..10 { println(i); } return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        match &f.body.stmts[0] {
            Stmt::For {
                decl,
                range_start,
                range_end,
                ..
            } => {
                assert_eq!(decl.name, "i");
                assert!(matches!(range_start.kind, ExprKind::Int(0)));
                assert!(matches!(range_end.kind, ExprKind::Int(10)));
            }
            _ => panic!("expected for loop"),
        }
    }

    #[test]
    fn parses_arrays_and_calls() {
        let program = parse_ok("fn main() -> int { let a = [1, 2, 3]; println(a[0]); return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        assert!(matches!(
            decl.init.as_ref().unwrap().kind,
            ExprKind::ArrayLit(_)
        ));
        let Stmt::Expr { expr, .. } = &f.body.stmts[1] else {
            panic!()
        };
        match &expr.kind {
            ExprKind::Call { args, .. } => {
                assert!(matches!(args[0].kind, ExprKind::Index { .. }));
            }
            _ => panic!("expected call"),
        }
    }

    #[test]
    fn parses_functions_with_params() {
        let program = parse_ok(
            "fn add(a: int, b: int) -> int { return a + b; }\nfn main() -> int { return add(1, 2); }",
        );
        assert_eq!(program.items.len(), 2);
    }

    #[test]
    fn reports_missing_semicolon() {
        let diags = parse_errs("fn main() -> int { let x = 1 return 0; }");
        assert!(diags.iter().any(|d| d.message.contains("`;`")));
    }

    #[test]
    fn reports_missing_brace() {
        let diags = parse_errs("fn main() -> int { return 0;");
        assert!(diags
            .iter()
            .any(|d| d.message.contains("expected `}`") || d.message.contains("expected `)`")));
    }

    #[test]
    fn parses_use_declaration() {
        let program = parse_ok("use geometry::shapes;\nfn main() -> int { return 0; }");
        let Item::Use(u) = &program.items[0] else {
            panic!("expected use item")
        };
        assert_eq!(u.path, vec!["geometry".to_string(), "shapes".to_string()]);
    }

    #[test]
    fn reports_bad_use_declaration() {
        let diags = parse_errs("use ;\nfn main() -> int { return 0; }");
        assert!(!diags.is_empty());
    }

    #[test]
    fn rejects_class() {
        let diags = parse_errs("class Player { }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNSUPPORTED));
    }

    #[test]
    fn recovers_after_error() {
        let out = parse_src("fn main() -> int { let x = ; let y = 2; return 0; }");
        assert!(!out.diagnostics.is_empty());
        let Item::Function(f) = &out.program.items[0] else {
            panic!()
        };
        assert!(f.body.stmts.len() >= 2);
    }

    #[test]
    fn parses_else_if_chain() {
        let program = parse_ok("fn main() -> int { if a { } else if b { } else { } return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::If { else_branch, .. } = &f.body.stmts[0] else {
            panic!()
        };
        assert!(matches!(else_branch, Some(ElseBranch::If(_))));
    }

    #[test]
    fn parses_struct_declaration() {
        let program =
            parse_ok("struct Point {\n    x: int,\n    y: int\n}\nfn main() -> int { return 0; }");
        let Item::Struct(s) = &program.items[0] else {
            panic!("expected struct item")
        };
        assert_eq!(s.name, "Point");
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[0].name, "x");
        assert!(matches!(
            s.fields[0].ty.kind,
            TypeExprKind::Named(ref n) if n == "int"
        ));
    }

    #[test]
    fn parses_struct_declaration_with_trailing_comma_and_array_field() {
        let program = parse_ok(
            "struct Bag {\n    items: [int],\n    label: string,\n}\nfn main() -> int { return 0; }",
        );
        let Item::Struct(s) = &program.items[0] else {
            panic!()
        };
        assert_eq!(s.fields.len(), 2);
        assert!(matches!(s.fields[0].ty.kind, TypeExprKind::Array(_)));
    }

    #[test]
    fn parses_struct_literal() {
        let program = parse_ok(
            "struct Point { x: int, y: int }\nfn main() -> int { let p = Point { x: 10, y: 20 }; return 0; }",
        );
        let Item::Function(f) = &program.items[1] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        match &decl.init.as_ref().unwrap().kind {
            ExprKind::StructLit { name, fields, .. } => {
                assert_eq!(name, "Point");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "x");
                assert!(matches!(fields[0].value.kind, ExprKind::Int(10)));
            }
            _ => panic!("expected struct literal"),
        }
    }

    #[test]
    fn parses_nested_struct_literal() {
        let program = parse_ok(
            "struct Inner { v: int }\nstruct Outer { inner: Inner, n: int }\nfn main() -> int { let o = Outer { inner: Inner { v: 1 }, n: 2 }; return 0; }",
        );
        let Item::Function(f) = &program.items[2] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        match &decl.init.as_ref().unwrap().kind {
            ExprKind::StructLit { fields, .. } => {
                assert!(matches!(fields[0].value.kind, ExprKind::StructLit { .. }));
            }
            _ => panic!("expected struct literal"),
        }
    }

    #[test]
    fn parses_field_access_chain() {
        let program = parse_ok("fn main() -> int { println(a.b.c); return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::Expr { expr, .. } = &f.body.stmts[0] else {
            panic!()
        };
        let ExprKind::Call { args, .. } = &expr.kind else {
            panic!()
        };
        match &args[0].kind {
            ExprKind::Field { base, name, .. } => {
                assert_eq!(name, "c");
                assert!(matches!(base.kind, ExprKind::Field { .. }));
            }
            _ => panic!("expected field access"),
        }
    }

    #[test]
    fn parses_field_assignment() {
        let program = parse_ok("fn main() -> int { p.x = 5; return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::Expr { expr, .. } = &f.body.stmts[0] else {
            panic!()
        };
        match &expr.kind {
            ExprKind::Assign { target, .. } => {
                assert!(matches!(target.kind, ExprKind::Field { .. }));
            }
            _ => panic!("expected assignment"),
        }
    }

    #[test]
    fn field_access_after_index_and_call() {
        let program = parse_ok("fn main() -> int { println(make().p.x); return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::Expr { expr, .. } = &f.body.stmts[0] else {
            panic!()
        };
        let ExprKind::Call { args, .. } = &expr.kind else {
            panic!()
        };
        match &args[0].kind {
            ExprKind::Field { base, .. } => {
                assert!(matches!(base.kind, ExprKind::Field { .. }));
            }
            _ => panic!("expected field access on call result"),
        }
    }

    #[test]
    fn struct_literal_not_consumed_in_if_condition() {
        let out = parse_src("fn main() -> int { if p { return 0; } return 1; }");
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        let Item::Function(f) = &out.program.items[0] else {
            panic!()
        };
        let Stmt::If { cond, .. } = &f.body.stmts[0] else {
            panic!()
        };
        assert!(matches!(cond.kind, ExprKind::Ident(_)));
    }

    #[test]
    fn struct_literal_allowed_inside_parentheses_in_condition() {
        let out =
            parse_src("fn main() -> int { if (Point { x: 1 }).x > 0 { return 0; } return 1; }");
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        let Item::Function(f) = &out.program.items[0] else {
            panic!()
        };
        let Stmt::If { cond, .. } = &f.body.stmts[0] else {
            panic!()
        };
        assert!(matches!(cond.kind, ExprKind::Binary { .. }));
    }

    #[test]
    fn reports_missing_colon_in_struct_field() {
        let diags =
            parse_errs("struct Point {\n    x int,\n    y: int\n}\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.message.contains("`:`")));
    }

    #[test]
    fn reports_missing_field_name_in_literal() {
        let diags = parse_errs("fn main() -> int { let p = Point { 5 }; return 0; }");
        assert!(!diags.is_empty());
    }

    #[test]
    fn reports_struct_inside_function() {
        let diags = parse_errs("fn main() -> int { struct P { x: int } return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNSUPPORTED));
    }

    #[test]
    fn reports_bad_field_access() {
        let diags = parse_errs("fn main() -> int { println(p.); return 0; }");
        assert!(!diags.is_empty());
    }

    #[test]
    fn parses_impl_block_with_methods() {
        let program = parse_ok(
            "struct P { x: int }\nimpl P {\n    fn get(self) -> int {\n        return self.x;\n    }\n    fn set(var self, v: int) -> void {\n        self.x = v;\n    }\n}\nfn main() -> int { return 0; }",
        );
        let Item::Impl(b) = &program.items[1] else {
            panic!("expected impl item")
        };
        assert!(matches!(
            &b.target.kind,
            TypeExprKind::Named(n) if n == "P"
        ));
        assert_eq!(b.methods.len(), 2);
        assert_eq!(b.methods[0].name, "get");
        assert_eq!(b.methods[0].receiver, Some(ReceiverKind::Value));
        assert_eq!(b.methods[1].receiver, Some(ReceiverKind::Mutable));
        assert_eq!(b.methods[1].params.len(), 1);
    }

    #[test]
    fn parses_method_calls_and_chains() {
        let program =
            parse_ok("fn main() -> int { let x = p.translated(1, 2).magnitude(); return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        match &decl.init.as_ref().unwrap().kind {
            ExprKind::Call { callee, args, .. } => {
                assert_eq!(args.len(), 0);
                assert!(matches!(callee.kind, ExprKind::Field { .. }));
            }
            other => panic!("expected call, got {:?}", other),
        }
    }

    #[test]
    fn parses_associated_function() {
        let program = parse_ok(
            "struct P { x: int }\nimpl P {\n    fn make() -> P {\n        return P { x: 1 };\n    }\n}\nfn main() -> int { return 0; }",
        );
        let Item::Impl(b) = &program.items[1] else {
            panic!()
        };
        assert!(b.methods[0].receiver.is_none());
    }

    #[test]
    fn parses_generic_struct_declaration() {
        let program = parse_ok(
            "struct Pair<A, B> {\n    first: A,\n    second: B\n}\nfn main() -> int { return 0; }",
        );
        let Item::Struct(s) = &program.items[0] else {
            panic!()
        };
        assert_eq!(s.params, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn parses_generic_function_declaration() {
        let program = parse_ok(
            "fn identity<T>(value: T) -> T {\n    return value;\n}\nfn main() -> int { return 0; }",
        );
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        assert_eq!(f.type_params, vec!["T".to_string()]);
    }

    #[test]
    fn parses_generic_impl_block() {
        let program = parse_ok(
            "struct Box<T> { value: T }\nimpl<T> Box<T> {\n    fn get(self) -> T {\n        return self.value;\n    }\n}\nfn main() -> int { return 0; }",
        );
        let Item::Impl(b) = &program.items[1] else {
            panic!()
        };
        assert_eq!(b.params, vec!["T".to_string()]);
        match &b.target.kind {
            TypeExprKind::Generic(name, args) => {
                assert_eq!(name, "Box");
                assert_eq!(args.len(), 1);
            }
            _ => panic!("expected generic impl target"),
        }
    }

    #[test]
    fn parses_turbofish_call() {
        let program = parse_ok("fn main() -> int {\n    let x = empty::<int>();\n    return 0;\n}");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        match &decl.init.as_ref().unwrap().kind {
            ExprKind::Call { type_args, .. } => {
                assert_eq!(type_args.len(), 1);
            }
            other => panic!("expected call, got {:?}", other),
        }
    }

    #[test]
    fn parses_try_operator() {
        let program = parse_ok("fn main() -> int {\n    let x = f()?;\n    return 0;\n}");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        assert!(matches!(decl.init.as_ref().unwrap().kind, ExprKind::Try(_)));
    }

    #[test]
    fn rejects_impl_inside_function() {
        let diags = parse_errs("fn main() -> int { impl P { } return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNSUPPORTED));
    }

    #[test]
    fn rejects_bad_impl_syntax() {
        let diags = parse_errs(
            "struct P { x: int }\nimpl P {\n    let x = 1;\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.message.contains("`fn`")));
    }

    #[test]
    fn parses_pub_items() {
        let program = parse_ok(
            "pub struct P { x: int }\npub fn helper() -> int { return 1; }\nfn main() -> int { return helper(); }",
        );
        let Item::Struct(s) = &program.items[0] else {
            panic!()
        };
        assert!(s.is_pub);
        let Item::Function(f) = &program.items[1] else {
            panic!()
        };
        assert!(f.is_pub);
    }

    #[test]
    fn rejects_pub_without_item() {
        let diags = parse_errs("pub let x = 1;\nfn main() -> int { return 0; }");
        assert!(!diags.is_empty());
    }

    #[test]
    fn parses_self_expression() {
        let program = parse_ok(
            "struct P { x: int }\nimpl P {\n    fn get(self) -> int {\n        return self.x;\n    }\n}\nfn main() -> int { return 0; }",
        );
        let Item::Impl(b) = &program.items[1] else {
            panic!()
        };
        let Stmt::Return { value, .. } = &b.methods[0].body.stmts[0] else {
            panic!()
        };
        let v = value.as_ref().unwrap();
        match &v.kind {
            ExprKind::Field { base, name, .. } => {
                assert_eq!(name, "x");
                assert!(matches!(base.kind, ExprKind::Ident(ref n) if n == "self"));
            }
            _ => panic!("expected field access on self"),
        }
    }

    #[test]
    fn parses_path_expressions() {
        let program = parse_ok("fn main() -> int { let d = Direction::North; return 0; }");
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        match &decl.init.as_ref().unwrap().kind {
            ExprKind::Path(segments, _) => {
                assert_eq!(
                    segments,
                    &vec!["Direction".to_string(), "North".to_string()]
                );
            }
            _ => panic!("expected path expression"),
        }
    }

    #[test]
    fn parses_generic_types() {
        let program = parse_ok(
            "fn first(x: Option<int>) -> Result<int, string> { return Result::Err(\"x\"); }\nfn main() -> int { return 0; }",
        );
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        match &f.params[0].ty.kind {
            TypeExprKind::Generic(name, args) => {
                assert_eq!(name, "Option");
                assert_eq!(args.len(), 1);
            }
            _ => panic!("expected generic type"),
        }
        match &f.ret.kind {
            TypeExprKind::Generic(name, args) => {
                assert_eq!(name, "Result");
                assert_eq!(args.len(), 2);
            }
            _ => panic!("expected generic return type"),
        }
    }

    #[test]
    fn parses_enum_declarations() {
        let program =
            parse_ok("enum Direction { North, South, East, West }\nfn main() -> int { return 0; }");
        let Item::Enum(e) = &program.items[0] else {
            panic!("expected enum item")
        };
        assert_eq!(e.name, "Direction");
        assert_eq!(e.variants.len(), 4);
        assert!(matches!(e.variants[0].payload, EnumPayload::Unit));
    }

    #[test]
    fn parses_enum_payloads() {
        let program = parse_ok(
            "enum Message {\n    Quit,\n    Move { x: int, y: int },\n    Write(string),\n}\nfn main() -> int { return 0; }",
        );
        let Item::Enum(e) = &program.items[0] else {
            panic!()
        };
        assert_eq!(e.variants.len(), 3);
        assert!(matches!(e.variants[0].payload, EnumPayload::Unit));
        match &e.variants[1].payload {
            EnumPayload::Named(fields) => assert_eq!(fields.len(), 2),
            _ => panic!("expected named payload"),
        }
        match &e.variants[2].payload {
            EnumPayload::Tuple(tys) => assert_eq!(tys.len(), 1),
            _ => panic!("expected tuple payload"),
        }
    }

    #[test]
    fn parses_generic_enum_declaration() {
        let program =
            parse_ok("enum Pair<A, B> { Both(A, B), Empty }\nfn main() -> int { return 0; }");
        let Item::Enum(e) = &program.items[0] else {
            panic!()
        };
        assert_eq!(e.params, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn parses_variant_constructions() {
        let program = parse_ok(
            "fn main() -> int {\n    let a = Direction::North;\n    let b = Message::Write(\"hi\");\n    let c = Message::Move { x: 1, y: 2 };\n    return 0;\n}",
        );
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::VarDecl(decl) = &f.body.stmts[0] else {
            panic!()
        };
        assert!(matches!(
            decl.init.as_ref().unwrap().kind,
            ExprKind::Path(_, _)
        ));
        let Stmt::VarDecl(decl) = &f.body.stmts[1] else {
            panic!()
        };
        match &decl.init.as_ref().unwrap().kind {
            ExprKind::Call { callee, args, .. } => {
                assert_eq!(args.len(), 1);
                assert!(matches!(callee.kind, ExprKind::Path(..)));
            }
            other => panic!("expected call, got {:?}", other),
        }
        let Stmt::VarDecl(decl) = &f.body.stmts[2] else {
            panic!()
        };
        match &decl.init.as_ref().unwrap().kind {
            ExprKind::StructLit { name, .. } => assert_eq!(name, "Message::Move"),
            other => panic!("expected struct literal, got {:?}", other),
        }
    }

    #[test]
    fn parses_match_expression() {
        let program = parse_ok(
            "enum E { A, B }\nfn f(e: E) -> int {\n    return match e {\n        E::A => 1,\n        E::B => 2\n    };\n}\nfn main() -> int { return 0; }",
        );
        let Item::Function(f) = &program.items[1] else {
            panic!()
        };
        let Stmt::Return { value, .. } = &f.body.stmts[0] else {
            panic!()
        };
        match &value.as_ref().unwrap().kind {
            ExprKind::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                assert!(matches!(arms[0].pattern, Pattern::Variant { .. }));
            }
            other => panic!("expected match, got {:?}", other),
        }
    }

    #[test]
    fn parses_all_pattern_kinds() {
        let program = parse_ok(
            "enum E { A, B(int), C { x: int, y: int } }\nfn f(e: E) -> int {\n    return match e {\n        E::A => 0,\n        E::B(value) => value,\n        E::C { x, y } => x + y,\n        E::B(inner) => inner,\n        _ => -1\n    };\n}\nfn main() -> int { return 0; }",
        );
        let Item::Function(f) = &program.items[1] else {
            panic!()
        };
        let Stmt::Return { value, .. } = &f.body.stmts[0] else {
            panic!()
        };
        let ExprKind::Match { arms, .. } = &value.as_ref().unwrap().kind else {
            panic!()
        };
        assert!(matches!(arms[0].pattern, Pattern::Variant { .. }));
        match &arms[1].pattern {
            Pattern::Variant { args, .. } => {
                assert!(matches!(args[0], Pattern::Binding(_, _, _)));
            }
            _ => panic!("expected variant pattern"),
        }
        match &arms[2].pattern {
            Pattern::Variant { named, .. } => {
                assert_eq!(named.len(), 2);
                assert!(matches!(named[0].pattern, Pattern::Binding(_, _, _)));
            }
            _ => panic!("expected named pattern"),
        }
        assert!(matches!(arms[4].pattern, Pattern::Wild(_)));
    }

    #[test]
    fn parses_nested_patterns() {
        let program = parse_ok(
            "fn f(o: Option<Option<int>>) -> int {\n    return match o {\n        Option::Some(Option::Some(v)) => v,\n        Option::Some(Option::None) => 0,\n        Option::None => -1\n    };\n}\nfn main() -> int { return 0; }",
        );
        let Item::Function(f) = &program.items[0] else {
            panic!()
        };
        let Stmt::Return { value, .. } = &f.body.stmts[0] else {
            panic!()
        };
        let ExprKind::Match { arms, .. } = &value.as_ref().unwrap().kind else {
            panic!()
        };
        match &arms[0].pattern {
            Pattern::Variant { args, .. } => {
                assert!(matches!(args[0], Pattern::Variant { .. }));
            }
            _ => panic!("expected nested pattern"),
        }
    }

    #[test]
    fn rejects_bad_pattern() {
        let diags = parse_errs(
            "fn f(e: int) -> int { return match e { 1.5 => 1 }; }\nfn main() -> int { return 0; }",
        );
        assert!(!diags.is_empty());
    }

    #[test]
    fn parses_literal_patterns() {
        let program = parse_ok(
            "fn f(e: int) -> int { return match e { 0 => 1, 42 => 2, -3 => 3, _ => 0 }; }\nfn main() -> int { return 0; }",
        );
        let f = program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Function(f) if f.name == "f" => Some(f),
                _ => None,
            })
            .expect("f");
        let Stmt::Return { value, .. } = &f.body.stmts[0] else {
            panic!("expected return");
        };
        let expr = value.as_ref().expect("return value");
        let ExprKind::Match { arms, .. } = &expr.kind else {
            panic!("expected match");
        };
        assert_eq!(arms.len(), 4);
        assert!(matches!(
            arms[0].pattern,
            Pattern::Literal(LiteralPattern::Int(0), _)
        ));
        assert!(matches!(
            arms[1].pattern,
            Pattern::Literal(LiteralPattern::Int(42), _)
        ));
        assert!(matches!(
            arms[2].pattern,
            Pattern::Literal(LiteralPattern::Int(-3), _)
        ));
    }

    #[test]
    fn parses_match_guard() {
        let program = parse_ok(
            "fn f(e: int) -> int { return match e { n if n > 0 => 1, _ => 0 }; }\nfn main() -> int { return 0; }",
        );
        let f = program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Function(f) if f.name == "f" => Some(f),
                _ => None,
            })
            .expect("f");
        let Stmt::Return { value, .. } = &f.body.stmts[0] else {
            panic!("expected return");
        };
        let expr = value.as_ref().expect("return value");
        let ExprKind::Match { arms, .. } = &expr.kind else {
            panic!("expected match");
        };
        assert!(arms[0].guard.is_some());
        assert!(arms[1].guard.is_none());
    }

    #[test]
    fn parses_extern_block() {
        let program = parse_ok(
            "extern \"C\" {\n    fn f(a: c_int, b: c_uint) -> c_long;\n    fn g();\n}\nfn main() -> int { return 0; }",
        );
        let block = program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Extern(b) => Some(b),
                _ => None,
            })
            .expect("extern block");
        assert_eq!(block.abi, "C");
        assert_eq!(block.functions.len(), 2);
        assert_eq!(block.functions[0].name, "f");
        assert_eq!(block.functions[0].symbol, "f");
        assert_eq!(block.functions[1].name, "g");
    }

    #[test]
    fn parses_unsafe_statement() {
        let program = parse_ok(
            "fn main() -> int {\n    unsafe {\n        let x = 1;\n    }\n    return 0;\n}",
        );
        let f = program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Function(f) => Some(f),
                _ => None,
            })
            .expect("main");
        assert!(matches!(f.body.stmts[0], Stmt::Unsafe(_)));
    }
}
