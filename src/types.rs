use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FfiKind {
    CInt,
    CUInt,
    CLong,
    CULong,
    CLongLong,
    CULongLong,
    F32,
    RawPtr,
}

impl FfiKind {
    /// The C type used in generated code. `long`/`unsigned long` keep their
    /// target-specific width (32-bit on Windows, 64-bit on LP64 systems).
    pub fn c_type(self) -> &'static str {
        match self {
            FfiKind::CInt => "int",
            FfiKind::CUInt => "unsigned int",
            FfiKind::CLong => "long",
            FfiKind::CULong => "unsigned long",
            FfiKind::CLongLong => "long long",
            FfiKind::CULongLong => "unsigned long long",
            FfiKind::F32 => "float",
            FfiKind::RawPtr => "void *",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FfiKind::CInt => "c_int",
            FfiKind::CUInt => "c_uint",
            FfiKind::CLong => "c_long",
            FfiKind::CULong => "c_ulong",
            FfiKind::CLongLong => "c_longlong",
            FfiKind::CULongLong => "c_ulonglong",
            FfiKind::F32 => "f32",
            FfiKind::RawPtr => "rawptr",
        }
    }

    pub fn mangle(self) -> &'static str {
        match self {
            FfiKind::CInt => "cint",
            FfiKind::CUInt => "cuint",
            FfiKind::CLong => "clong",
            FfiKind::CULong => "culong",
            FfiKind::CLongLong => "clonglong",
            FfiKind::CULongLong => "culonglong",
            FfiKind::F32 => "f32",
            FfiKind::RawPtr => "rawptr",
        }
    }

    /// Signed integer ABI types (useful for `int(x)` conversions).
    pub fn is_integer(self) -> bool {
        matches!(
            self,
            FfiKind::CInt
                | FfiKind::CUInt
                | FfiKind::CLong
                | FfiKind::CULong
                | FfiKind::CLongLong
                | FfiKind::CULongLong
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Char,
    Void,
    Array(Box<Type>),
    Slice(Box<Type>),
    Struct(String, Vec<Type>),
    Enum(String, Vec<Type>),
    Param(String),
    Ffi(FfiKind),
    /// Internal handle to a reference-counted JSON value (`std::json`). It is
    /// managed: cloning retains, dropping releases. Source code can only
    /// obtain it through the public `std::json` API (the handle lives inside
    /// the built-in `std::json::JsonValue` struct).
    Json,
    /// Internal handle to a reference-counted router (`std::web`). Managed
    /// like `Type::Json`; the handle lives inside the built-in
    /// `std::web::Router` struct.
    Web,
    /// Internal handle to a reference-counted parsed template (`std::html`).
    /// Managed like `Type::Json`; the handle lives inside the built-in
    /// `std::html::Template` struct.
    Html,
    /// Internal handle to a reference-counted byte buffer (`std::fs::Bytes`).
    /// Managed like `Type::Json`; the handle lives inside the built-in
    /// `std::fs::Bytes` struct.
    Bytes,
    /// Internal handle to reference-counted form data (`std::web::FormData`).
    /// Managed like `Type::Json`; the handle lives inside the built-in
    /// `std::web::FormData` struct.
    Form,
    /// Internal handle to parsed request cookies (`std::web::Cookies`).
    Cookies,
    /// Internal handle to a server-side session store
    /// (`std::web::SessionStore`).
    SessionStore,
    /// Internal handle to a server-side session (`std::web::Session`).
    Session,
    /// Internal handle to an open SQLite database (`std::db::Database`).
    Db,
    /// Internal handle to a prepared SQLite statement (`std::db::Statement`).
    Stmt,
    /// Internal handle to a login rate limiter (`std::auth::Limiter`).
    Limiter,
    /// Internal handle to a UI component tree (`std::ui::UiNode`).
    Ui,
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }

    pub fn is_basic(&self) -> bool {
        matches!(
            self,
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Char
        )
    }

    pub fn element(&self) -> Option<&Type> {
        match self {
            Type::Array(t) => Some(t),
            _ => None,
        }
    }

    pub fn contains_param(&self) -> bool {
        match self {
            Type::Param(_) => true,
            Type::Enum(_, args) | Type::Struct(_, args) => args.iter().any(|a| a.contains_param()),
            Type::Array(inner) | Type::Slice(inner) => inner.contains_param(),
            _ => false,
        }
    }
}

