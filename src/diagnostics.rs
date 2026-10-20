use crate::token::Span;

pub mod codes {
    pub const UNEXPECTED_CHAR: &str = "E0001";
    pub const UNTERMINATED_STRING: &str = "E0002";
    pub const INVALID_NUMBER: &str = "E0003";
    pub const UNTERMINATED_COMMENT: &str = "E0005";
    pub const INVALID_ESCAPE: &str = "E0006";
    pub const INVALID_CHAR_LITERAL: &str = "E0007";
    pub const UNEXPECTED_TOKEN: &str = "E1001";
    pub const TYPE_MISMATCH: &str = "E1002";
    pub const UNDEFINED_VARIABLE: &str = "E1003";
    pub const UNDEFINED_FUNCTION: &str = "E1004";
    pub const ARG_COUNT: &str = "E1005";
    pub const IMMUTABLE: &str = "E1006";
    pub const MISSING_RETURN: &str = "E1007";
    pub const BREAK_OUTSIDE_LOOP: &str = "E1008";
    pub const CONDITION_NOT_BOOL: &str = "E1009";
    pub const DUPLICATE_DECL: &str = "E1010";
    pub const NOT_CALLABLE: &str = "E1011";
    pub const INVALID_OPERANDS: &str = "E1012";
    pub const INVALID_INDEX: &str = "E1013";
    pub const NOT_CONSTANT: &str = "E1014";
    pub const UNSUPPORTED: &str = "E1015";
    pub const UNKNOWN_TYPE: &str = "E1016";
    pub const INVALID_LVALUE: &str = "E1017";
    pub const INVALID_RETURN: &str = "E1018";
    pub const INVALID_ARGUMENT: &str = "E1019";
    pub const UNKNOWN_FIELD: &str = "E1020";
    pub const DUPLICATE_FIELD: &str = "E1021";
    pub const MISSING_FIELD: &str = "E1022";
    pub const NOT_A_STRUCT: &str = "E1023";
    pub const RECURSIVE_STRUCT: &str = "E1024";
    pub const DUPLICATE_METHOD: &str = "E1030";
    pub const UNKNOWN_METHOD: &str = "E1031";
    pub const MUTATING_METHOD_RECEIVER: &str = "E1032";
    pub const IMPL_UNKNOWN_TYPE: &str = "E1033";
    pub const UNKNOWN_VARIANT: &str = "E1040";
    pub const NON_EXHAUSTIVE_MATCH: &str = "E1041";
    pub const UNREACHABLE_PATTERN: &str = "E1042";
    pub const INVALID_ENUM: &str = "E1043";
    pub const INVALID_PATTERN: &str = "E1044";
    pub const CANNOT_INFER_TYPE_ARGS: &str = "E1045";
    pub const RECURSIVE_ENUM: &str = "E1047";
    pub const MODULE_NOT_FOUND: &str = "E1050";
    pub const PRIVATE_SYMBOL: &str = "E1051";
    pub const DUPLICATE_IMPORT: &str = "E1052";
    pub const MODULE_NAME_CONFLICT: &str = "E1053";
    pub const ENTRY_IMPORT: &str = "E1054";
    pub const UNKNOWN_IMPORT: &str = "E1055";
    pub const RESERVED_IDENT: &str = "E1056";
    pub const TRY_INVALID_OPERAND: &str = "E1060";
    pub const TRY_RETURN_MISMATCH: &str = "E1061";
    pub const UNSAFE_REQUIRED: &str = "E1070";
    pub const INVALID_EXTERN: &str = "E1071";
    pub const INVALID_FFI_TYPE: &str = "E1072";
    pub const MISSING_MAIN: &str = "E2001";
    pub const BAD_MAIN: &str = "E2002";
    pub const IO: &str = "E3001";
    pub const NO_C_COMPILER: &str = "E3002";
    pub const NATIVE_COMPILE: &str = "E3003";
    pub const BUILD_CONFIG: &str = "E3005";
    pub const USAGE: &str = "E4001";
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub label: Option<String>,
    pub notes: Vec<String>,
    pub help: Option<String>,
    pub file: Option<String>,
}

