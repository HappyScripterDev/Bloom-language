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