pub fn c_ident(name: &str) -> String {
    name.replace("::", "_")
}

pub fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "str".to_string(),
        Type::Char => "char".to_string(),
        Type::Void => "void".to_string(),
        Type::Struct(name, args) => {
            let mut s = c_ident(name);
            for a in args {
                s.push('_');
                s.push_str(&mangle_type(a));
            }
            s
        }
        Type::Enum(name, args) => {
            let mut s = c_ident(name);
            for a in args {
                s.push('_');
                s.push_str(&mangle_type(a));
            }
            s
        }
        Type::Array(inner) => format!("arr_{}", mangle_type(inner)),
        Type::Slice(inner) => format!("slice_{}", mangle_type(inner)),
        Type::Ffi(kind) => kind.mangle().to_string(),
        Type::Json => "json".to_string(),
        Type::Web => "web".to_string(),
        Type::Html => "html".to_string(),
        Type::Bytes => "bytes".to_string(),
        Type::Form => "form".to_string(),
        Type::Cookies => "cookies".to_string(),
        Type::SessionStore => "session_store".to_string(),
        Type::Session => "session".to_string(),
        Type::Db => "db".to_string(),
        Type::Stmt => "stmt".to_string(),
        Type::Limiter => "auth_limiter".to_string(),
        Type::Ui => "ui".to_string(),
        Type::Param(name) => c_ident(name),
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Char => write!(f, "char"),
            Type::Void => write!(f, "void"),
            Type::Array(t) => write!(f, "[{}]", t),
            Type::Slice(t) => write!(f, "Slice<{}>", t),
            Type::Ffi(kind) => write!(f, "{}", kind.name()),
            Type::Json => write!(f, "std::json::JsonValue"),
            Type::Web => write!(f, "std::web::Router"),
            Type::Html => write!(f, "std::html::Template"),
            Type::Bytes => write!(f, "std::fs::Bytes"),
            Type::Form => write!(f, "std::web::FormData"),
            Type::Cookies => write!(f, "std::web::Cookies"),
            Type::SessionStore => write!(f, "std::web::SessionStore"),
            Type::Session => write!(f, "std::web::Session"),
            Type::Db => write!(f, "std::db::Database"),
            Type::Stmt => write!(f, "std::db::Statement"),
            Type::Limiter => write!(f, "std::auth::Limiter"),
            Type::Ui => write!(f, "std::ui::UiNode"),
            Type::Struct(name, args) => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                    write!(f, "{}<{}>", name, args.join(", "))
                }
            }
            Type::Enum(name, args) => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                    write!(f, "{}<{}>", name, args.join(", "))
                }
            }
            Type::Param(name) => write!(f, "{}", name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_type_is_displayed_and_mangled() {
        assert_eq!(Type::Form.to_string(), "std::web::FormData");
        assert_eq!(mangle_type(&Type::Form), "form");
    }

    #[test]
    fn form_type_is_named_like_the_other_managed_handles() {
        assert_eq!(Type::Form.to_string().split("::").last(), Some("FormData"));
    }

    #[test]
    fn database_types_are_displayed_and_mangled() {
        assert_eq!(Type::Db.to_string(), "std::db::Database");
        assert_eq!(Type::Stmt.to_string(), "std::db::Statement");
        assert_eq!(mangle_type(&Type::Db), "db");
        assert_eq!(mangle_type(&Type::Stmt), "stmt");
    }

    #[test]
    fn limiter_type_is_displayed_and_mangled() {
        assert_eq!(Type::Limiter.to_string(), "std::auth::Limiter");
        assert_eq!(mangle_type(&Type::Limiter), "auth_limiter");
    }

    #[test]
    fn cookie_and_session_types_are_displayed_and_mangled() {
        assert_eq!(Type::Cookies.to_string(), "std::web::Cookies");
        assert_eq!(Type::SessionStore.to_string(), "std::web::SessionStore");
        assert_eq!(Type::Session.to_string(), "std::web::Session");
        assert_eq!(mangle_type(&Type::Cookies), "cookies");
        assert_eq!(mangle_type(&Type::SessionStore), "session_store");
        assert_eq!(mangle_type(&Type::Session), "session");
    }
}