impl Diagnostic {
    pub fn new(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            code,
            message: message.into(),
            span,
            label: None,
            notes: Vec::new(),
            help: None,
            file: None,
        }
    }
}

fn line_slice(source: &str, line: u32) -> &str {
    source
        .lines()
        .nth((line as usize).saturating_sub(1))
        .unwrap_or("")
}

pub fn render(diag: &Diagnostic, source: &str, file: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("error[{}]: {}\n", diag.code, diag.message));
    out.push_str(&format!(
        "  --> {}:{}:{}\n",
        file, diag.span.start.line, diag.span.start.col
    ));
    let text = line_slice(source, diag.span.start.line);
    let line_no = diag.span.start.line.to_string();
    let gutter = " ".repeat(line_no.len());
    out.push_str(&format!("{} |\n", gutter));
    out.push_str(&format!("{} | {}\n", line_no, text));
    let caret_start = (diag.span.start.col as usize).saturating_sub(1);
    let mut caret_len = if diag.span.end.line == diag.span.start.line {
        (diag.span.end.col as usize).saturating_sub(diag.span.start.col as usize)
    } else {
        1
    };
    let visible: Vec<char> = text.chars().collect();
    if caret_start >= visible.len() && !visible.is_empty() {
        caret_len = 1;
    }
    if caret_len == 0 {
        caret_len = 1;
    }
    if !visible.is_empty() {
        caret_len = caret_len.min(visible.len().saturating_sub(caret_start).max(1));
    }
    let caret_line = format!("{}{}", " ".repeat(caret_start), "^".repeat(caret_len));
    let suffix = match &diag.label {
        Some(l) => format!(" {}", l),
        None => String::new(),
    };
    out.push_str(&format!("{} | {}{}\n", gutter, caret_line, suffix));
    out.push_str(&format!("{} |\n", gutter));
    for note in &diag.notes {
        out.push_str(&format!("{} = note: {}\n", gutter, note));
    }
    if let Some(help) = &diag.help {
        out.push_str(&format!("{} = help: {}\n", gutter, help));
    }
    out
}

pub fn render_all_with_sources(
    diagnostics: &[Diagnostic],
    sources: &[(String, String)],
    fallback_source: &str,
    fallback_file: &str,
) -> String {
    let mut out = String::new();
    for diag in diagnostics.iter().take(100) {
        let (diag_source, diag_file) = match &diag.file {
            Some(f) => match sources.iter().find(|(name, _)| name == f) {
                Some((name, src)) => (src.as_str(), name.as_str()),
                None => (fallback_source, f.as_str()),
            },
            None => (fallback_source, fallback_file),
        };
        out.push_str(&render(diag, diag_source, diag_file));
        out.push('\n');
    }
    if diagnostics.len() > 100 {
        out.push_str(&format!(
            "... and {} more errors (only the first 100 are shown)\n\n",
            diagnostics.len() - 100
        ));
    }
    out.push_str(&format!(
        "error: aborting due to {} previous error{}\n",
        diagnostics.len(),
        if diagnostics.len() == 1 { "" } else { "s" }
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::{Pos, Span};

    fn span(line: u32, col: u32) -> Span {
        Span::new(Pos::new(0, line, col), Pos::new(0, line, col + 1))
    }

    #[test]
    fn renders_expected_shape() {
        let source = "let age: int = \"hello\";\n";
        let mut d = Diagnostic::new(codes::TYPE_MISMATCH, "incompatible types", span(1, 13));
        d.label = Some("expected `int`, found `string`".to_string());
        d.help = Some("assign an integer or change the declared type".to_string());
        let text = render(&d, source, "main.blm");
        assert!(text.contains("error[E1002]: incompatible types"));
        assert!(text.contains("--> main.blm:1:13"));
        assert!(text.contains("let age: int = \"hello\";"));
        assert!(text.contains("= help: assign an integer or change the declared type"));
    }
}
