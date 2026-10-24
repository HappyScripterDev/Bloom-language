use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diagnostics::{codes, Diagnostic};
use crate::matchcheck::{self, MatchCtx, NormPat};
use crate::modules::{ImportedItem, LoadedProgram};
use crate::token::Span;
use crate::types::Type;

#[derive(Debug, Clone)]
pub struct FnSig {
    pub c_name: String,
    pub type_params: Vec<String>,
    pub params: Vec<Type>,
    pub ret: Type,
    pub body: Block,
    pub span: Span,
    pub module: String,
    pub is_pub: bool,
    /// `extern` declaration: no body, direct symbol name, unsafe to call.
    pub is_extern: bool,
    /// Calling convention: `false` = C, `true` = system (Windows `__stdcall`).
    pub system_abi: bool,
}

#[derive(Debug, Clone)]
pub struct VarInfo {
    pub c_name: String,
    pub ty: Type,
    pub mutable: bool,
    pub kind: DeclKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
    Print,
    Println,
    Input,
    Len,
    Str,
    Int,
    Float,
    Char,
    Exit,
    Panic,
    Push,
    Pop,
    Insert,
    Remove,
    Slice,
    SliceView,
    Join,
    Contains,
    StartsWith,
    EndsWith,
    Trim,
    Replace,
    ToUpper,
    ToLower,
    Split,
    CIntConv,
    CUIntConv,
    CLongConv,
    CULongConv,
    CLongLongConv,
    CULongLongConv,
    F32Conv,
    RawPtrNull,
}

#[derive(Debug, Clone)]
pub enum Callee {
    Builtin(BuiltinKind),
    User(FunctionCallInfo),
    Method(MethodCallInfo),
}

#[derive(Debug, Clone)]
pub struct FunctionCallInfo {
    pub index: usize,
    pub type_args: Vec<Type>,
    pub base_c_name: String,
    pub params: Vec<Type>,
}

#[derive(Debug, Clone)]
pub struct MethodCallInfo {
    pub owner: String,
    pub method: String,
    pub base_c_name: String,
    pub receiver: Option<ReceiverKind>,
    pub params: Vec<Type>,
    pub type_args: Vec<Type>,
}

#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub name: String,
    pub owner: String,
    pub c_name: String,
    pub receiver: Option<ReceiverKind>,
    pub type_params: Vec<String>,
    pub params: Vec<Type>,
    pub ret: Type,
    pub body: Block,
    pub is_pub: bool,
    pub module: String,
    pub is_enum: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Char(char),
}

impl ConstValue {
    pub fn ty(&self) -> Type {
        match self {
            ConstValue::Int(_) => Type::Int,
            ConstValue::Float(_) => Type::Float,
            ConstValue::Bool(_) => Type::Bool,
            ConstValue::Str(_) => Type::String,
            ConstValue::Char(_) => Type::Char,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GlobalInfo {
    pub c_name: String,
    pub ty: Type,
    pub value: ConstValue,
    pub module: String,
    pub is_pub: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleUnit {
    pub path_display: String,
    pub program: Program,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub params: Vec<String>,
    pub fields: Vec<StructField>,
    pub span: Span,
    pub module: String,
    pub is_pub: bool,
}

#[derive(Debug, Clone)]
pub struct EnumVariantInfo {
    pub name: String,
    pub fields: Vec<Type>,
    pub field_names: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub params: Vec<String>,
    pub variants: Vec<EnumVariantInfo>,
    pub span: Span,
    pub is_pub: bool,
    pub module: String,
}

impl EnumInfo {
    pub fn variant(&self, name: &str) -> Option<(usize, &EnumVariantInfo)> {
        self.variants
            .iter()
            .enumerate()
            .find(|(_, v)| v.name == name)
    }

    pub fn variant_names(&self) -> String {
        self.variants
            .iter()
            .map(|v| v.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn substitute_type(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Param(name) => bindings.get(name).cloned().unwrap_or(Type::Int),
        Type::Array(inner) => Type::Array(Box::new(substitute_type(inner, bindings))),
        Type::Slice(inner) => Type::Slice(Box::new(substitute_type(inner, bindings))),
        Type::Enum(name, args) => Type::Enum(
            name.clone(),
            args.iter().map(|a| substitute_type(a, bindings)).collect(),
        ),
        Type::Struct(name, args) => Type::Struct(
            name.clone(),
            args.iter().map(|a| substitute_type(a, bindings)).collect(),
        ),
        other => other.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct FunctionInstance {
    pub index: usize,
    pub type_args: Vec<Type>,
    pub c_name: String,
}

#[derive(Debug, Clone)]
pub struct MethodInstance {
    pub owner: String,
    pub method: String,
    pub type_args: Vec<Type>,
    pub c_name: String,
}

#[derive(Debug, Clone)]
pub struct VariantRef {
    pub enum_name: String,
    pub enum_args: Vec<Type>,
    pub tag: usize,
    pub variant_name: String,
    pub field_names: Option<Vec<String>>,
    pub field_types: Vec<Type>,
}

#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub c_name: String,
    pub ty: Type,
    pub name: String,
    pub decl_id: DeclId,
}

impl StructInfo {
    pub fn field(&self, name: &str) -> Option<&StructField> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn field_names(&self) -> String {
        self.fields
            .iter()
            .map(|f| format!("{}: {}", f.name, f.ty))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub struct CheckedProgram {
    pub modules: Vec<ModuleUnit>,
    pub entry_index: usize,
    pub sources: Vec<(String, String)>,
    pub expr_types: HashMap<ExprId, Type>,
    pub var_decls: HashMap<DeclId, VarInfo>,
    pub ident_refs: HashMap<ExprId, DeclId>,
    pub callee_refs: HashMap<ExprId, Callee>,
    pub functions: Vec<FnSig>,
    pub globals: Vec<GlobalInfo>,
    pub structs: HashMap<String, StructInfo>,
    pub methods: HashMap<String, HashMap<String, MethodInfo>>,
    pub enums: HashMap<String, EnumInfo>,
    pub instantiations: Vec<Type>,
    pub fn_instantiations: Vec<FunctionInstance>,
    pub method_instantiations: Vec<MethodInstance>,
    pub variant_refs: HashMap<ExprId, VariantRef>,
    pub global_refs: HashMap<ExprId, usize>,
    pub match_arm_bindings: HashMap<(ExprId, usize), Vec<BindingInfo>>,
    pub main_index: usize,
}

/// FFI-compatible types: the ABI scalars and the opaque pointer.
fn is_ffi_type(ty: &Type) -> bool {
    matches!(ty, Type::Ffi(_))
}

fn builtin_kind(name: &str) -> Option<BuiltinKind> {
    Some(match name {
        "print" => BuiltinKind::Print,
        "println" => BuiltinKind::Println,
        "input" => BuiltinKind::Input,
        "len" => BuiltinKind::Len,
        "str" => BuiltinKind::Str,
        "int" => BuiltinKind::Int,
        "float" => BuiltinKind::Float,
        "char" => BuiltinKind::Char,
        "exit" => BuiltinKind::Exit,
        "panic" => BuiltinKind::Panic,
        "push" => BuiltinKind::Push,
        "pop" => BuiltinKind::Pop,
        "insert" => BuiltinKind::Insert,
        "remove" => BuiltinKind::Remove,
        "slice" => BuiltinKind::Slice,
        "slice_view" => BuiltinKind::SliceView,
        "join" => BuiltinKind::Join,
        "contains" => BuiltinKind::Contains,
        "starts_with" => BuiltinKind::StartsWith,
        "ends_with" => BuiltinKind::EndsWith,
        "trim" => BuiltinKind::Trim,
        "replace" => BuiltinKind::Replace,
        "to_upper" => BuiltinKind::ToUpper,
        "to_lower" => BuiltinKind::ToLower,
        "split" => BuiltinKind::Split,
        "c_int" => BuiltinKind::CIntConv,
        "c_uint" => BuiltinKind::CUIntConv,
        "c_long" => BuiltinKind::CLongConv,
        "c_ulong" => BuiltinKind::CULongConv,
        "c_longlong" => BuiltinKind::CLongLongConv,
        "c_ulonglong" => BuiltinKind::CULongLongConv,
        "f32" => BuiltinKind::F32Conv,
        "rawptr_null" => BuiltinKind::RawPtrNull,
        _ => return None,
    })
}

#[allow(dead_code)]
pub fn check(program: Program) -> Result<CheckedProgram, Vec<Diagnostic>> {
    let loaded = LoadedProgram {
        modules: vec![crate::modules::LoadedModule {
            name: String::new(),
            path: std::path::PathBuf::from("input.blm"),
            source: String::new(),
            program,
            imports: Vec::new(),
            builtin: None,
        }],
        entry_index: 0,
    };
    check_modules(loaded, &[])
}

#[derive(Debug, Clone)]
enum PathResolution {
    Function(String),
    Struct(String),
    Enum(String),
    EnumVariant(String, String),
    AssociatedMethod(String, String),
    Global(String),
    Unknown,
}

fn short_of(canonical: &str) -> String {
    match canonical.rfind("::") {
        Some(pos) => canonical[pos + 2..].to_string(),
        None => canonical.to_string(),
    }
}

fn specialized_c_name(base: &str, args: &[Type]) -> String {
    if args.is_empty() {
        base.to_string()
    } else {
        let suffix: Vec<String> = args.iter().map(crate::types::mangle_type).collect();
        format!("{}_{}", base, suffix.join("_"))
    }
}

fn describe_type_expr(te: &TypeExpr) -> String {
    match &te.kind {
        TypeExprKind::Named(name) => name.clone(),
        TypeExprKind::Generic(name, args) => {
            let inner: Vec<String> = args.iter().map(describe_type_expr).collect();
            format!("{}<{}>", name, inner.join(", "))
        }
        TypeExprKind::Array(inner) => format!("[{}]", describe_type_expr(inner)),
    }
}

fn display_module(module: &str) -> String {
    if module.is_empty() {
        "the entry module".to_string()
    } else {
        module.to_string()
    }
}

pub fn check_modules(
    mut loaded: LoadedProgram,
    sources: &[(String, String)],
) -> Result<CheckedProgram, Vec<Diagnostic>> {
    let module_names: Vec<String> = loaded.modules.iter().map(|m| m.name.clone()).collect();
    let module_paths: Vec<String> = crate::modules::module_display_paths(&loaded.modules);
    let mut ck = Checker {
        expr_types: HashMap::new(),
        var_decls: HashMap::new(),
        ident_refs: HashMap::new(),
        callee_refs: HashMap::new(),
        functions: Vec::new(),
        fn_index: HashMap::new(),
        globals: Vec::new(),
        global_index: HashMap::new(),
        structs: HashMap::new(),
        methods: HashMap::new(),
        enums: builtin_enums(),
        enum_params: None,
        variant_refs: HashMap::new(),
        global_refs: HashMap::new(),
        match_arm_bindings: HashMap::new(),
        pending_variant_field_types: Vec::new(),
        scopes: Vec::new(),
        consts: HashMap::new(),
        error_exprs: HashSet::new(),
        diags: Vec::new(),
        c_counter: 0,
        current_fn: None,
        current_ret: None,
        loop_depth: 0,
        unsafe_depth: 0,
        loaded_modules: module_names.iter().cloned().collect(),
        current_module: String::new(),
        is_entry: true,
        imports: HashMap::new(),
        all_imports: Vec::new(),
        module_paths: module_paths.clone(),
        module_exports: HashMap::new(),
    };
    for m in &loaded.modules {
        ck.all_imports.push(import_map(&m.name, &m.imports));
    }
    macro_rules! per_module {
        ($f:ident) => {
            for i in 0..loaded.modules.len() {
                ck.set_module_context(i, &loaded.modules);
                let before = ck.diags.len();
                let program = &mut loaded.modules[i].program;
                ck.$f(program);
                let file = ck.module_paths[i].clone();
                for d in &mut ck.diags[before..] {
                    if d.file.is_none() {
                        d.file = Some(file.clone());
                    }
                }
            }
        };
    }
    per_module!(register_enum_names);
    per_module!(register_struct_names);
    per_module!(resolve_enum_variants);
    per_module!(resolve_struct_fields);
    ck.check_struct_cycles();
    ck.register_builtin_modules(&loaded);
    per_module!(collect_functions);
    per_module!(collect_extern_functions);
    per_module!(collect_methods);
    ck.build_module_exports(&loaded);
    for (i, m) in loaded.modules.iter().enumerate() {
        let resolved = ck.resolved_import_map(&m.name, &m.imports);
        ck.all_imports[i] = resolved;
    }
    ck.validate_imports(&loaded);
    for (i, m) in loaded.modules.iter().enumerate() {
        ck.set_module_context(i, &loaded.modules);
        let before = ck.diags.len();
        ck.scopes.push(HashMap::new());
        for item in &m.program.items {
            if let Item::Global(decl) = item {
                ck.check_global(decl);
            }
        }
        for item in &m.program.items {
            if let Item::Function(f) = item {
                ck.check_function(f);
            }
        }
        for item in &m.program.items {
            if let Item::Impl(b) = item {
                for meth in &b.methods {
                    ck.check_method(b, meth);
                }
            }
        }
        ck.scopes.pop();
        let file = ck.module_paths[i].clone();
        for d in &mut ck.diags[before..] {
            if d.file.is_none() {
                d.file = Some(file.clone());
            }
        }
    }
    ck.set_module_context(loaded.entry_index, &loaded.modules);
    let main_index = ck.check_main();
    let (instantiations, fn_instantiations, method_instantiations) = ck.collect_instantiations();
    if ck.diags.is_empty() {
        let main_index = main_index.unwrap_or(0);
        let modules = loaded
            .modules
            .iter()
            .zip(ck.module_paths.iter())
            .map(|(m, path)| ModuleUnit {
                path_display: path.clone(),
                program: m.program.clone(),
            })
            .collect();
        Ok(CheckedProgram {
            modules,
            entry_index: loaded.entry_index,
            sources: sources.to_vec(),
            expr_types: ck.expr_types,
            var_decls: ck.var_decls,
            ident_refs: ck.ident_refs,
            callee_refs: ck.callee_refs,
            functions: ck.functions,
            globals: ck.globals,
            structs: ck.structs,
            methods: ck.methods,
            enums: ck.enums,
            instantiations,
            fn_instantiations,
            method_instantiations,
            variant_refs: ck.variant_refs,
            global_refs: ck.global_refs,
            match_arm_bindings: ck.match_arm_bindings,
            main_index,
        })
    } else {
        Err(ck.diags)
    }
}

fn import_map(
    module_name: &str,
    edges: &[crate::modules::ImportEdge],
) -> HashMap<String, ImportedItem> {
    let mut map = HashMap::new();
    for edge in edges {
        match &edge.target {
            ImportedItem::Module(canonical) => {
                map.insert(
                    edge.local_name.clone(),
                    ImportedItem::Module(canonical.clone()),
                );
            }
            ImportedItem::Symbol(canonical) => {
                map.insert(
                    edge.local_name.clone(),
                    ImportedItem::Symbol(canonical.clone()),
                );
            }
        }
    }
    let _ = module_name;
    map
}

struct Checker {
    expr_types: HashMap<ExprId, Type>,
    var_decls: HashMap<DeclId, VarInfo>,
    ident_refs: HashMap<ExprId, DeclId>,
    callee_refs: HashMap<ExprId, Callee>,
    functions: Vec<FnSig>,
    fn_index: HashMap<String, usize>,
    globals: Vec<GlobalInfo>,
    global_index: HashMap<String, usize>,
    structs: HashMap<String, StructInfo>,
    methods: HashMap<String, HashMap<String, MethodInfo>>,
    enums: HashMap<String, EnumInfo>,
    enum_params: Option<HashSet<String>>,
    variant_refs: HashMap<ExprId, VariantRef>,
    global_refs: HashMap<ExprId, usize>,
    match_arm_bindings: HashMap<(ExprId, usize), Vec<BindingInfo>>,
    pending_variant_field_types: Vec<(usize, Type, Span)>,
    scopes: Vec<HashMap<String, DeclId>>,
    consts: HashMap<DeclId, ConstValue>,
    error_exprs: HashSet<ExprId>,
    diags: Vec<Diagnostic>,
    c_counter: u32,
    current_fn: Option<usize>,
    current_ret: Option<Type>,
    loop_depth: u32,
    unsafe_depth: u32,
    loaded_modules: HashSet<String>,
    current_module: String,
    is_entry: bool,
    imports: HashMap<String, ImportedItem>,
    all_imports: Vec<HashMap<String, ImportedItem>>,
    module_paths: Vec<String>,
    module_exports: HashMap<String, HashMap<String, String>>,
}

fn builtin_enums() -> HashMap<String, EnumInfo> {
    let mut enums = HashMap::new();
    let span = Span::point(1, 1);
    enums.insert(
        "Option".to_string(),
        EnumInfo {
            params: vec!["T".to_string()],
            variants: vec![
                EnumVariantInfo {
                    name: "Some".to_string(),
                    fields: vec![Type::Param("T".to_string())],
                    field_names: None,
                },
                EnumVariantInfo {
                    name: "None".to_string(),
                    fields: Vec::new(),
                    field_names: None,
                },
            ],
            span,
            is_pub: true,
            module: String::new(),
        },
    );
    enums.insert(
        "Result".to_string(),
        EnumInfo {
            params: vec!["T".to_string(), "E".to_string()],
            variants: vec![
                EnumVariantInfo {
                    name: "Ok".to_string(),
                    fields: vec![Type::Param("T".to_string())],
                    field_names: None,
                },
                EnumVariantInfo {
                    name: "Err".to_string(),
                    fields: vec![Type::Param("E".to_string())],
                    field_names: None,
                },
            ],
            span,
            is_pub: true,
            module: String::new(),
        },
    );
    enums
}

impl Checker {
    fn error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.diags.push(Diagnostic::new(code, message, span));
    }

    fn set_module_context(&mut self, index: usize, modules: &[crate::modules::LoadedModule]) {
        self.current_module = modules[index].name.clone();
        self.is_entry = index == 0;
        self.imports = self.all_imports[index].clone();
    }

    fn canonical_name(&self, name: &str) -> String {
        if self.is_entry {
            name.to_string()
        } else {
            format!("{}::{}", self.current_module, name)
        }
    }

    fn c_symbol(&self, name: &str) -> String {
        if self.is_entry {
            name.to_string()
        } else {
            format!("{}_{}", self.current_module.replace("::", "_"), name)
        }
    }

    fn module_of_canonical(canonical: &str) -> String {
        match canonical.rfind("::") {
            Some(pos) => canonical[..pos].to_string(),
            None => String::new(),
        }
    }

    fn short_of_canonical(canonical: &str) -> String {
        match canonical.rfind("::") {
            Some(pos) => canonical[pos + 2..].to_string(),
            None => canonical.to_string(),
        }
    }

    fn resolve_module_path(&self, segments: &[String]) -> Option<String> {
        let first = segments.first()?;
        let base = match self.imports.get(first)? {
            ImportedItem::Module(c) => c.clone(),
            ImportedItem::Symbol(_) => return None,
        };
        let mut canonical = base;
        for s in &segments[1..] {
            canonical = format!("{}::{}", canonical, s);
        }
        if self.loaded_modules.contains(&canonical) {
            Some(canonical)
        } else {
            None
        }
    }

    fn resolve_path(&self, segments: &[String]) -> PathResolution {
        if segments.is_empty() {
            return PathResolution::Unknown;
        }
        if segments.len() == 1 {
            let name = &segments[0];
            let local = self.canonical_name(name);
            if self.fn_index.contains_key(&local) {
                return PathResolution::Function(local);
            }
            if self.structs.contains_key(&local) {
                return PathResolution::Struct(local);
            }
            if self.enums.contains_key(&local) {
                return PathResolution::Enum(local);
            }
            if self.global_index.contains_key(&local) {
                return PathResolution::Global(local);
            }
            if let Some(info) = self.enums.get(name) {
                if info.module.is_empty() {
                    return PathResolution::Enum(name.clone());
                }
            }
            if let Some(ImportedItem::Symbol(canonical)) = self.imports.get(name) {
                if self.fn_index.contains_key(canonical) {
                    return PathResolution::Function(canonical.clone());
                }
                if self.structs.contains_key(canonical) {
                    return PathResolution::Struct(canonical.clone());
                }
                if self.enums.contains_key(canonical) {
                    return PathResolution::Enum(canonical.clone());
                }
                if self.global_index.contains_key(canonical) {
                    return PathResolution::Global(canonical.clone());
                }
            }
            return PathResolution::Unknown;
        }
        if segments.len() == 2 {
            if let Some(type_canonical) = self.resolve_named_type(&segments[..1]) {
                if self.assoc_method_of(&type_canonical, &segments[1]) {
                    return PathResolution::AssociatedMethod(type_canonical, segments[1].clone());
                }
            }
            if let Some(enum_canonical) = self.resolve_enum_name(&segments[..1]) {
                return PathResolution::EnumVariant(enum_canonical, segments[1].clone());
            }
        }
        for k in (1..segments.len()).rev() {
            if let Some(module_canonical) = self.resolve_module_path(&segments[..k]) {
                let rest = &segments[k..];
                if rest.len() == 1 {
                    let canonical = format!("{}::{}", module_canonical, rest[0]);
                    if self.fn_index.contains_key(&canonical) {
                        return PathResolution::Function(canonical);
                    }
                    if self.structs.contains_key(&canonical) {
                        return PathResolution::Struct(canonical);
                    }
                    if self.enums.contains_key(&canonical) {
                        return PathResolution::Enum(canonical);
                    }
                    if self.global_index.contains_key(&canonical) {
                        return PathResolution::Global(canonical);
                    }
                    if let Some(resolved) = self
                        .module_exports
                        .get(&module_canonical)
                        .and_then(|m| m.get(&rest[0]))
                        .cloned()
                    {
                        if self.fn_index.contains_key(&resolved) {
                            return PathResolution::Function(resolved);
                        }
                        if self.structs.contains_key(&resolved) {
                            return PathResolution::Struct(resolved);
                        }
                        if self.enums.contains_key(&resolved) {
                            return PathResolution::Enum(resolved);
                        }
                        if self.global_index.contains_key(&resolved) {
                            return PathResolution::Global(resolved);
                        }
                    }
                } else if rest.len() == 2 {
                    let type_canonical = format!("{}::{}", module_canonical, rest[0]);
                    if self.enums.contains_key(&type_canonical) {
                        return PathResolution::EnumVariant(type_canonical, rest[1].clone());
                    }
                    if (self.structs.contains_key(&type_canonical)
                        || self.enums.contains_key(&type_canonical))
                        && self.assoc_method_of(&type_canonical, &rest[1])
                    {
                        return PathResolution::AssociatedMethod(type_canonical, rest[1].clone());
                    }
                }
            }
        }
        PathResolution::Unknown
    }

    fn resolve_named_type(&self, segments: &[String]) -> Option<String> {
        if segments.len() != 1 {
            return None;
        }
        let local = self.canonical_name(&segments[0]);
        if self.structs.contains_key(&local) || self.enums.contains_key(&local) {
            return Some(local);
        }
        if let Some(ImportedItem::Symbol(canonical)) = self.imports.get(&segments[0]) {
            if self.structs.contains_key(canonical) || self.enums.contains_key(canonical) {
                return Some(canonical.clone());
            }
        }
        None
    }

    fn assoc_method_of(&self, type_canonical: &str, method: &str) -> bool {
        self.methods
            .get(type_canonical)
            .and_then(|ms| ms.get(method))
            .map(|info| info.receiver.is_none())
            .unwrap_or(false)
    }

    fn check_symbol_visibility(
        &mut self,
        canonical: &str,
        symbol_module: &str,
        kind: &str,
        is_pub: bool,
        span: Span,
    ) -> bool {
        if symbol_module != self.current_module && !is_pub {
            let mut diag = Diagnostic::new(
                codes::PRIVATE_SYMBOL,
                format!(
                    "{} `{}` is private to module `{}`",
                    kind,
                    short_of(canonical),
                    display_module(symbol_module)
                ),
                span,
            );
            diag.help = Some(format!(
                "mark it as `pub` in `{}` to make it available to other modules",
                display_module(symbol_module)
            ));
            self.diags.push(diag);
            return false;
        }
        true
    }

    fn symbol_exists(&self, canonical: &str) -> bool {
        self.fn_index.contains_key(canonical)
            || self.structs.contains_key(canonical)
            || self.enums.contains_key(canonical)
            || self.global_index.contains_key(canonical)
    }

    fn resolve_export_target(&self, canonical: &str) -> Option<String> {
        if self.symbol_exists(canonical) {
            return Some(canonical.to_string());
        }
        let (module, name) = canonical.rsplit_once("::")?;
        self.module_exports
            .get(module)
            .and_then(|m| m.get(name))
            .cloned()
    }

    fn resolved_import_map(
        &self,
        module: &str,
        edges: &[crate::modules::ImportEdge],
    ) -> HashMap<String, ImportedItem> {
        let _ = module;
        let mut map = HashMap::new();
        for edge in edges {
            let target = match &edge.target {
                ImportedItem::Module(c) => ImportedItem::Module(c.clone()),
                ImportedItem::Symbol(c) => {
                    let resolved = self.resolve_export_target(c).unwrap_or_else(|| c.clone());
                    ImportedItem::Symbol(resolved)
                }
            };
            map.insert(edge.local_name.clone(), target);
        }
        map
    }

    /// Builds the per-module export maps (public declarations plus `pub use`
    /// re-exports, resolved transitively to the original canonical symbol).
    fn build_module_exports(&mut self, loaded: &LoadedProgram) {
        let canonical_in = |is_entry: bool, module: &str, name: &str| -> String {
            if is_entry {
                name.to_string()
            } else {
                format!("{}::{}", module, name)
            }
        };
        let mut exports: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut pending: Vec<(String, String, String)> = Vec::new();
        for (i, m) in loaded.modules.iter().enumerate() {
            let is_entry = i == loaded.entry_index;
            let mut map: HashMap<String, String> = HashMap::new();
            for item in &m.program.items {
                match item {
                    Item::Function(f) if f.is_pub => {
                        map.insert(f.name.clone(), canonical_in(is_entry, &m.name, &f.name));
                    }
                    Item::Struct(s) if s.is_pub => {
                        map.insert(s.name.clone(), canonical_in(is_entry, &m.name, &s.name));
                    }
                    Item::Enum(e) if e.is_pub => {
                        map.insert(e.name.clone(), canonical_in(is_entry, &m.name, &e.name));
                    }
                    Item::Global(g) => {
                        map.insert(g.name.clone(), canonical_in(is_entry, &m.name, &g.name));
                    }
                    Item::Use(u) if u.is_pub => {
                        let local = u.path.last().cloned().unwrap_or_default();
                        if let Some(edge) = m.imports.iter().find(|e| e.local_name == local) {
                            if let ImportedItem::Symbol(c) = &edge.target {
                                pending.push((m.name.clone(), local, c.clone()));
                            }
                        }
                    }
                    _ => {}
                }
            }
            exports.insert(m.name.clone(), map);
        }
        let mut changed = true;
        while changed {
            changed = false;
            for (module, local, target) in &pending {
                let resolved = if self.symbol_exists(target) {
                    Some(target.clone())
                } else {
                    match target.rsplit_once("::") {
                        Some((tm, tn)) => exports.get(tm).and_then(|m| m.get(tn)).cloned(),
                        None => None,
                    }
                };
                if let Some(r) = resolved {
                    let entry = exports.get_mut(module).expect("module exports");
                    if entry.get(local) != Some(&r) {
                        entry.insert(local.clone(), r);
                        changed = true;
                    }
                }
            }
        }
        self.module_exports = exports;
    }

    fn validate_imports(&mut self, loaded: &LoadedProgram) {
        for (i, m) in loaded.modules.iter().enumerate() {
            self.set_module_context(i, &loaded.modules);
            let mut seen: HashMap<String, Span> = HashMap::new();
            let display = self.module_paths[i].clone();
            for edge in &m.imports {
                let before = self.diags.len();
                if let Some(prev) = seen.get(&edge.local_name) {
                    let mut diag = Diagnostic::new(
                        codes::DUPLICATE_IMPORT,
                        format!("`{}` is imported more than once", edge.local_name),
                        edge.path_span,
                    );
                    diag.notes
                        .push(format!("the first import is at line {}", prev.start.line));
                    self.diags.push(diag);
                }
                seen.insert(edge.local_name.clone(), edge.path_span);
                match &edge.target {
                    ImportedItem::Module(_) => {}
                    ImportedItem::Symbol(raw) => {
                        let canonical = self
                            .resolve_export_target(raw)
                            .unwrap_or_else(|| raw.clone());
                        let canonical = &canonical;
                        let found = self
                            .fn_index
                            .get(canonical)
                            .copied()
                            .map(|idx| {
                                let sig = &self.functions[idx];
                                (sig.module.clone(), sig.is_pub, "function")
                            })
                            .or_else(|| {
                                self.structs
                                    .get(canonical)
                                    .map(|s| (s.module.clone(), s.is_pub, "struct"))
                            })
                            .or_else(|| {
                                self.enums
                                    .get(canonical)
                                    .map(|e| (e.module.clone(), e.is_pub, "enum"))
                            })
                            .or_else(|| {
                                self.global_index.get(canonical).map(|&gi| {
                                    let g = &self.globals[gi];
                                    (g.module.clone(), g.is_pub, "constant")
                                })
                            });
                        match found {
                            Some((module, is_pub, kind)) => {
                                self.check_symbol_visibility(
                                    canonical,
                                    &module,
                                    kind,
                                    is_pub,
                                    edge.path_span,
                                );
                            }
                            None => {
                                let module_display = Self::module_of_canonical(canonical);
                                let mut diag = Diagnostic::new(
                                    codes::UNKNOWN_IMPORT,
                                    format!(
                                        "module `{}` has no public symbol `{}`",
                                        display_module(&module_display),
                                        Self::short_of_canonical(canonical)
                                    ),
                                    edge.path_span,
                                );
                                diag.help = Some(format!(
                                    "check the name or declare it in `{}`",
                                    display_module(&module_display)
                                ));
                                self.diags.push(diag);
                            }
                        }
                    }
                }
                for d in &mut self.diags[before..] {
                    if d.file.is_none() {
                        d.file = Some(display.clone());
                    }
                }
            }
        }
    }

    fn fresh_c_name(&mut self, name: &str) -> String {
        let n = self.c_counter;
        self.c_counter += 1;
        format!("v_{}_{}", name, n)
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, decl_id: DeclId, span: Span) {
        let scope = self.scopes.last_mut().expect("scope stack");
        if let Some(prev) = scope.get(name) {
            let prev = *prev;
            let kind = self.decl_kind_word(prev);
            self.error(
                codes::DUPLICATE_DECL,
                format!("`{}` is already declared in this scope", name),
                span,
            );
            if let Some(d) = self.diags.last_mut() {
                d.help = Some(format!(
                    "choose another name or rename the previous {} declaration",
                    kind
                ));
            }
            return;
        }
        scope.insert(name.to_string(), decl_id);
    }

    fn decl_kind_word(&self, id: DeclId) -> &'static str {
        match self.var_decls.get(&id).map(|v| v.kind) {
            Some(DeclKind::Let) => "let",
            Some(DeclKind::Var) => "var",
            Some(DeclKind::Const) => "const",
            None => "variable",
        }
    }

    fn lookup(&self, name: &str) -> Option<DeclId> {
        for scope in self.scopes.iter().rev() {
            if let Some(id) = scope.get(name) {
                return Some(*id);
            }
        }
        None
    }

    fn coerce(
        &mut self,
        from: &Type,
        to: &Type,
        span: Span,
        context: &str,
        code: &'static str,
    ) -> bool {
        if from == to {
            return true;
        }
        if *from == Type::Int && *to == Type::Float {
            return true;
        }
        let mut diag = Diagnostic::new(
            code,
            format!("incompatible types: expected `{}`, found `{}`", to, from),
            span,
        );
        diag.label = Some(format!("expected `{}`, found `{}`", to, from));
        diag.help = Some(match context {
            "assignment" => format!(
                "assign a `{}` value or convert explicitly (e.g. with `float(...)`)",
                to
            ),
            "return" => format!("the function declares return type `{}`", to),
            "argument" => format!("pass a `{}` value for this parameter", to),
            "initializer" => format!("the declared type is `{}`", to),
            "field" => format!("the field has type `{}`", to),
            _ => format!("expected `{}`", to),
        });
        self.diags.push(diag);
        false
    }

    fn require_value(&mut self, ty: &Type, span: Span, what: &str) {
        if *ty == Type::Void {
            self.error(
                codes::TYPE_MISMATCH,
                format!("{} has no value (the function returns `void`)", what),
                span,
            );
        }
    }

    fn register_builtin_modules(&mut self, loaded: &LoadedProgram) {
        let mut http_loaded = false;
        let mut tls_loaded = false;
        for m in &loaded.modules {
            match m.builtin {
                Some(crate::modules::BuiltinModule::Math) => {
                    self.register_math_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Fs) => {
                    self.register_fs_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Process) => {
                    self.register_process_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Time) => {
                    self.register_time_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Net) => {
                    self.register_net_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Tls) => {
                    self.register_tls_functions(&m.name);
                    tls_loaded = true;
                }
                Some(crate::modules::BuiltinModule::Http) => {
                    self.register_http_functions(&m.name);
                    http_loaded = true;
                }
                Some(crate::modules::BuiltinModule::Path) => {
                    self.register_path_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Json) => {
                    self.register_json_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Web) => {
                    self.register_web_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Html) => {
                    self.register_html_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Db) => {
                    self.register_db_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Auth) => {
                    self.register_auth_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Ui) => {
                    self.register_ui_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Desktop) => {
                    self.register_desktop_functions(&m.name);
                }
                Some(crate::modules::BuiltinModule::Win32) => {
                    if cfg!(windows) {
                        self.register_win32_functions(&m.name);
                    } else {
                        let mut diag = Diagnostic::new(
                            codes::MODULE_NOT_FOUND,
                            "`std::win32` is only available on Windows",
                            Span::point(1, 1),
                        );
                        diag.help = Some(
                            "Windows API wrappers cannot be provided on this platform".to_string(),
                        );
                        self.diags.push(diag);
                    }
                }
                None => {}
            }
        }
        if http_loaded {
            // The HTTP runtime uses the net and TLS helpers; register their
            // signatures so the Result typedefs they reference are emitted.
            self.register_net_functions("std::net");
            self.register_tls_functions("std::tls");
        } else if tls_loaded {
            self.register_net_functions("std::net");
        }
    }

    fn register_builtin_fn(
        &mut self,
        module: &str,
        name: &str,
        c_name: &str,
        params: Vec<Type>,
        ret: Type,
    ) {
        let canonical = format!("{}::{}", module, name);
        if self.fn_index.contains_key(&canonical) {
            return;
        }
        let span = Span::point(1, 1);
        let idx = self.functions.len();
        self.fn_index.insert(canonical, idx);
        self.functions.push(FnSig {
            c_name: c_name.to_string(),
            type_params: Vec::new(),
            params,
            ret,
            body: Block {
                stmts: Vec::new(),
                span,
            },
            span,
            module: module.to_string(),
            is_pub: true,
            is_extern: false,
            system_abi: false,
        });
    }

    fn register_fs_functions(&mut self, module: &str) {
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "read_text",
                "bloom_fs_read_text",
                vec![Type::String],
                str_err(Type::String),
            ),
            (
                "write_text",
                "bloom_fs_write_text",
                vec![Type::String, Type::String],
                str_err(Type::Int),
            ),
            (
                "append_text",
                "bloom_fs_append_text",
                vec![Type::String, Type::String],
                str_err(Type::Int),
            ),
            (
                "exists",
                "bloom_fs_exists",
                vec![Type::String],
                str_err(Type::Bool),
            ),
            (
                "is_file",
                "bloom_fs_is_file",
                vec![Type::String],
                str_err(Type::Bool),
            ),
            (
                "is_dir",
                "bloom_fs_is_dir",
                vec![Type::String],
                str_err(Type::Bool),
            ),
            (
                "file_size",
                "bloom_fs_file_size",
                vec![Type::String],
                str_err(Type::Int),
            ),
            (
                "create_dir_all",
                "bloom_fs_create_dir_all",
                vec![Type::String],
                str_err(Type::Bool),
            ),
            (
                "remove_file",
                "bloom_fs_remove_file",
                vec![Type::String],
                str_err(Type::Bool),
            ),
            (
                "remove_dir",
                "bloom_fs_remove_dir",
                vec![Type::String],
                str_err(Type::Bool),
            ),
            (
                "copy_file",
                "bloom_fs_copy_file",
                vec![Type::String, Type::String],
                str_err(Type::Int),
            ),
            (
                "rename",
                "bloom_fs_rename",
                vec![Type::String, Type::String],
                str_err(Type::Bool),
            ),
            (
                "current_dir",
                "bloom_fs_current_dir",
                vec![],
                str_err(Type::String),
            ),
            (
                "join_path",
                "bloom_fs_join_path",
                vec![Type::String, Type::String],
                Type::String,
            ),
            (
                "read_dir",
                "bloom_fs_read_dir",
                vec![Type::String],
                Type::Enum(
                    "Result".to_string(),
                    vec![Type::Array(Box::new(Type::String)), Type::String],
                ),
            ),
            (
                "read_text_limited",
                "bloom_fs_read_text_limited",
                vec![Type::String, Type::Int],
                str_err(Type::String),
            ),
            (
                "read_bytes",
                "bloom_fs_read_bytes",
                vec![Type::String],
                str_err(Type::Struct("std::fs::Bytes".to_string(), Vec::new())),
            ),
            (
                "bytes_len",
                "bloom_fs_bytes_len",
                vec![Type::Struct("std::fs::Bytes".to_string(), Vec::new())],
                Type::Int,
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
        let span = Span::point(1, 1);
        let bytes_canonical = format!("{}::Bytes", module);
        self.structs
            .entry(bytes_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![StructField {
                    name: "id".to_string(),
                    ty: Type::Bytes,
                }],
                span,
                module: module.to_string(),
                is_pub: true,
            });
    }

    fn register_path_functions(&mut self, module: &str) {
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let opt_str = Type::Enum("Option".to_string(), vec![Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "join",
                "bloom_path_join",
                vec![Type::String, Type::String],
                Type::String,
            ),
            (
                "parent",
                "bloom_path_parent",
                vec![Type::String],
                opt_str.clone(),
            ),
            (
                "file_name",
                "bloom_path_file_name",
                vec![Type::String],
                opt_str.clone(),
            ),
            (
                "extension",
                "bloom_path_extension",
                vec![Type::String],
                opt_str.clone(),
            ),
            (
                "stem",
                "bloom_path_stem",
                vec![Type::String],
                opt_str.clone(),
            ),
            (
                "is_absolute",
                "bloom_path_is_absolute",
                vec![Type::String],
                Type::Bool,
            ),
            (
                "normalize",
                "bloom_path_normalize",
                vec![Type::String],
                str_err(Type::String),
            ),
            (
                "with_extension",
                "bloom_path_with_extension",
                vec![Type::String, Type::String],
                Type::String,
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_json_functions(&mut self, module: &str) {
        let span = Span::point(1, 1);
        let canonical = format!("{}::JsonValue", module);
        self.structs.entry(canonical).or_insert_with(|| StructInfo {
            params: Vec::new(),
            fields: vec![StructField {
                name: "id".to_string(),
                ty: Type::Json,
            }],
            span,
            module: module.to_string(),
            is_pub: true,
        });
        let json_ty = Type::Struct(format!("{}::JsonValue", module), Vec::new());
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let opt_json = Type::Enum("Option".to_string(), vec![json_ty.clone()]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "parse",
                "bloom_json_parse",
                vec![Type::String],
                str_err(json_ty.clone()),
            ),
            (
                "stringify",
                "bloom_json_stringify",
                vec![json_ty.clone()],
                str_err(Type::String),
            ),
            (
                "stringify_pretty",
                "bloom_json_stringify_pretty",
                vec![json_ty.clone()],
                str_err(Type::String),
            ),
            (
                "get",
                "bloom_json_get",
                vec![json_ty.clone(), Type::String],
                opt_json.clone(),
            ),
            (
                "has",
                "bloom_json_has",
                vec![json_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "get_string",
                "bloom_json_get_string",
                vec![json_ty.clone(), Type::String],
                str_err(Type::String),
            ),
            (
                "get_int",
                "bloom_json_get_int",
                vec![json_ty.clone(), Type::String],
                str_err(Type::Int),
            ),
            (
                "get_float",
                "bloom_json_get_float",
                vec![json_ty.clone(), Type::String],
                str_err(Type::Float),
            ),
            (
                "get_bool",
                "bloom_json_get_bool",
                vec![json_ty.clone(), Type::String],
                str_err(Type::Bool),
            ),
            (
                "as_string",
                "bloom_json_as_string",
                vec![json_ty.clone()],
                str_err(Type::String),
            ),
            (
                "as_int",
                "bloom_json_as_int",
                vec![json_ty.clone()],
                str_err(Type::Int),
            ),
            (
                "as_float",
                "bloom_json_as_float",
                vec![json_ty.clone()],
                str_err(Type::Float),
            ),
            (
                "as_bool",
                "bloom_json_as_bool",
                vec![json_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "array_length",
                "bloom_json_array_length",
                vec![json_ty.clone()],
                str_err(Type::Int),
            ),
            (
                "array_get",
                "bloom_json_array_get",
                vec![json_ty.clone(), Type::Int],
                opt_json.clone(),
            ),
            (
                "object_keys",
                "bloom_json_object_keys",
                vec![json_ty.clone()],
                str_err(Type::Array(Box::new(Type::String))),
            ),
            (
                "set",
                "bloom_json_set",
                vec![json_ty.clone(), Type::String, json_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "push",
                "bloom_json_push",
                vec![json_ty.clone(), json_ty.clone()],
                str_err(Type::Bool),
            ),
            ("null", "bloom_json_null", vec![], json_ty.clone()),
            (
                "make_bool",
                "bloom_json_make_bool",
                vec![Type::Bool],
                json_ty.clone(),
            ),
            (
                "make_int",
                "bloom_json_make_int",
                vec![Type::Int],
                json_ty.clone(),
            ),
            (
                "make_float",
                "bloom_json_make_float",
                vec![Type::Float],
                str_err(json_ty.clone()),
            ),
            (
                "make_string",
                "bloom_json_make_string",
                vec![Type::String],
                json_ty.clone(),
            ),
            (
                "make_array",
                "bloom_json_make_array",
                vec![],
                json_ty.clone(),
            ),
            (
                "make_object",
                "bloom_json_make_object",
                vec![],
                json_ty.clone(),
            ),
            (
                "type_of",
                "bloom_json_type_of",
                vec![json_ty.clone()],
                Type::String,
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_web_functions(&mut self, module: &str) {
        // std::web builds on the JSON value type (route parameters) and the
        // HTTP request/response helpers; register their signatures so the
        // required structs, Result instantiations and runtimes exist. The
        // HTTP runtime in turn needs the net and TLS runtimes; sessions use
        // the monotonic clock from std::time.
        self.register_json_functions("std::json");
        self.register_http_functions("std::http");
        self.register_net_functions("std::net");
        self.register_tls_functions("std::tls");
        self.register_time_functions("std::time");
        let span = Span::point(1, 1);
        let router_canonical = format!("{}::Router", module);
        self.structs
            .entry(router_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![StructField {
                    name: "id".to_string(),
                    ty: Type::Web,
                }],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        let json_ty = Type::Struct("std::json::JsonValue".to_string(), Vec::new());
        let match_canonical = format!("{}::Match", module);
        self.structs
            .entry(match_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![
                    StructField {
                        name: "status".to_string(),
                        ty: Type::Int,
                    },
                    StructField {
                        name: "name".to_string(),
                        ty: Type::String,
                    },
                    StructField {
                        name: "allow".to_string(),
                        ty: Type::String,
                    },
                    StructField {
                        name: "params".to_string(),
                        ty: json_ty.clone(),
                    },
                ],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        let router_ty = Type::Struct(format!("{}::Router", module), Vec::new());
        let match_ty = Type::Struct(format!("{}::Match", module), Vec::new());
        let form_canonical = format!("{}::FormData", module);
        self.structs
            .entry(form_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![StructField {
                    name: "id".to_string(),
                    ty: Type::Form,
                }],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        let form_ty = Type::Struct(format!("{}::FormData", module), Vec::new());
        for (name, fields) in [
            ("Cookies", vec![("id", Type::Cookies)]),
            ("SessionStore", vec![("id", Type::SessionStore)]),
            ("Session", vec![("id", Type::Session)]),
            (
                "CookieOptions",
                vec![
                    ("path", Type::String),
                    ("max_age", Type::Int),
                    ("same_site", Type::String),
                    ("http_only", Type::Bool),
                    ("insecure_http", Type::Bool),
                ],
            ),
        ] {
            let canonical = format!("{}::{}", module, name);
            let fields = fields
                .into_iter()
                .map(|(n, ty)| StructField {
                    name: n.to_string(),
                    ty,
                })
                .collect();
            self.structs.entry(canonical).or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields,
                span,
                module: module.to_string(),
                is_pub: true,
            });
        }
        let cookies_ty = Type::Struct(format!("{}::Cookies", module), Vec::new());
        let store_ty = Type::Struct(format!("{}::SessionStore", module), Vec::new());
        let session_ty = Type::Struct(format!("{}::Session", module), Vec::new());
        let options_ty = Type::Struct(format!("{}::CookieOptions", module), Vec::new());
        let request_ty = Type::Struct("std::http::HttpRequest".to_string(), Vec::new());
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let opt_str = Type::Enum("Option".to_string(), vec![Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            ("router", "bloom_web_new_router", vec![], router_ty.clone()),
            (
                "route",
                "bloom_web_route",
                vec![router_ty.clone(), Type::String, Type::String, Type::String],
                str_err(Type::Bool),
            ),
            (
                "resolve",
                "bloom_web_resolve",
                vec![router_ty.clone(), Type::String, Type::String],
                str_err(match_ty.clone()),
            ),
            (
                "resolve_request",
                "bloom_web_resolve_request",
                vec![router_ty.clone(), request_ty.clone()],
                str_err(match_ty.clone()),
            ),
            (
                "json_body",
                "bloom_web_json_body",
                vec![Type::String],
                str_err(json_ty.clone()),
            ),
            (
                "json_body_request",
                "bloom_web_json_body_request",
                vec![request_ty.clone()],
                str_err(json_ty.clone()),
            ),
            (
                "json_response",
                "bloom_web_json_response",
                vec![Type::Int, Type::Int, json_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "json_response_with",
                "bloom_web_json_response_with",
                vec![Type::Int, Type::Int, json_ty.clone(), Type::String],
                str_err(Type::Bool),
            ),
            (
                "static_file",
                "bloom_web_static_file",
                vec![Type::Int, Type::String, request_ty.clone()],
                str_err(Type::Int),
            ),
            (
                "form_body",
                "bloom_web_form_body",
                vec![Type::String],
                str_err(form_ty.clone()),
            ),
            (
                "form_body_request",
                "bloom_web_form_request",
                vec![request_ty.clone()],
                str_err(form_ty.clone()),
            ),
            (
                "form_count",
                "bloom_web_form_count",
                vec![form_ty.clone()],
                Type::Int,
            ),
            (
                "form_has",
                "bloom_web_form_has",
                vec![form_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "form_get",
                "bloom_web_form_get",
                vec![form_ty.clone(), Type::String],
                str_err(Type::String),
            ),
            (
                "form_first",
                "bloom_web_form_first",
                vec![form_ty.clone(), Type::String],
                opt_str.clone(),
            ),
            (
                "form_values",
                "bloom_web_form_values",
                vec![form_ty.clone(), Type::String],
                Type::Array(Box::new(Type::String)),
            ),
            (
                "form_names",
                "bloom_web_form_names",
                vec![form_ty.clone()],
                Type::Array(Box::new(Type::String)),
            ),
            (
                "form_encode",
                "bloom_web_form_encode",
                vec![form_ty],
                str_err(Type::String),
            ),
            (
                "cookies",
                "bloom_web_cookies_parse",
                vec![request_ty.clone()],
                str_err(cookies_ty.clone()),
            ),
            (
                "cookie_count",
                "bloom_web_cookie_count",
                vec![cookies_ty.clone()],
                Type::Int,
            ),
            (
                "cookie_has",
                "bloom_web_cookie_has",
                vec![cookies_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "cookie_get",
                "bloom_web_cookie_get",
                vec![cookies_ty.clone(), Type::String],
                str_err(Type::String),
            ),
            (
                "cookie_first",
                "bloom_web_cookie_first",
                vec![cookies_ty.clone(), Type::String],
                opt_str.clone(),
            ),
            (
                "cookie_names",
                "bloom_web_cookie_names",
                vec![cookies_ty.clone()],
                Type::Array(Box::new(Type::String)),
            ),
            (
                "set_cookie",
                "bloom_web_set_cookie",
                vec![Type::String, Type::String, options_ty.clone()],
                str_err(Type::String),
            ),
            (
                "delete_cookie",
                "bloom_web_delete_cookie",
                vec![Type::String, Type::String, Type::Bool],
                str_err(Type::String),
            ),
            (
                "cookie_headers",
                "bloom_web_cookie_headers",
                vec![Type::Array(Box::new(Type::String))],
                str_err(Type::String),
            ),
            (
                "session_store",
                "bloom_web_session_store_new",
                vec![Type::Int, Type::Int],
                str_err(store_ty.clone()),
            ),
            (
                "session_count",
                "bloom_web_session_count",
                vec![store_ty.clone()],
                Type::Int,
            ),
            (
                "session_new",
                "bloom_web_session_new",
                vec![store_ty.clone()],
                str_err(session_ty.clone()),
            ),
            (
                "session_load",
                "bloom_web_session_load",
                vec![store_ty.clone(), cookies_ty.clone(), Type::String],
                str_err(session_ty.clone()),
            ),
            (
                "session_id",
                "bloom_web_session_id",
                vec![session_ty.clone()],
                Type::String,
            ),
            (
                "session_data",
                "bloom_web_session_data",
                vec![session_ty.clone()],
                json_ty.clone(),
            ),
            (
                "session_destroy",
                "bloom_web_session_destroy",
                vec![store_ty.clone(), session_ty.clone()],
                Type::Bool,
            ),
            (
                "session_expire",
                "bloom_web_session_expire",
                vec![store_ty.clone()],
                Type::Int,
            ),
            (
                "session_cookie",
                "bloom_web_session_cookie",
                vec![session_ty.clone(), Type::String, Type::Int],
                str_err(Type::String),
            ),
            (
                "session_cookie_insecure",
                "bloom_web_session_cookie_insecure",
                vec![session_ty.clone(), Type::String, Type::Int],
                str_err(Type::String),
            ),
            (
                "csrf_token",
                "bloom_web_csrf_token",
                vec![session_ty.clone()],
                str_err(Type::String),
            ),
            (
                "csrf_check",
                "bloom_web_csrf_check",
                vec![session_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "csrf_rotate",
                "bloom_web_csrf_rotate",
                vec![session_ty.clone()],
                str_err(Type::String),
            ),
            (
                "csrf_clear",
                "bloom_web_csrf_clear",
                vec![session_ty.clone()],
                Type::Void,
            ),
            (
                "session_registered",
                "bloom_web_session_registered",
                vec![store_ty.clone(), session_ty.clone()],
                Type::Bool,
            ),
            (
                "session_regenerate",
                "bloom_web_session_regenerate",
                vec![store_ty, session_ty],
                str_err(Type::Struct(format!("{}::Session", module), Vec::new())),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_html_functions(&mut self, module: &str) {
        // Templates render JSON data and can be sent over the existing HTTP
        // response helpers; register those signatures so the required
        // structs, Result instantiations and runtimes exist. Loading
        // templates from files needs the fs helpers as well.
        self.register_json_functions("std::json");
        self.register_http_functions("std::http");
        self.register_net_functions("std::net");
        self.register_tls_functions("std::tls");
        self.register_fs_functions("std::fs");
        let span = Span::point(1, 1);
        let template_canonical = format!("{}::Template", module);
        self.structs
            .entry(template_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![StructField {
                    name: "id".to_string(),
                    ty: Type::Html,
                }],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        let template_ty = Type::Struct(format!("{}::Template", module), Vec::new());
        let json_ty = Type::Struct("std::json::JsonValue".to_string(), Vec::new());
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "escape",
                "bloom_html_escape",
                vec![Type::String],
                Type::String,
            ),
            (
                "template",
                "bloom_html_template",
                vec![Type::String],
                str_err(template_ty.clone()),
            ),
            (
                "render",
                "bloom_html_render",
                vec![template_ty.clone(), json_ty.clone()],
                str_err(Type::String),
            ),
            (
                "render_page",
                "bloom_html_render_page",
                vec![Type::Int, Type::Int, template_ty.clone(), json_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "render_page_with",
                "bloom_html_render_page_with",
                vec![
                    Type::Int,
                    Type::Int,
                    template_ty.clone(),
                    json_ty.clone(),
                    Type::String,
                ],
                str_err(Type::Bool),
            ),
            (
                "template_file",
                "bloom_html_template_file",
                vec![Type::String],
                str_err(template_ty.clone()),
            ),
            (
                "template_file_from",
                "bloom_html_template_file_from",
                vec![Type::String, Type::String],
                str_err(template_ty.clone()),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_db_functions(&mut self, module: &str) {
        // BLOB columns use the managed std::fs::Bytes buffer, so the fs
        // runtime and its struct must exist as well.
        self.register_fs_functions("std::fs");
        let span = Span::point(1, 1);
        for (name, ty) in [("Database", Type::Db), ("Statement", Type::Stmt)] {
            let canonical = format!("{}::{}", module, name);
            self.structs.entry(canonical).or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![StructField {
                    name: "id".to_string(),
                    ty,
                }],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        }
        let db_ty = Type::Struct(format!("{}::Database", module), Vec::new());
        let stmt_ty = Type::Struct(format!("{}::Statement", module), Vec::new());
        let bytes_ty = Type::Struct("std::fs::Bytes".to_string(), Vec::new());
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "open",
                "bloom_db_open",
                vec![Type::String],
                str_err(db_ty.clone()),
            ),
            (
                "open_memory",
                "bloom_db_open_memory",
                vec![],
                str_err(db_ty.clone()),
            ),
            (
                "busy_timeout",
                "bloom_db_busy_timeout",
                vec![db_ty.clone(), Type::Int],
                str_err(Type::Bool),
            ),
            (
                "prepare",
                "bloom_db_prepare",
                vec![db_ty.clone(), Type::String],
                str_err(stmt_ty.clone()),
            ),
            (
                "bind_null",
                "bloom_stmt_bind_null",
                vec![stmt_ty.clone(), Type::Int],
                str_err(Type::Bool),
            ),
            (
                "bind_int",
                "bloom_stmt_bind_int",
                vec![stmt_ty.clone(), Type::Int, Type::Int],
                str_err(Type::Bool),
            ),
            (
                "bind_float",
                "bloom_stmt_bind_float",
                vec![stmt_ty.clone(), Type::Int, Type::Float],
                str_err(Type::Bool),
            ),
            (
                "bind_bool",
                "bloom_stmt_bind_bool",
                vec![stmt_ty.clone(), Type::Int, Type::Bool],
                str_err(Type::Bool),
            ),
            (
                "bind_text",
                "bloom_stmt_bind_text",
                vec![stmt_ty.clone(), Type::Int, Type::String],
                str_err(Type::Bool),
            ),
            (
                "bind_bytes",
                "bloom_stmt_bind_bytes",
                vec![stmt_ty.clone(), Type::Int, bytes_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "step",
                "bloom_stmt_step",
                vec![stmt_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "execute",
                "bloom_stmt_execute",
                vec![stmt_ty.clone()],
                str_err(Type::Int),
            ),
            (
                "reset",
                "bloom_stmt_reset",
                vec![stmt_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "column_count",
                "bloom_stmt_column_count",
                vec![stmt_ty.clone()],
                Type::Int,
            ),
            (
                "column_name",
                "bloom_stmt_column_name",
                vec![stmt_ty.clone(), Type::Int],
                str_err(Type::String),
            ),
            (
                "column_index",
                "bloom_stmt_column_index",
                vec![stmt_ty.clone(), Type::String],
                str_err(Type::Int),
            ),
            (
                "column_type",
                "bloom_stmt_column_type",
                vec![stmt_ty.clone(), Type::Int],
                str_err(Type::String),
            ),
            (
                "column_int",
                "bloom_stmt_column_int",
                vec![stmt_ty.clone(), Type::Int],
                str_err(Type::Int),
            ),
            (
                "column_float",
                "bloom_stmt_column_float",
                vec![stmt_ty.clone(), Type::Int],
                str_err(Type::Float),
            ),
            (
                "column_bool",
                "bloom_stmt_column_bool",
                vec![stmt_ty.clone(), Type::Int],
                str_err(Type::Bool),
            ),
            (
                "column_text",
                "bloom_stmt_column_text",
                vec![stmt_ty.clone(), Type::Int],
                str_err(Type::String),
            ),
            (
                "column_bytes",
                "bloom_stmt_column_bytes",
                vec![stmt_ty.clone(), Type::Int],
                str_err(bytes_ty),
            ),
            (
                "changes",
                "bloom_db_changes",
                vec![db_ty.clone()],
                Type::Int,
            ),
            (
                "last_insert_rowid",
                "bloom_db_last_insert_rowid",
                vec![db_ty.clone()],
                Type::Int,
            ),
            (
                "begin",
                "bloom_db_begin",
                vec![db_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "commit",
                "bloom_db_commit",
                vec![db_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "rollback",
                "bloom_db_rollback",
                vec![db_ty.clone()],
                str_err(Type::Bool),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_auth_functions(&mut self, module: &str) {
        // Authentication builds on SQLite, JSON session data and the web
        // session/CSRF machinery; register those signatures (and runtimes)
        // first.
        self.register_web_functions("std::web");
        self.register_db_functions("std::db");
        let span = Span::point(1, 1);
        let user_canonical = format!("{}::UserInfo", module);
        self.structs
            .entry(user_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![
                    StructField {
                        name: "id".to_string(),
                        ty: Type::Int,
                    },
                    StructField {
                        name: "username".to_string(),
                        ty: Type::String,
                    },
                    StructField {
                        name: "display_name".to_string(),
                        ty: Type::String,
                    },
                    StructField {
                        name: "active".to_string(),
                        ty: Type::Bool,
                    },
                    StructField {
                        name: "auth_version".to_string(),
                        ty: Type::Int,
                    },
                ],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        let limiter_canonical = format!("{}::Limiter", module);
        self.structs
            .entry(limiter_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![StructField {
                    name: "id".to_string(),
                    ty: Type::Limiter,
                }],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        let db_ty = Type::Struct("std::db::Database".to_string(), Vec::new());
        let store_ty = Type::Struct("std::web::SessionStore".to_string(), Vec::new());
        let session_ty = Type::Struct("std::web::Session".to_string(), Vec::new());
        let user_ty = Type::Struct(format!("{}::UserInfo", module), Vec::new());
        let limiter_ty = Type::Struct(format!("{}::Limiter", module), Vec::new());
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let opt_user = Type::Enum("Option".to_string(), vec![user_ty.clone()]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "password_hash",
                "bloom_auth_password_hash",
                vec![Type::String],
                str_err(Type::String),
            ),
            (
                "password_verify",
                "bloom_auth_password_verify",
                vec![Type::String, Type::String],
                str_err(Type::Bool),
            ),
            (
                "password_needs_rehash",
                "bloom_auth_password_needs_rehash",
                vec![Type::String],
                str_err(Type::Bool),
            ),
            (
                "setup",
                "bloom_auth_setup",
                vec![db_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "register_user",
                "bloom_auth_register_user",
                vec![db_ty.clone(), Type::String, Type::String],
                str_err(Type::Int),
            ),
            (
                "authenticate",
                "bloom_auth_authenticate",
                vec![db_ty.clone(), Type::String, Type::String],
                str_err(Type::Int),
            ),
            (
                "user_get",
                "bloom_auth_user_get",
                vec![db_ty.clone(), Type::Int],
                str_err(user_ty.clone()),
            ),
            (
                "user_find",
                "bloom_auth_user_find",
                vec![db_ty.clone(), Type::String],
                str_err(Type::Int),
            ),
            (
                "user_disable",
                "bloom_auth_user_disable",
                vec![db_ty.clone(), Type::Int],
                str_err(Type::Bool),
            ),
            (
                "user_enable",
                "bloom_auth_user_enable",
                vec![db_ty.clone(), Type::Int],
                str_err(Type::Bool),
            ),
            (
                "password_change",
                "bloom_auth_password_change",
                vec![db_ty.clone(), Type::Int, Type::String],
                str_err(Type::Bool),
            ),
            (
                "login",
                "bloom_auth_login",
                vec![
                    store_ty.clone(),
                    session_ty.clone(),
                    db_ty.clone(),
                    Type::String,
                    Type::String,
                ],
                str_err(session_ty.clone()),
            ),
            (
                "logout",
                "bloom_auth_logout",
                vec![store_ty.clone(), session_ty.clone()],
                str_err(Type::Bool),
            ),
            (
                "current_user",
                "bloom_auth_current_user",
                vec![db_ty.clone(), store_ty.clone(), session_ty.clone()],
                Type::Enum("Result".to_string(), vec![opt_user, Type::String]),
            ),
            (
                "role_create",
                "bloom_auth_role_create",
                vec![db_ty.clone(), Type::String],
                str_err(Type::Bool),
            ),
            (
                "role_delete",
                "bloom_auth_role_delete",
                vec![db_ty.clone(), Type::String],
                str_err(Type::Bool),
            ),
            (
                "role_assign",
                "bloom_auth_role_assign",
                vec![db_ty.clone(), Type::Int, Type::String],
                str_err(Type::Bool),
            ),
            (
                "role_remove",
                "bloom_auth_role_remove",
                vec![db_ty.clone(), Type::Int, Type::String],
                str_err(Type::Bool),
            ),
            (
                "roles_of",
                "bloom_auth_roles_of",
                vec![db_ty.clone(), Type::Int],
                Type::Enum(
                    "Result".to_string(),
                    vec![Type::Array(Box::new(Type::String)), Type::String],
                ),
            ),
            (
                "has_role",
                "bloom_auth_has_role",
                vec![db_ty.clone(), Type::Int, Type::String],
                str_err(Type::Bool),
            ),
            (
                "limiter",
                "bloom_auth_limiter_new",
                vec![Type::Int, Type::Int, Type::Int],
                str_err(limiter_ty.clone()),
            ),
            (
                "limiter_check",
                "bloom_auth_limiter_check",
                vec![limiter_ty.clone(), Type::String],
                str_err(Type::Bool),
            ),
            (
                "limiter_reset",
                "bloom_auth_limiter_reset",
                vec![limiter_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "limiter_count",
                "bloom_auth_limiter_count",
                vec![limiter_ty.clone()],
                Type::Int,
            ),
            (
                "limiter_expire",
                "bloom_auth_limiter_expire",
                vec![limiter_ty],
                Type::Int,
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }
    fn register_ui_functions(&mut self, module: &str) {
        // The UI layer renders through the existing HTML escaper and the
        // HTTP response helpers; register those runtimes first.
        self.register_html_functions("std::html");
        let span = Span::point(1, 1);
        let node_canonical = format!("{}::UiNode", module);
        self.structs
            .entry(node_canonical)
            .or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields: vec![StructField {
                    name: "id".to_string(),
                    ty: Type::Ui,
                }],
                span,
                module: module.to_string(),
                is_pub: true,
            });
        let option_structs: Vec<(&str, Vec<(&str, Type)>)> = vec![
            (
                "ButtonOptions",
                vec![
                    ("variant", Type::String),
                    ("size", Type::String),
                    ("type_name", Type::String),
                    ("name", Type::String),
                    ("value", Type::String),
                    ("disabled", Type::Bool),
                ],
            ),
            (
                "InputOptions",
                vec![
                    ("type_name", Type::String),
                    ("name", Type::String),
                    ("value", Type::String),
                    ("id", Type::String),
                    ("placeholder", Type::String),
                    ("required", Type::Bool),
                    ("disabled", Type::Bool),
                ],
            ),
            (
                "FormOptions",
                vec![("action", Type::String), ("method", Type::String)],
            ),
            (
                "PageOptions",
                vec![
                    ("title", Type::String),
                    ("description", Type::String),
                    ("lang", Type::String),
                    ("theme", Type::String),
                    ("stylesheet_href", Type::String),
                ],
            ),
            ("TableOptions", vec![("caption", Type::String)]),
        ];
        for (name, fields) in option_structs {
            let canonical = format!("{}::{}", module, name);
            let fields = fields
                .into_iter()
                .map(|(n, ty)| StructField {
                    name: n.to_string(),
                    ty,
                })
                .collect();
            self.structs.entry(canonical).or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields,
                span,
                module: module.to_string(),
                is_pub: true,
            });
        }
        let node_ty = Type::Struct(format!("{}::UiNode", module), Vec::new());
        let button_options = Type::Struct(format!("{}::ButtonOptions", module), Vec::new());
        let input_options = Type::Struct(format!("{}::InputOptions", module), Vec::new());
        let form_options = Type::Struct(format!("{}::FormOptions", module), Vec::new());
        let page_options = Type::Struct(format!("{}::PageOptions", module), Vec::new());
        let table_options = Type::Struct(format!("{}::TableOptions", module), Vec::new());
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let node_err = str_err(node_ty.clone());
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "append",
                "bloom_ui_append",
                vec![node_ty.clone(), node_ty.clone()],
                Type::Bool,
            ),
            (
                "set_id",
                "bloom_ui_set_id",
                vec![node_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "placeholder",
                "bloom_ui_placeholder",
                vec![node_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "require",
                "bloom_ui_require",
                vec![node_ty.clone()],
                Type::Bool,
            ),
            (
                "disable",
                "bloom_ui_disable",
                vec![node_ty.clone()],
                Type::Bool,
            ),
            (
                "readonly",
                "bloom_ui_readonly",
                vec![node_ty.clone()],
                Type::Bool,
            ),
            (
                "set_value",
                "bloom_ui_set_value",
                vec![node_ty.clone(), Type::String],
                Type::Bool,
            ),
            ("check", "bloom_ui_check", vec![node_ty.clone()], Type::Bool),
            (
                "select_option",
                "bloom_ui_select_option",
                vec![node_ty.clone()],
                Type::Bool,
            ),
            (
                "set_aria_label",
                "bloom_ui_set_aria_label",
                vec![node_ty.clone(), Type::String],
                Type::Bool,
            ),
            (
                "set_class",
                "bloom_ui_set_class",
                vec![node_ty.clone(), Type::String],
                str_err(Type::Bool),
            ),
            (
                "container",
                "bloom_ui_container",
                vec![Type::String],
                node_err.clone(),
            ),
            (
                "stack",
                "bloom_ui_stack",
                vec![Type::String],
                node_err.clone(),
            ),
            (
                "row",
                "bloom_ui_row",
                vec![Type::String, Type::Bool],
                node_err.clone(),
            ),
            (
                "grid",
                "bloom_ui_grid",
                vec![Type::Int, Type::String],
                node_err.clone(),
            ),
            (
                "section",
                "bloom_ui_section",
                vec![Type::String],
                node_ty.clone(),
            ),
            ("divider", "bloom_ui_divider", vec![], node_ty.clone()),
            (
                "heading",
                "bloom_ui_heading",
                vec![Type::Int, Type::String],
                node_err.clone(),
            ),
            ("text", "bloom_ui_text", vec![Type::String], node_ty.clone()),
            (
                "muted_text",
                "bloom_ui_muted_text",
                vec![Type::String],
                node_ty.clone(),
            ),
            (
                "link",
                "bloom_ui_link",
                vec![Type::String, Type::String],
                node_err.clone(),
            ),
            (
                "badge",
                "bloom_ui_badge",
                vec![Type::String, Type::String],
                node_err.clone(),
            ),
            ("card", "bloom_ui_card", vec![Type::String], node_ty.clone()),
            (
                "alert",
                "bloom_ui_alert",
                vec![Type::String, Type::String],
                node_err.clone(),
            ),
            (
                "button",
                "bloom_ui_button",
                vec![Type::String, button_options],
                node_err.clone(),
            ),
            (
                "form",
                "bloom_ui_form",
                vec![form_options],
                node_err.clone(),
            ),
            (
                "label",
                "bloom_ui_label",
                vec![Type::String, Type::String],
                node_err.clone(),
            ),
            (
                "input",
                "bloom_ui_input",
                vec![input_options],
                node_err.clone(),
            ),
            (
                "textarea",
                "bloom_ui_textarea",
                vec![Type::String, Type::String, Type::Int, Type::String],
                node_err.clone(),
            ),
            (
                "select",
                "bloom_ui_select",
                vec![Type::String, Type::String, Type::Bool],
                node_err.clone(),
            ),
            (
                "option",
                "bloom_ui_option",
                vec![Type::String, Type::String],
                node_ty.clone(),
            ),
            (
                "checkbox",
                "bloom_ui_checkbox",
                vec![Type::String, Type::String, Type::String],
                node_err.clone(),
            ),
            (
                "radio",
                "bloom_ui_radio",
                vec![Type::String, Type::String, Type::String],
                node_err.clone(),
            ),
            (
                "field",
                "bloom_ui_field",
                vec![
                    node_ty.clone(),
                    Type::String,
                    Type::String,
                    Type::String,
                    Type::String,
                ],
                node_err.clone(),
            ),
            (
                "help_text",
                "bloom_ui_help_text",
                vec![Type::String],
                node_ty.clone(),
            ),
            (
                "form_error",
                "bloom_ui_form_error",
                vec![Type::String],
                node_ty.clone(),
            ),
            (
                "submit_button",
                "bloom_ui_submit_button",
                vec![Type::String],
                node_ty.clone(),
            ),
            ("nav", "bloom_ui_nav", vec![Type::String], node_ty.clone()),
            (
                "nav_link",
                "bloom_ui_nav_link",
                vec![Type::String, Type::String, Type::Bool],
                node_err.clone(),
            ),
            (
                "breadcrumbs",
                "bloom_ui_breadcrumbs",
                vec![Type::String],
                node_ty.clone(),
            ),
            (
                "breadcrumb",
                "bloom_ui_breadcrumb",
                vec![Type::String, Type::String],
                node_err.clone(),
            ),
            (
                "table",
                "bloom_ui_table",
                vec![table_options],
                node_err.clone(),
            ),
            (
                "table_row",
                "bloom_ui_table_row",
                vec![node_ty.clone(), Type::Bool],
                node_err.clone(),
            ),
            (
                "table_cell",
                "bloom_ui_table_cell",
                vec![node_ty.clone(), Type::String],
                node_err.clone(),
            ),
            (
                "table_header_cell",
                "bloom_ui_table_header_cell",
                vec![node_ty.clone(), Type::String],
                node_err.clone(),
            ),
            (
                "empty_state",
                "bloom_ui_empty_state",
                vec![Type::String, Type::String],
                node_ty.clone(),
            ),
            (
                "avatar",
                "bloom_ui_avatar",
                vec![Type::String, Type::String, Type::String],
                node_err.clone(),
            ),
            (
                "progress",
                "bloom_ui_progress",
                vec![Type::Int, Type::Int, Type::String],
                node_err.clone(),
            ),
            (
                "skeleton",
                "bloom_ui_skeleton",
                vec![Type::Int],
                node_err.clone(),
            ),
            (
                "pagination",
                "bloom_ui_pagination",
                vec![Type::Int, Type::Int, Type::String],
                node_err.clone(),
            ),
            (
                "document",
                "bloom_ui_document",
                vec![page_options],
                node_err.clone(),
            ),
            (
                "body",
                "bloom_ui_body",
                vec![node_ty.clone()],
                node_ty.clone(),
            ),
            (
                "render",
                "bloom_ui_render",
                vec![node_ty.clone()],
                str_err(Type::String),
            ),
            (
                "send_page",
                "bloom_ui_send_page",
                vec![Type::Int, Type::Int, node_ty.clone(), Type::String],
                str_err(Type::Bool),
            ),
            ("stylesheet", "bloom_ui_stylesheet", vec![], Type::String),
            (
                "send_stylesheet",
                "bloom_ui_send_stylesheet",
                vec![Type::Int],
                str_err(Type::Bool),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }
    fn register_desktop_functions(&mut self, module: &str) {
        let span = Span::point(1, 1);
        let struct_defs: Vec<(&str, Vec<(&str, Type)>)> = vec![
            ("Window", vec![("id", Type::DesktopWindow)]),
            (
                "Event",
                vec![
                    ("kind", Type::String),
                    ("control", Type::Int),
                    ("text", Type::String),
                    ("checked", Type::Bool),
                ],
            ),
            (
                "Bounds",
                vec![
                    ("x", Type::Int),
                    ("y", Type::Int),
                    ("width", Type::Int),
                    ("height", Type::Int),
                ],
            ),
            ("Size", vec![("width", Type::Int), ("height", Type::Int)]),
        ];
        for (name, fields) in struct_defs {
            let canonical = format!("{}::{}", module, name);
            let fields = fields
                .into_iter()
                .map(|(n, ty)| StructField {
                    name: n.to_string(),
                    ty,
                })
                .collect();
            self.structs.entry(canonical).or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields,
                span,
                module: module.to_string(),
                is_pub: true,
            });
        }
        let window_ty = Type::Struct(format!("{}::Window", module), Vec::new());
        let event_ty = Type::Struct(format!("{}::Event", module), Vec::new());
        let bounds_ty = Type::Struct(format!("{}::Bounds", module), Vec::new());
        let size_ty = Type::Struct(format!("{}::Size", module), Vec::new());
        let result = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let window_err = result(window_ty.clone());
        let bool_err = result(Type::Bool);
        let int_err = result(Type::Int);
        let str_err = result(Type::String);
        let event_err = result(event_ty);
        let bounds_err = result(bounds_ty);
        let size_err = result(size_ty);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            ("available", "bloom_desktop_available", vec![], Type::Bool),
            (
                "window_create",
                "bloom_desktop_window_create",
                vec![Type::String, Type::Int, Type::Int],
                window_err,
            ),
            (
                "window_show",
                "bloom_desktop_window_show",
                vec![window_ty.clone()],
                bool_err.clone(),
            ),
            (
                "window_hide",
                "bloom_desktop_window_hide",
                vec![window_ty.clone()],
                bool_err.clone(),
            ),
            (
                "window_set_title",
                "bloom_desktop_window_set_title",
                vec![window_ty.clone(), Type::String],
                bool_err.clone(),
            ),
            (
                "window_set_size",
                "bloom_desktop_window_set_size",
                vec![window_ty.clone(), Type::Int, Type::Int],
                bool_err.clone(),
            ),
            (
                "window_set_position",
                "bloom_desktop_window_set_position",
                vec![window_ty.clone(), Type::Int, Type::Int],
                bool_err.clone(),
            ),
            (
                "window_close",
                "bloom_desktop_window_close",
                vec![window_ty.clone()],
                bool_err.clone(),
            ),
            (
                "window_alive",
                "bloom_desktop_window_alive",
                vec![window_ty.clone()],
                Type::Bool,
            ),
            (
                "window_dpi",
                "bloom_desktop_window_dpi",
                vec![window_ty.clone()],
                int_err.clone(),
            ),
            (
                "window_client_size",
                "bloom_desktop_window_client_size",
                vec![window_ty.clone()],
                size_err,
            ),
            (
                "message_box",
                "bloom_desktop_message_box",
                vec![Type::String, Type::String],
                bool_err.clone(),
            ),
            (
                "label_create",
                "bloom_desktop_label_create",
                vec![
                    window_ty.clone(),
                    Type::String,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                ],
                int_err.clone(),
            ),
            (
                "button_create",
                "bloom_desktop_button_create",
                vec![
                    window_ty.clone(),
                    Type::String,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                ],
                int_err.clone(),
            ),
            (
                "text_input_create",
                "bloom_desktop_text_input_create",
                vec![
                    window_ty.clone(),
                    Type::String,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                ],
                int_err.clone(),
            ),
            (
                "checkbox_create",
                "bloom_desktop_checkbox_create",
                vec![
                    window_ty.clone(),
                    Type::String,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                ],
                int_err.clone(),
            ),
            (
                "control_text",
                "bloom_desktop_control_text",
                vec![window_ty.clone(), Type::Int],
                str_err,
            ),
            (
                "control_set_text",
                "bloom_desktop_control_set_text",
                vec![window_ty.clone(), Type::Int, Type::String],
                bool_err.clone(),
            ),
            (
                "control_checked",
                "bloom_desktop_control_checked",
                vec![window_ty.clone(), Type::Int],
                bool_err.clone(),
            ),
            (
                "control_set_checked",
                "bloom_desktop_control_set_checked",
                vec![window_ty.clone(), Type::Int, Type::Bool],
                bool_err.clone(),
            ),
            (
                "control_enabled",
                "bloom_desktop_control_enabled",
                vec![window_ty.clone(), Type::Int],
                bool_err.clone(),
            ),
            (
                "control_set_enabled",
                "bloom_desktop_control_set_enabled",
                vec![window_ty.clone(), Type::Int, Type::Bool],
                bool_err.clone(),
            ),
            (
                "control_visible",
                "bloom_desktop_control_visible",
                vec![window_ty.clone(), Type::Int],
                bool_err.clone(),
            ),
            (
                "control_set_visible",
                "bloom_desktop_control_set_visible",
                vec![window_ty.clone(), Type::Int, Type::Bool],
                bool_err.clone(),
            ),
            (
                "control_bounds",
                "bloom_desktop_control_bounds",
                vec![window_ty.clone(), Type::Int],
                bounds_err,
            ),
            (
                "control_set_bounds",
                "bloom_desktop_control_set_bounds",
                vec![
                    window_ty.clone(),
                    Type::Int,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                    Type::Int,
                ],
                bool_err.clone(),
            ),
            (
                "control_focus",
                "bloom_desktop_control_focus",
                vec![window_ty.clone(), Type::Int],
                bool_err,
            ),
            (
                "next_event",
                "bloom_desktop_next_event",
                vec![window_ty.clone()],
                event_err,
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }
    fn register_process_functions(&mut self, module: &str) {
        let span = Span::point(1, 1);
        let canonical = format!("{}::ProcessOutput", module);
        self.structs.entry(canonical).or_insert_with(|| StructInfo {
            params: Vec::new(),
            fields: vec![
                StructField {
                    name: "exit_code".to_string(),
                    ty: Type::Int,
                },
                StructField {
                    name: "stdout".to_string(),
                    ty: Type::String,
                },
                StructField {
                    name: "stderr".to_string(),
                    ty: Type::String,
                },
            ],
            span,
            module: module.to_string(),
            is_pub: true,
        });
        let out = Type::Struct(format!("{}::ProcessOutput", module), Vec::new());
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "run",
                "bloom_process_run",
                vec![Type::String, Type::Array(Box::new(Type::String))],
                Type::Enum("Result".to_string(), vec![out.clone(), Type::String]),
            ),
            (
                "run_in",
                "bloom_process_run_in",
                vec![
                    Type::String,
                    Type::Array(Box::new(Type::String)),
                    Type::String,
                ],
                Type::Enum("Result".to_string(), vec![out, Type::String]),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_time_functions(&mut self, module: &str) {
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            ("unix_seconds", "bloom_time_unix_seconds", vec![], Type::Int),
            ("unix_millis", "bloom_time_unix_millis", vec![], Type::Int),
            (
                "monotonic_millis",
                "bloom_time_monotonic_millis",
                vec![],
                Type::Int,
            ),
            (
                "sleep_ms",
                "bloom_time_sleep_ms",
                vec![Type::Int],
                Type::Void,
            ),
            (
                "format_utc",
                "bloom_time_format_utc",
                vec![Type::Int],
                Type::String,
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_math_functions(&mut self, module: &str) {
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            ("sqrt", "bloom_math_sqrt", vec![Type::Float], Type::Float),
            (
                "pow",
                "bloom_math_pow",
                vec![Type::Float, Type::Float],
                Type::Float,
            ),
            ("floor", "bloom_math_floor", vec![Type::Float], Type::Float),
            ("ceil", "bloom_math_ceil", vec![Type::Float], Type::Float),
            ("round", "bloom_math_round", vec![Type::Float], Type::Float),
            ("abs", "bloom_math_abs", vec![Type::Float], Type::Float),
            (
                "min",
                "bloom_math_min",
                vec![Type::Float, Type::Float],
                Type::Float,
            ),
            (
                "max",
                "bloom_math_max",
                vec![Type::Float, Type::Float],
                Type::Float,
            ),
            (
                "clamp",
                "bloom_math_clamp",
                vec![Type::Float, Type::Float, Type::Float],
                Type::Float,
            ),
            ("abs_int", "bloom_math_abs_int", vec![Type::Int], Type::Int),
            (
                "min_int",
                "bloom_math_min_int",
                vec![Type::Int, Type::Int],
                Type::Int,
            ),
            (
                "max_int",
                "bloom_math_max_int",
                vec![Type::Int, Type::Int],
                Type::Int,
            ),
            (
                "clamp_int",
                "bloom_math_clamp_int",
                vec![Type::Int, Type::Int, Type::Int],
                Type::Int,
            ),
        ];
        let span = Span::point(1, 1);
        for (name, c_name, params, ret) in defs {
            let canonical = format!("{}::{}", module, name);
            if self.fn_index.contains_key(&canonical) {
                continue;
            }
            let idx = self.functions.len();
            self.fn_index.insert(canonical, idx);
            self.functions.push(FnSig {
                c_name: c_name.to_string(),
                type_params: Vec::new(),
                params,
                ret,
                body: Block {
                    stmts: Vec::new(),
                    span,
                },
                span,
                module: module.to_string(),
                is_pub: true,
                is_extern: false,
                system_abi: false,
            });
        }
    }

    fn register_win32_functions(&mut self, module: &str) {
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            ("process_id", "bloom_win32_process_id", vec![], Type::Int),
            (
                "tick_count_ms",
                "bloom_win32_tick_count_ms",
                vec![],
                Type::Int,
            ),
            ("last_error", "bloom_win32_last_error", vec![], Type::Int),
            (
                "get_computer_name",
                "bloom_win32_get_computer_name",
                vec![],
                str_err(Type::String),
            ),
            (
                "sleep_ms",
                "bloom_win32_sleep_ms",
                vec![Type::Int],
                Type::Void,
            ),
            (
                "message_box",
                "bloom_win32_message_box",
                vec![Type::String, Type::String, Type::Int],
                str_err(Type::Int),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_net_functions(&mut self, module: &str) {
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let opt_str = Type::Enum("Option".to_string(), vec![Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "tcp_connect",
                "bloom_net_tcp_connect",
                vec![Type::String, Type::Int],
                str_err(Type::Int),
            ),
            (
                "tcp_listen",
                "bloom_net_tcp_listen",
                vec![Type::String, Type::Int, Type::Int],
                str_err(Type::Int),
            ),
            (
                "tcp_accept",
                "bloom_net_tcp_accept",
                vec![Type::Int],
                str_err(Type::Int),
            ),
            (
                "tcp_read",
                "bloom_net_tcp_read",
                vec![Type::Int, Type::Int],
                str_err(opt_str),
            ),
            (
                "tcp_write_all",
                "bloom_net_tcp_write_all",
                vec![Type::Int, Type::String],
                str_err(Type::Int),
            ),
            (
                "tcp_close",
                "bloom_net_tcp_close",
                vec![Type::Int],
                str_err(Type::Bool),
            ),
            (
                "tcp_set_timeout",
                "bloom_net_tcp_set_timeout",
                vec![Type::Int, Type::Int],
                str_err(Type::Bool),
            ),
            (
                "local_port",
                "bloom_net_local_port",
                vec![Type::Int],
                str_err(Type::Int),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_tls_functions(&mut self, module: &str) {
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let opt_str = Type::Enum("Option".to_string(), vec![Type::String]);
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "tls_connect",
                "bloom_tls_connect",
                vec![Type::String, Type::Int],
                str_err(Type::Int),
            ),
            (
                "tls_connect_with_ca",
                "bloom_tls_connect_with_ca",
                vec![Type::String, Type::Int, Type::String],
                str_err(Type::Int),
            ),
            (
                "tls_connect_ex",
                "bloom_tls_connect_ex",
                vec![Type::String, Type::Int, Type::Int, Type::String],
                str_err(Type::Int),
            ),
            (
                "tls_read",
                "bloom_tls_read",
                vec![Type::Int, Type::Int],
                str_err(opt_str),
            ),
            (
                "tls_write_all",
                "bloom_tls_write_all",
                vec![Type::Int, Type::String],
                str_err(Type::Int),
            ),
            (
                "tls_close",
                "bloom_tls_close",
                vec![Type::Int],
                str_err(Type::Bool),
            ),
            (
                "tls_set_timeout",
                "bloom_tls_set_timeout",
                vec![Type::Int, Type::Int],
                str_err(Type::Bool),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn register_http_functions(&mut self, module: &str) {
        // Binary responses carry a byte buffer, so the fs runtime (and its
        // `Bytes` struct) must exist as well.
        self.register_fs_functions("std::fs");
        let span = Span::point(1, 1);
        let request_fields: Vec<(&str, Type)> = vec![
            ("method", Type::String),
            ("target", Type::String),
            ("version", Type::String),
            ("headers", Type::String),
            ("body", Type::String),
        ];
        let response_fields: Vec<(&str, Type)> = vec![
            ("status", Type::Int),
            ("reason", Type::String),
            ("version", Type::String),
            ("headers", Type::String),
            ("body", Type::String),
        ];
        for (name, fields) in [
            ("HttpRequest", request_fields),
            ("HttpResponse", response_fields),
        ] {
            let canonical = format!("{}::{}", module, name);
            let fields = fields
                .into_iter()
                .map(|(n, ty)| StructField {
                    name: n.to_string(),
                    ty,
                })
                .collect();
            self.structs.entry(canonical).or_insert_with(|| StructInfo {
                params: Vec::new(),
                fields,
                span,
                module: module.to_string(),
                is_pub: true,
            });
        }
        let str_err = |ok: Type| Type::Enum("Result".to_string(), vec![ok, Type::String]);
        let request_ty = Type::Struct(format!("{}::HttpRequest", module), Vec::new());
        let response_ty = Type::Struct(format!("{}::HttpResponse", module), Vec::new());
        let bytes_ty = Type::Struct("std::fs::Bytes".to_string(), Vec::new());
        let defs: Vec<(&str, &str, Vec<Type>, Type)> = vec![
            (
                "get",
                "bloom_http_get",
                vec![Type::String],
                str_err(response_ty.clone()),
            ),
            (
                "post",
                "bloom_http_post",
                vec![Type::String, Type::String, Type::String],
                str_err(response_ty.clone()),
            ),
            (
                "request",
                "bloom_http_request",
                vec![Type::String, Type::String, Type::String, Type::String],
                str_err(response_ty.clone()),
            ),
            (
                "read_request",
                "bloom_http_read_request",
                vec![Type::Int],
                str_err(request_ty),
            ),
            (
                "write_response",
                "bloom_http_write_response",
                vec![Type::Int, Type::Int, Type::String, Type::String],
                str_err(Type::Bool),
            ),
            (
                "write_response_full",
                "bloom_http_write_response_full",
                vec![
                    Type::Int,
                    Type::Int,
                    Type::String,
                    Type::String,
                    Type::String,
                ],
                str_err(Type::Bool),
            ),
            (
                "header_get",
                "bloom_http_header_get",
                vec![Type::String, Type::String],
                Type::Enum("Option".to_string(), vec![Type::String]),
            ),
            (
                "get_with_ca",
                "bloom_http_get_with_ca",
                vec![Type::String, Type::String],
                str_err(response_ty.clone()),
            ),
            (
                "post_with_ca",
                "bloom_http_post_with_ca",
                vec![Type::String, Type::String, Type::String, Type::String],
                str_err(response_ty.clone()),
            ),
            (
                "request_with_ca",
                "bloom_http_request_with_ca",
                vec![
                    Type::String,
                    Type::String,
                    Type::String,
                    Type::String,
                    Type::String,
                ],
                str_err(response_ty),
            ),
            (
                "write_response_bytes",
                "bloom_http_write_response_bytes",
                vec![
                    Type::Int,
                    Type::Int,
                    Type::String,
                    Type::String,
                    bytes_ty.clone(),
                ],
                str_err(Type::Bool),
            ),
            (
                "write_response_bytes_head",
                "bloom_http_write_response_bytes_head",
                vec![Type::Int, Type::Int, Type::String, Type::String, bytes_ty],
                str_err(Type::Bool),
            ),
        ];
        for (name, c_name, params, ret) in defs {
            self.register_builtin_fn(module, name, c_name, params, ret);
        }
    }

    fn collect_functions(&mut self, program: &mut Program) {
        for item in &mut program.items {
            let Item::Function(f) = item else { continue };
            if builtin_kind(&f.name).is_some() {
                self.error(
                    codes::DUPLICATE_DECL,
                    format!(
                        "`{}` is a built-in function and cannot be redefined",
                        f.name
                    ),
                    f.name_span,
                );
                continue;
            }
            if f.name.contains("__") {
                self.error(
                    codes::RESERVED_IDENT,
                    format!(
                        "identifier `{}` contains `__`, which is reserved for code generation",
                        f.name
                    ),
                    f.name_span,
                );
                continue;
            }
            let canonical = self.canonical_name(&f.name);
            if self.fn_index.contains_key(&canonical) {
                self.error(
                    codes::DUPLICATE_DECL,
                    format!("function `{}` is defined more than once", f.name),
                    f.name_span,
                );
                continue;
            }
            let mut type_params = Vec::new();
            let mut tp_seen = HashSet::new();
            for tp in &f.type_params {
                if !tp_seen.insert(tp.clone()) {
                    self.error(
                        codes::INVALID_ENUM,
                        format!("duplicate type parameter `{}` in function `{}`", tp, f.name),
                        f.name_span,
                    );
                }
                type_params.push(tp.clone());
            }
            self.enum_params = Some(type_params.iter().cloned().collect());
            let ret = self.type_from_typeexpr(&f.ret, true);
            let mut params = Vec::new();
            let mut seen = HashMap::new();
            for p in &f.params {
                if seen.contains_key(&p.name) {
                    self.error(
                        codes::DUPLICATE_DECL,
                        format!("duplicate parameter name `{}`", p.name),
                        p.span,
                    );
                }
                seen.insert(p.name.clone(), ());
                params.push(self.type_from_typeexpr(&p.ty, false));
            }
            self.enum_params = None;
            let idx = self.functions.len();
            let c_name = format!("bf_{}", self.c_symbol(&f.name));
            f.c_name = c_name.clone();
            self.fn_index.insert(canonical.clone(), idx);
            self.functions.push(FnSig {
                c_name,
                type_params,
                params,
                ret,
                body: f.body.clone(),
                span: f.span,
                module: self.current_module.clone(),
                is_pub: f.is_pub,
                is_extern: false,
                system_abi: false,
            });
        }
    }

    /// Registers `extern` declarations: validated ABI, FFI-compatible
    /// signatures, direct symbol names (no Bloom mangling), no body.
    fn collect_extern_functions(&mut self, program: &mut Program) {
        for item in &mut program.items {
            let Item::Extern(block) = item else { continue };
            let system_abi = match block.abi.as_str() {
                "C" => false,
                "system" => true,
                other => {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_EXTERN,
                        format!(
                            "unknown ABI `{}`: supported are \"C\" and \"system\"",
                            other
                        ),
                        block.abi_span,
                    );
                    diag.help =
                        Some("use `extern \"C\"` for C libraries or `extern \"system\"` for the Windows API".to_string());
                    self.diags.push(diag);
                    continue;
                }
            };
            for f in &mut block.functions {
                if builtin_kind(&f.name).is_some() {
                    self.error(
                        codes::INVALID_EXTERN,
                        format!(
                            "`{}` is a built-in function and cannot be declared as external",
                            f.name
                        ),
                        f.name_span,
                    );
                    continue;
                }
                let canonical = self.canonical_name(&f.name);
                if self.fn_index.contains_key(&canonical) {
                    self.error(
                        codes::DUPLICATE_DECL,
                        format!("function `{}` is declared more than once", f.name),
                        f.name_span,
                    );
                    continue;
                }
                // Duplicate external symbol (same linker symbol, different
                // Bloom names) is rejected as well.
                if self
                    .functions
                    .iter()
                    .any(|s| s.is_extern && s.c_name == f.symbol)
                {
                    self.error(
                        codes::INVALID_EXTERN,
                        format!("external symbol `{}` is already declared", f.symbol),
                        f.name_span,
                    );
                    continue;
                }
                let mut params = Vec::new();
                let mut seen = HashMap::new();
                for p in &f.params {
                    if seen.contains_key(&p.name) {
                        self.error(
                            codes::DUPLICATE_DECL,
                            format!("duplicate parameter name `{}`", p.name),
                            p.span,
                        );
                    }
                    seen.insert(p.name.clone(), ());
                    let ty = self.type_from_typeexpr(&p.ty, false);
                    if !is_ffi_type(&ty) {
                        self.error(
                            codes::INVALID_FFI_TYPE,
                            format!(
                                "type `{}` is not supported in `extern` declarations (parameter `{}`)",
                                ty, p.name
                            ),
                            p.span,
                        );
                    }
                    params.push(ty);
                }
                let ret = self.type_from_typeexpr(&f.ret, true);
                if ret != Type::Void && !is_ffi_type(&ret) {
                    self.error(
                        codes::INVALID_FFI_TYPE,
                        format!(
                            "return type `{}` is not supported in `extern` declarations",
                            ret
                        ),
                        f.ret.span,
                    );
                }
                f.canonical = canonical.clone();
                f.is_system_abi = system_abi;
                let idx = self.functions.len();
                self.fn_index.insert(canonical, idx);
                self.functions.push(FnSig {
                    c_name: f.symbol.clone(),
                    type_params: Vec::new(),
                    params,
                    ret,
                    body: Block {
                        stmts: Vec::new(),
                        span: f.span,
                    },
                    span: f.span,
                    module: self.current_module.clone(),
                    is_pub: f.is_pub,
                    is_extern: true,
                    system_abi,
                });
            }
        }
    }

    fn type_from_typeexpr(&mut self, te: &TypeExpr, allow_void: bool) -> Type {
        match &te.kind {
            TypeExprKind::Named(name) => match name.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "bool" => Type::Bool,
                "string" => Type::String,
                "char" => Type::Char,
                "c_int" => Type::Ffi(crate::types::FfiKind::CInt),
                "c_uint" => Type::Ffi(crate::types::FfiKind::CUInt),
                "c_long" => Type::Ffi(crate::types::FfiKind::CLong),
                "c_ulong" => Type::Ffi(crate::types::FfiKind::CULong),
                "c_longlong" => Type::Ffi(crate::types::FfiKind::CLongLong),
                "c_ulonglong" => Type::Ffi(crate::types::FfiKind::CULongLong),
                "f32" => Type::Ffi(crate::types::FfiKind::F32),
                "rawptr" => Type::Ffi(crate::types::FfiKind::RawPtr),
                "void" => {
                    if allow_void {
                        Type::Void
                    } else {
                        self.error(
                            codes::UNKNOWN_TYPE,
                            "`void` is only allowed as a function return type",
                            te.span,
                        );
                        Type::Int
                    }
                }
                other => {
                    if let Some(params) = &self.enum_params {
                        if params.contains(other) {
                            return Type::Param(other.to_string());
                        }
                    }
                    let segments: Vec<String> = other.split("::").map(|s| s.to_string()).collect();
                    match self.resolve_path(&segments) {
                        PathResolution::Struct(canonical) => {
                            let (module, is_pub, params) = self
                                .structs
                                .get(&canonical)
                                .map(|s| (s.module.clone(), s.is_pub, s.params.clone()))
                                .unwrap_or_default();
                            if !self.check_symbol_visibility(
                                &canonical, &module, "struct", is_pub, te.span,
                            ) {
                                return Type::Int;
                            }
                            if params.is_empty() {
                                return Type::Struct(canonical, Vec::new());
                            }
                            let mut diag = Diagnostic::new(
                                codes::UNKNOWN_TYPE,
                                format!("struct `{}` requires type arguments", other),
                                te.span,
                            );
                            diag.help = Some(format!(
                                "write e.g. `{}<{}>`",
                                other,
                                params
                                    .iter()
                                    .map(|_| "int".to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                            self.diags.push(diag);
                            return Type::Int;
                        }
                        PathResolution::Enum(canonical) => {
                            let info = match self.enums.get(&canonical).cloned() {
                                Some(info) => info,
                                None => return Type::Int,
                            };
                            if !self.check_symbol_visibility(
                                &canonical,
                                &info.module,
                                "enum",
                                info.is_pub,
                                te.span,
                            ) {
                                return Type::Int;
                            }
                            if info.params.is_empty() {
                                return Type::Enum(canonical, Vec::new());
                            }
                            let mut diag = Diagnostic::new(
                                codes::UNKNOWN_TYPE,
                                format!("enum `{}` requires type arguments", other),
                                te.span,
                            );
                            diag.help = Some(format!(
                                "write e.g. `{}<{}>`",
                                other,
                                info.params
                                    .iter()
                                    .map(|_| "int".to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                            self.diags.push(diag);
                            return Type::Int;
                        }
                        _ => {}
                    }
                    self.error(
                        codes::UNKNOWN_TYPE,
                        format!("unknown type `{}`", other),
                        te.span,
                    );
                    if let Some(d) = self.diags.last_mut() {
                        d.help = Some(
                            "known types are: int, float, bool, string, char, arrays like [int], declared structs and enums"
                                .to_string(),
                        );
                    }
                    Type::Int
                }
            },
            TypeExprKind::Array(inner) => {
                let elem = self.type_from_typeexpr(inner, false);
                if matches!(elem, Type::Array(_)) {
                    self.error(
                        codes::UNSUPPORTED,
                        "arrays of arrays are not supported in Bloom 0.4",
                        te.span,
                    );
                }
                if matches!(elem, Type::Struct(_, _)) {
                    let mut diag = Diagnostic::new(
                        codes::UNSUPPORTED,
                        format!(
                            "arrays of structs (`[{}]`) are not supported in Bloom 0.4",
                            elem
                        ),
                        te.span,
                    );
                    diag.help =
                        Some("store the struct fields in separate arrays for now".to_string());
                    self.diags.push(diag);
                }
                if matches!(elem, Type::Enum(_, _)) {
                    let mut diag = Diagnostic::new(
                        codes::UNSUPPORTED,
                        format!(
                            "arrays of enums (`[{}]`) are not supported in Bloom 0.4",
                            elem
                        ),
                        te.span,
                    );
                    diag.help =
                        Some("store enum values in individual variables for now".to_string());
                    self.diags.push(diag);
                }
                Type::Array(Box::new(elem))
            }
            TypeExprKind::Generic(name, args) => {
                if name == "Slice" {
                    if args.len() != 1 {
                        self.error(
                            codes::UNKNOWN_TYPE,
                            format!(
                                "`Slice` expects exactly one type argument, found {}",
                                args.len()
                            ),
                            te.span,
                        );
                        return Type::Int;
                    }
                    let elem = self.type_from_typeexpr(&args[0], false);
                    let is_param = matches!(elem, Type::Param(_));
                    if !elem.is_basic() && !is_param {
                        let mut diag = Diagnostic::new(
                            codes::UNKNOWN_TYPE,
                            format!(
                                "`Slice<{}>` is not supported: slice views require a basic element type",
                                elem
                            ),
                            te.span,
                        );
                        diag.help = Some(
                            "use `int`, `float`, `bool`, `string` or `char` as element type"
                                .to_string(),
                        );
                        self.diags.push(diag);
                        return Type::Int;
                    }
                    return Type::Slice(Box::new(elem));
                }
                let mut resolved = Vec::new();
                for a in args {
                    resolved.push(self.type_from_typeexpr(a, false));
                }
                let segments: Vec<String> = name.split("::").map(|s| s.to_string()).collect();
                let canonical = match self.resolve_path(&segments) {
                    PathResolution::Enum(canonical) => canonical,
                    PathResolution::Struct(canonical) => {
                        let info = self.structs.get(&canonical).cloned();
                        let Some(info) = info else {
                            return Type::Int;
                        };
                        if !self.check_symbol_visibility(
                            &canonical,
                            &info.module,
                            "struct",
                            info.is_pub,
                            te.span,
                        ) {
                            return Type::Int;
                        }
                        if resolved.len() != info.params.len() {
                            let mut diag = Diagnostic::new(
                                codes::UNKNOWN_TYPE,
                                format!(
                                    "struct `{}` expects {} type argument{}, found {}",
                                    name,
                                    info.params.len(),
                                    if info.params.len() == 1 { "" } else { "s" },
                                    resolved.len()
                                ),
                                te.span,
                            );
                            diag.help = Some(format!(
                                "write e.g. `{}<{}>`",
                                name,
                                info.params
                                    .iter()
                                    .map(|_| "int".to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                            self.diags.push(diag);
                            return Type::Int;
                        }
                        for (a, arg_te) in resolved.iter().zip(args.iter()) {
                            if *a == Type::Void {
                                self.error(
                                    codes::UNKNOWN_TYPE,
                                    "`void` cannot be used as a type argument",
                                    arg_te.span,
                                );
                            }
                        }
                        return Type::Struct(canonical, resolved);
                    }
                    _ => {
                        self.error(
                            codes::UNKNOWN_TYPE,
                            format!("unknown type `{}`", name),
                            te.span,
                        );
                        return Type::Int;
                    }
                };
                let Some(info) = self.enums.get(&canonical).cloned() else {
                    self.error(
                        codes::UNKNOWN_TYPE,
                        format!("unknown type `{}`", name),
                        te.span,
                    );
                    return Type::Int;
                };
                if !self.check_symbol_visibility(
                    &canonical,
                    &info.module,
                    "enum",
                    info.is_pub,
                    te.span,
                ) {
                    return Type::Int;
                }
                if resolved.len() != info.params.len() {
                    let mut diag = Diagnostic::new(
                        codes::UNKNOWN_TYPE,
                        format!(
                            "enum `{}` expects {} type argument{}, found {}",
                            name,
                            info.params.len(),
                            if info.params.len() == 1 { "" } else { "s" },
                            resolved.len()
                        ),
                        te.span,
                    );
                    diag.help = Some(format!(
                        "write e.g. `{}<{}>`",
                        name,
                        info.params
                            .iter()
                            .map(|_| "int".to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    self.diags.push(diag);
                    return Type::Int;
                }
                for (a, arg_te) in resolved.iter().zip(args.iter()) {
                    if *a == Type::Void {
                        self.error(
                            codes::UNKNOWN_TYPE,
                            "`void` cannot be used as a type argument",
                            arg_te.span,
                        );
                    }
                }
                Type::Enum(canonical, resolved)
            }
        }
    }

    fn register_struct_names(&mut self, program: &mut Program) {
        for item in &mut program.items {
            let Item::Struct(s) = item else { continue };
            if is_basic_type_name(&s.name)
                || builtin_kind(&s.name).is_some()
                || s.name == "Option"
                || s.name == "Result"
            {
                let mut diag = Diagnostic::new(
                    codes::DUPLICATE_DECL,
                    format!(
                        "`{}` is a reserved name and cannot be used as a struct name",
                        s.name
                    ),
                    s.name_span,
                );
                diag.help = Some("choose a different struct name".to_string());
                self.diags.push(diag);
                continue;
            }
            if s.name.contains("__") {
                self.error(
                    codes::RESERVED_IDENT,
                    format!(
                        "identifier `{}` contains `__`, which is reserved for code generation",
                        s.name
                    ),
                    s.name_span,
                );
                continue;
            }
            let canonical = self.canonical_name(&s.name);
            if self.structs.contains_key(&canonical) {
                self.error(
                    codes::DUPLICATE_DECL,
                    format!("struct `{}` is defined more than once", s.name),
                    s.name_span,
                );
                continue;
            }
            if self.enums.contains_key(&canonical) {
                self.error(
                    codes::DUPLICATE_DECL,
                    format!("`{}` is already declared as an enum", s.name),
                    s.name_span,
                );
                continue;
            }
            let mut params = Vec::new();
            let mut seen = HashSet::new();
            for p in &s.params {
                if !seen.insert(p.clone()) {
                    self.error(
                        codes::INVALID_ENUM,
                        format!("duplicate type parameter `{}` in struct `{}`", p, s.name),
                        s.name_span,
                    );
                }
                params.push(p.clone());
            }
            s.canonical = canonical.clone();
            self.structs.insert(
                canonical,
                StructInfo {
                    params,
                    fields: Vec::new(),
                    span: s.span,
                    module: self.current_module.clone(),
                    is_pub: s.is_pub,
                },
            );
        }
    }

    fn resolve_struct_fields(&mut self, program: &Program) {
        for item in &program.items {
            let Item::Struct(s) = item else { continue };
            let exists = self
                .structs
                .get(&s.canonical)
                .map(|info| info.fields.is_empty() && info.span == s.span)
                .unwrap_or(false);
            if !exists {
                continue;
            }
            if s.fields.is_empty() {
                let mut diag = Diagnostic::new(
                    codes::UNSUPPORTED,
                    format!("struct `{}` must declare at least one field", s.name),
                    s.span,
                );
                diag.help = Some("empty structs are not supported in Bloom 0.6".to_string());
                self.diags.push(diag);
                continue;
            }
            let param_set: HashSet<String> = s.params.iter().cloned().collect();
            self.enum_params = Some(param_set);
            let mut fields = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for f in &s.fields {
                if !seen.insert(f.name.clone()) {
                    self.error(
                        codes::DUPLICATE_DECL,
                        format!("duplicate field `{}` in struct `{}`", f.name, s.name),
                        f.name_span,
                    );
                    continue;
                }
                let ty = self.type_from_typeexpr(&f.ty, false);
                fields.push(StructField {
                    name: f.name.clone(),
                    ty,
                });
            }
            self.enum_params = None;
            if let Some(info) = self.structs.get_mut(&s.canonical) {
                info.fields = fields;
            }
        }
    }

    fn check_struct_cycles(&mut self) {
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        let mut spans: HashMap<String, Span> = HashMap::new();
        let mut is_enum: HashMap<String, bool> = HashMap::new();
        for (name, s) in &self.structs {
            let mut deps = Vec::new();
            for f in &s.fields {
                collect_named_types(&f.ty, &mut deps);
            }
            graph.insert(name.clone(), deps);
            spans.insert(name.clone(), s.span);
            is_enum.insert(name.clone(), false);
        }
        for (name, e) in &self.enums {
            if name == "Option" || name == "Result" {
                continue;
            }
            let mut deps = Vec::new();
            for v in &e.variants {
                for f in &v.fields {
                    collect_named_types(f, &mut deps);
                }
            }
            graph.insert(name.clone(), deps);
            spans.insert(name.clone(), e.span);
            is_enum.insert(name.clone(), true);
        }
        let recursive = find_cycles_in_graph(&graph);
        for name in recursive {
            let span = spans
                .get(&name)
                .copied()
                .unwrap_or_else(|| Span::point(1, 1));
            let is_enum_type = *is_enum.get(&name).unwrap_or(&false);
            let (code, message) = if is_enum_type {
                (
                    codes::RECURSIVE_ENUM,
                    format!("enum `{}` contains itself as a payload", name),
                )
            } else {
                (
                    codes::RECURSIVE_STRUCT,
                    format!("struct `{}` contains itself as a field", name),
                )
            };
            let mut diag = Diagnostic::new(code, message, span);
            diag.help = Some(
                "values are stored inline; a type cannot contain itself directly or indirectly; \
                 indirection (references) is planned for a later version"
                    .to_string(),
            );
            self.diags.push(diag);
        }
    }

    fn check_global(&mut self, decl: &VarDecl) {
        let init = decl.init.as_ref().expect("global initializer");
        if matches!(init.kind, ExprKind::StructLit { .. }) {
            let mut diag = Diagnostic::new(
                codes::NOT_CONSTANT,
                "struct values cannot be used as global declarations in Bloom 0.5",
                init.span,
            );
            diag.help = Some(format!(
                "global initializers must be constant; declare `{}` inside a function instead",
                decl.name
            ));
            self.diags.push(diag);
            return;
        }
        let declared = decl.ty.as_ref().map(|t| self.type_from_typeexpr(t, false));
        match self.eval_const(init) {
            Ok(value) => {
                let ty = match &declared {
                    Some(dt) => {
                        let vt = value.ty();
                        if !self.coerce(&vt, dt, init.span, "initializer", codes::TYPE_MISMATCH) {
                            return;
                        }
                        if *dt == Type::Float && vt == Type::Int {
                            match value {
                                ConstValue::Int(i) => ConstValue::Float(i as f64),
                                other => other,
                            }
                        } else {
                            value
                        }
                    }
                    None => value,
                };
                let vt = ty.ty();
                let c_name = self.fresh_c_name(&decl.name);
                self.var_decls.insert(
                    decl.id,
                    VarInfo {
                        c_name: c_name.clone(),
                        ty: vt.clone(),
                        mutable: decl.kind == DeclKind::Var,
                        kind: decl.kind,
                    },
                );
                self.consts.insert(decl.id, ty.clone());
                let canonical = self.canonical_name(&decl.name);
                let index = self.globals.len();
                self.global_index.insert(canonical.clone(), index);
                self.globals.push(GlobalInfo {
                    c_name,
                    ty: vt,
                    value: ty,
                    module: self.current_module.clone(),
                    is_pub: true,
                });
                self.declare(&decl.name, decl.id, decl.name_span);
            }
            Err(diag) => {
                self.diags.push(diag);
            }
        }
    }

    fn check_function(&mut self, f: &Function) {
        let canonical = self.canonical_name(&f.name);
        let idx = match self.fn_index.get(&canonical) {
            Some(i) => *i,
            None => return,
        };
        self.current_fn = Some(idx);
        self.loop_depth = 0;
        self.push_scope();
        let sig = self.functions[idx].clone();
        self.current_ret = Some(sig.ret.clone());
        self.enum_params = Some(sig.type_params.iter().cloned().collect());
        for (i, p) in f.params.iter().enumerate() {
            let ty = sig.params.get(i).cloned().unwrap_or(Type::Int);
            let c_name = self.fresh_c_name(&p.name);
            self.var_decls.insert(
                p.id,
                VarInfo {
                    c_name,
                    ty,
                    mutable: false,
                    kind: DeclKind::Let,
                },
            );
            self.declare(&p.name, p.id, p.span);
        }
        self.check_block(&f.body);
        self.enum_params = None;
        if sig.ret != Type::Void && !always_returns(&f.body) {
            let mut diag = Diagnostic::new(
                codes::MISSING_RETURN,
                format!(
                    "function `{}` may reach the end of its body without returning a value of type `{}`",
                    f.name, sig.ret
                ),
                f.body.span,
            );
            diag.help = Some(format!(
                "add a `return` statement for every path, e.g. `return 0;` for `{}`",
                sig.ret
            ));
            self.diags.push(diag);
        }
        self.pop_scope();
        self.current_fn = None;
        self.current_ret = None;
    }

    fn collect_methods(&mut self, program: &mut Program) {
        for item in &mut program.items {
            let Item::Impl(b) = item else { continue };
            let target_desc = describe_type_expr(&b.target);
            let mut tp_seen = HashSet::new();
            for tp in &b.params {
                if !tp_seen.insert(tp.clone()) {
                    self.error(
                        codes::INVALID_ENUM,
                        format!("duplicate type parameter `{}` in impl block", tp),
                        b.ty_span,
                    );
                }
            }
            self.enum_params = Some(b.params.iter().cloned().collect());
            let (path_segments, target_args): (Vec<String>, Vec<Type>) = match &b.target.kind {
                TypeExprKind::Named(name) => (
                    name.split("::").map(|s| s.to_string()).collect(),
                    Vec::new(),
                ),
                TypeExprKind::Generic(name, args) => {
                    let mut resolved = Vec::new();
                    for a in args {
                        resolved.push(self.type_from_typeexpr(a, false));
                    }
                    (name.split("::").map(|s| s.to_string()).collect(), resolved)
                }
                TypeExprKind::Array(_) => {
                    self.error(
                        codes::IMPL_UNKNOWN_TYPE,
                        "impl blocks can only target named types (structs or enums)",
                        b.ty_span,
                    );
                    self.enum_params = None;
                    continue;
                }
            };
            self.enum_params = None;
            let (owner, is_enum) = if let Some(canonical) = self.resolve_struct_name(&path_segments)
            {
                (canonical, false)
            } else if let Some(canonical) = self.resolve_enum_name(&path_segments) {
                (canonical, true)
            } else {
                let mut diag = Diagnostic::new(
                    codes::IMPL_UNKNOWN_TYPE,
                    format!("impl block for unknown type `{}`", target_desc),
                    b.ty_span,
                );
                diag.help = Some(format!(
                    "declare the type first with `struct {} {{ ... }}` or `enum {} {{ ... }}`",
                    target_desc, target_desc
                ));
                self.diags.push(diag);
                continue;
            };
            if b.params.is_empty() {
                if !target_args.is_empty() {
                    let mut diag = Diagnostic::new(
                        codes::IMPL_UNKNOWN_TYPE,
                        "specialized impl blocks are not supported; use a generic impl block",
                        b.ty_span,
                    );
                    diag.help = Some(format!(
                        "write `impl<T> {}<T> {{ ... }}` instead",
                        short_of(&owner)
                    ));
                    self.diags.push(diag);
                    continue;
                }
            } else {
                let expected: Vec<Type> = b.params.iter().map(|p| Type::Param(p.clone())).collect();
                if target_args != expected {
                    let mut diag = Diagnostic::new(
                        codes::IMPL_UNKNOWN_TYPE,
                        "the impl type arguments must be exactly the impl's type parameters",
                        b.ty_span,
                    );
                    diag.help = Some(format!(
                        "write `impl<{}> {}<{}> {{ ... }}`",
                        b.params.join(", "),
                        short_of(&owner),
                        b.params.join(", ")
                    ));
                    self.diags.push(diag);
                    continue;
                }
            }
            let owner_module = if is_enum {
                self.enums.get(&owner).map(|e| e.module.clone())
            } else {
                self.structs.get(&owner).map(|s| s.module.clone())
            }
            .unwrap_or_default();
            if !owner_module.is_empty() && owner_module != self.current_module {
                let mut diag = Diagnostic::new(
                    codes::IMPL_UNKNOWN_TYPE,
                    format!(
                        "impl blocks for `{}` must be in the module where the type is declared",
                        target_desc
                    ),
                    b.ty_span,
                );
                diag.help = Some(format!(
                    "move the impl block into module `{}`",
                    display_module(&owner_module)
                ));
                self.diags.push(diag);
                continue;
            }
            b.canonical_type = owner.clone();
            for m in &mut b.methods {
                if m.name.contains("__") {
                    self.error(
                        codes::RESERVED_IDENT,
                        format!(
                            "identifier `{}` contains `__`, which is reserved for code generation",
                            m.name
                        ),
                        m.name_span,
                    );
                    continue;
                }
                let field_conflict = if is_enum {
                    self.enums
                        .get(&owner)
                        .and_then(|e| e.variant(&m.name))
                        .is_some()
                } else {
                    self.structs
                        .get(&owner)
                        .and_then(|s| s.field(&m.name))
                        .is_some()
                };
                if field_conflict {
                    let what = if is_enum { "variant" } else { "field" };
                    self.error(
                        codes::DUPLICATE_METHOD,
                        format!(
                            "method `{}` conflicts with {} `{}` of `{}`",
                            m.name,
                            what,
                            m.name,
                            short_of(&owner)
                        ),
                        m.name_span,
                    );
                    continue;
                }
                let already = self
                    .methods
                    .get(&owner)
                    .map(|ms| ms.contains_key(&m.name))
                    .unwrap_or(false);
                if already {
                    self.error(
                        codes::DUPLICATE_METHOD,
                        format!(
                            "method `{}` is defined more than once for `{}`",
                            m.name,
                            short_of(&owner)
                        ),
                        m.name_span,
                    );
                    continue;
                }
                if is_enum && m.receiver == Some(ReceiverKind::Mutable) {
                    let mut diag = Diagnostic::new(
                        codes::IMPL_UNKNOWN_TYPE,
                        format!(
                            "mutable receivers (`var self`) on enums are not supported yet (method `{}`)",
                            m.name
                        ),
                        m.name_span,
                    );
                    diag.help = Some("take `self` and return a new enum value instead".to_string());
                    self.diags.push(diag);
                    continue;
                }
                self.enum_params = Some(b.params.iter().cloned().collect());
                let ret = self.type_from_typeexpr(&m.ret, true);
                let mut params = Vec::new();
                let mut seen = HashMap::new();
                for p in &m.params {
                    if seen.contains_key(&p.name) {
                        self.error(
                            codes::DUPLICATE_DECL,
                            format!("duplicate parameter name `{}`", p.name),
                            p.span,
                        );
                    }
                    seen.insert(p.name.clone(), ());
                    params.push(self.type_from_typeexpr(&p.ty, false));
                }
                self.enum_params = None;
                let c_name = format!("bf_{}_{}", owner.replace("::", "_"), m.name);
                m.c_name = c_name.clone();
                let info = MethodInfo {
                    name: m.name.clone(),
                    owner: owner.clone(),
                    c_name,
                    receiver: m.receiver,
                    type_params: b.params.clone(),
                    params,
                    ret,
                    body: m.body.clone(),
                    is_pub: m.is_pub,
                    module: self.current_module.clone(),
                    is_enum,
                };
                self.methods
                    .entry(owner.clone())
                    .or_default()
                    .insert(m.name.clone(), info);
            }
        }
    }

    fn check_method(&mut self, b: &ImplBlock, m: &Method) {
        let info = match self
            .methods
            .get(&b.canonical_type)
            .and_then(|ms| ms.get(&m.name))
            .cloned()
        {
            Some(i) => i,
            None => return,
        };
        self.current_fn = None;
        self.current_ret = Some(info.ret.clone());
        self.loop_depth = 0;
        self.push_scope();
        self.enum_params = Some(info.type_params.iter().cloned().collect());
        let self_type = if info.is_enum {
            Type::Enum(
                info.owner.clone(),
                info.type_params
                    .iter()
                    .map(|p| Type::Param(p.clone()))
                    .collect(),
            )
        } else {
            Type::Struct(
                info.owner.clone(),
                info.type_params
                    .iter()
                    .map(|p| Type::Param(p.clone()))
                    .collect(),
            )
        };
        if let Some(receiver) = info.receiver {
            let self_c = self.fresh_c_name("self");
            let mutable = receiver == ReceiverKind::Mutable;
            self.var_decls.insert(
                m.self_id,
                VarInfo {
                    c_name: self_c,
                    ty: self_type,
                    mutable,
                    kind: if mutable {
                        DeclKind::Var
                    } else {
                        DeclKind::Let
                    },
                },
            );
            self.declare("self", m.self_id, m.name_span);
        }
        for (i, p) in m.params.iter().enumerate() {
            let ty = info.params.get(i).cloned().unwrap_or(Type::Int);
            let c_name = self.fresh_c_name(&p.name);
            self.var_decls.insert(
                p.id,
                VarInfo {
                    c_name,
                    ty,
                    mutable: false,
                    kind: DeclKind::Let,
                },
            );
            self.declare(&p.name, p.id, p.span);
        }
        self.check_block(&m.body);
        self.enum_params = None;
        if info.ret != Type::Void && !always_returns(&m.body) {
            let mut diag = Diagnostic::new(
                codes::MISSING_RETURN,
                format!(
                    "method `{}` may reach the end of its body without returning a value of type `{}`",
                    m.name, info.ret
                ),
                m.body.span,
            );
            diag.help = Some(format!(
                "add a `return` statement for every path, e.g. `return 0;` for `{}`",
                info.ret
            ));
            self.diags.push(diag);
        }
        self.pop_scope();
        self.current_ret = None;
    }

    fn check_main(&mut self) -> Option<usize> {
        let main_canonical = self.canonical_name("main");
        let Some(&idx) = self.fn_index.get(&main_canonical) else {
            let mut diag = Diagnostic::new(
                codes::MISSING_MAIN,
                "no `main` function found",
                Span::point(1, 1),
            );
            diag.help = Some(
                "every Bloom program needs `fn main() -> int { ... }` as its entry point"
                    .to_string(),
            );
            self.diags.push(diag);
            return None;
        };
        let sig = self.functions[idx].clone();
        if !sig.params.is_empty() {
            let mut diag = Diagnostic::new(
                codes::BAD_MAIN,
                "`main` must not take any parameters",
                sig.span,
            );
            diag.help = Some("write `fn main() -> int { ... }`".to_string());
            self.diags.push(diag);
        }
        if sig.ret != Type::Int {
            let mut diag = Diagnostic::new(
                codes::BAD_MAIN,
                format!("`main` must return `int`, found `{}`", sig.ret),
                sig.span,
            );
            diag.help = Some("write `fn main() -> int { ... }`".to_string());
            self.diags.push(diag);
        }
        Some(idx)
    }

    fn check_block(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        self.pop_scope();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(decl) => self.check_var_decl(decl),
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr, None);
            }
            Stmt::If {
                cond,
                then_block,
                else_branch,
                ..
            } => {
                let ct = self.check_expr(cond, None);
                self.require_value(&ct, cond.span, "condition");
                if ct != Type::Bool && ct != Type::Void {
                    let mut diag = Diagnostic::new(
                        codes::CONDITION_NOT_BOOL,
                        format!("condition must be `bool`, found `{}`", ct),
                        cond.span,
                    );
                    diag.help =
                        Some("use a comparison such as `x > 0` or a boolean variable".to_string());
                    self.diags.push(diag);
                }
                self.check_block(then_block);
                match else_branch {
                    Some(ElseBranch::If(s)) => self.check_stmt(s),
                    Some(ElseBranch::Block(b)) => self.check_block(b),
                    None => {}
                }
            }
            Stmt::While { cond, body, .. } => {
                let ct = self.check_expr(cond, None);
                self.require_value(&ct, cond.span, "condition");
                self.require_bool(&ct, cond.span);
                self.loop_depth += 1;
                self.check_block(body);
                self.loop_depth -= 1;
            }
            Stmt::For {
                decl,
                range_start,
                range_end,
                body,
                ..
            } => {
                let st = self.check_expr(range_start, Some(&Type::Int));
                self.require_value(&st, range_start.span, "range start");
                if st != Type::Int && st != Type::Void {
                    self.error(
                        codes::TYPE_MISMATCH,
                        format!("range start must be `int`, found `{}`", st),
                        range_start.span,
                    );
                }
                let et = self.check_expr(range_end, Some(&Type::Int));
                self.require_value(&et, range_end.span, "range end");
                if et != Type::Int && et != Type::Void {
                    self.error(
                        codes::TYPE_MISMATCH,
                        format!("range end must be `int`, found `{}`", et),
                        range_end.span,
                    );
                }
                self.push_scope();
                let c_name = self.fresh_c_name(&decl.name);
                self.var_decls.insert(
                    decl.id,
                    VarInfo {
                        c_name,
                        ty: Type::Int,
                        mutable: false,
                        kind: DeclKind::Let,
                    },
                );
                self.declare(&decl.name, decl.id, decl.name_span);
                self.loop_depth += 1;
                self.check_block(body);
                self.loop_depth -= 1;
                self.pop_scope();
            }
            Stmt::Return { value, span } => {
                let ret = self.current_ret.clone().unwrap_or(Type::Void);
                match value {
                    Some(expr) => {
                        if ret == Type::Void {
                            self.error(
                                codes::INVALID_RETURN,
                                "cannot return a value from a function that returns `void`",
                                *span,
                            );
                            self.check_expr(expr, None);
                        } else {
                            let vt = self.check_expr(expr, Some(&ret));
                            self.require_value(&vt, expr.span, "return value");
                            self.coerce(&vt, &ret, expr.span, "return", codes::TYPE_MISMATCH);
                        }
                    }
                    None => {
                        if ret != Type::Void {
                            let mut diag = Diagnostic::new(
                                codes::INVALID_RETURN,
                                format!("missing return value (expected `{}`)", ret),
                                *span,
                            );
                            diag.help = Some(format!("write `return <{}>;`", ret));
                            self.diags.push(diag);
                        }
                    }
                }
            }
            Stmt::Break { span } => {
                if self.loop_depth == 0 {
                    let mut diag = Diagnostic::new(
                        codes::BREAK_OUTSIDE_LOOP,
                        "`break` is only allowed inside a loop",
                        *span,
                    );
                    diag.help =
                        Some("`break` and `continue` must be inside `while` or `for`".to_string());
                    self.diags.push(diag);
                }
            }
            Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    let mut diag = Diagnostic::new(
                        codes::BREAK_OUTSIDE_LOOP,
                        "`continue` is only allowed inside a loop",
                        *span,
                    );
                    diag.help =
                        Some("`break` and `continue` must be inside `while` or `for`".to_string());
                    self.diags.push(diag);
                }
            }
            Stmt::Block(b) => self.check_block(b),
            Stmt::Unsafe(b) => {
                self.unsafe_depth += 1;
                self.check_block(b);
                self.unsafe_depth -= 1;
            }
        }
    }

    fn require_bool(&mut self, ty: &Type, span: Span) {
        if *ty != Type::Bool && *ty != Type::Void {
            let mut diag = Diagnostic::new(
                codes::CONDITION_NOT_BOOL,
                format!("condition must be `bool`, found `{}`", ty),
                span,
            );
            diag.help = Some("use a comparison such as `x > 0` or a boolean variable".to_string());
            self.diags.push(diag);
        }
    }

    fn check_var_decl(&mut self, decl: &VarDecl) {
        let init = match &decl.init {
            Some(e) => e,
            None => return,
        };
        let declared = decl.ty.as_ref().map(|t| self.type_from_typeexpr(t, false));
        let init_ty = if let Some(dt) = &declared {
            self.check_expr(init, Some(dt))
        } else {
            self.check_expr(init, None)
        };
        self.require_value(&init_ty, init.span, "initializer");
        if let Some(dt) = &declared {
            self.coerce(&init_ty, dt, init.span, "initializer", codes::TYPE_MISMATCH);
        }
        let final_ty = match &declared {
            Some(dt) => dt.clone(),
            None => init_ty.clone(),
        };
        if let Type::Array(_) = final_ty {
            if init_ty == Type::Void {
                self.error(
                    codes::TYPE_MISMATCH,
                    "cannot infer the type of this declaration",
                    decl.span,
                );
            }
        }
        if decl.kind == DeclKind::Const {
            match self.eval_const(init) {
                Ok(value) => {
                    if let Some(dt) = &declared {
                        if !self.coerce(
                            &value.ty(),
                            dt,
                            init.span,
                            "initializer",
                            codes::TYPE_MISMATCH,
                        ) {
                            return;
                        }
                    }
                    self.consts.insert(decl.id, value);
                }
                Err(diag) => {
                    self.diags.push(diag);
                }
            }
        }
        let c_name = self.fresh_c_name(&decl.name);
        self.var_decls.insert(
            decl.id,
            VarInfo {
                c_name,
                ty: final_ty,
                mutable: decl.kind == DeclKind::Var,
                kind: decl.kind,
            },
        );
        self.declare(&decl.name, decl.id, decl.name_span);
    }

    fn check_expr(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        let ty = self.check_expr_inner(expr, expected);
        self.expr_types.insert(expr.id, ty.clone());
        ty
    }

    fn check_expr_inner(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        match &expr.kind {
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::Float,
            ExprKind::Str(_) => Type::String,
            ExprKind::Char(_) => Type::Char,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Path(segments, type_arg_exprs) => {
                if !type_arg_exprs.is_empty() {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_ARGUMENT,
                        "type arguments are only allowed on function calls",
                        expr.span,
                    );
                    diag.help = Some(format!(
                        "call the function, e.g. `{}::<...>(...)`",
                        segments.join("::")
                    ));
                    self.diags.push(diag);
                    self.error_exprs.insert(expr.id);
                    return Type::Int;
                }
                self.check_path_expr(expr.id, segments, expr.span, expected)
            }
            ExprKind::Ident(name) => {
                if let Some(id) = self.lookup(name) {
                    self.ident_refs.insert(expr.id, id);
                    self.var_decls
                        .get(&id)
                        .map(|v| v.ty.clone())
                        .unwrap_or(Type::Int)
                } else {
                    let segments = vec![name.clone()];
                    match self.resolve_path(&segments) {
                        PathResolution::Global(canonical) => {
                            if let Some(&index) = self.global_index.get(&canonical) {
                                let g = self.globals[index].clone();
                                if self.check_symbol_visibility(
                                    &canonical, &g.module, "constant", g.is_pub, expr.span,
                                ) {
                                    self.global_refs.insert(expr.id, index);
                                    return g.ty;
                                }
                            }
                            self.error_exprs.insert(expr.id);
                            Type::Int
                        }
                        PathResolution::Function(canonical) => {
                            let mut diag = Diagnostic::new(
                                codes::NOT_CALLABLE,
                                format!(
                                    "`{}` is a function; functions are not values",
                                    short_of(&canonical)
                                ),
                                expr.span,
                            );
                            diag.help = Some(format!("call it as `{}(...)`", short_of(&canonical)));
                            self.diags.push(diag);
                            self.error_exprs.insert(expr.id);
                            Type::Int
                        }
                        _ => {
                            let mut diag = Diagnostic::new(
                                codes::UNDEFINED_VARIABLE,
                                format!("cannot find `{}` in this scope", name),
                                expr.span,
                            );
                            diag.help = Some(if name == "self" {
                                "`self` is only available inside methods (impl blocks)".to_string()
                            } else {
                                format!(
                                    "declare it first, e.g. `let {} = ...;`, or import it with `use`",
                                    name
                                )
                            });
                            self.diags.push(diag);
                            self.error_exprs.insert(expr.id);
                            Type::Int
                        }
                    }
                }
            }
            ExprKind::Unary { op, operand } => {
                let ot = self.check_expr(operand, None);
                self.require_value(&ot, operand.span, "operand");
                match op {
                    UnaryOp::Neg => match ot {
                        Type::Int => Type::Int,
                        Type::Float => Type::Float,
                        other => {
                            let mut diag = Diagnostic::new(
                                codes::INVALID_OPERANDS,
                                format!("unary `-` cannot be applied to `{}`", other),
                                expr.span,
                            );
                            diag.help = Some("unary `-` requires `int` or `float`".to_string());
                            self.diags.push(diag);
                            Type::Int
                        }
                    },
                    UnaryOp::Not => match ot {
                        Type::Bool => Type::Bool,
                        other => {
                            let mut diag = Diagnostic::new(
                                codes::INVALID_OPERANDS,
                                format!("`!` cannot be applied to `{}`", other),
                                expr.span,
                            );
                            diag.help = Some("`!` requires `bool`".to_string());
                            self.diags.push(diag);
                            Type::Bool
                        }
                    },
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.check_expr(lhs, None);
                let rt = self.check_expr(rhs, None);
                self.require_value(&lt, lhs.span, "operand");
                self.require_value(&rt, rhs.span, "operand");
                self.binary_result(*op, &lt, &rt, expr.span)
            }
            ExprKind::Assign { target, value } => {
                let target_ty = self.check_assignment_target(target);
                let vt = self.check_expr(value, Some(&target_ty));
                self.require_value(&vt, value.span, "assigned value");
                self.coerce(
                    &vt,
                    &target_ty,
                    value.span,
                    "assignment",
                    codes::TYPE_MISMATCH,
                );
                target_ty
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
            } => {
                if !type_args.is_empty() {
                    if let ExprKind::Field { .. } = &callee.kind {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_ARGUMENT,
                            "type arguments on method calls are not supported",
                            expr.span,
                        );
                        diag.help = Some(
                            "the type arguments are inferred from the receiver value".to_string(),
                        );
                        self.diags.push(diag);
                    }
                }
                self.check_call(expr.id, callee, args, expr.span, expected)
            }
            ExprKind::Try(operand) => self.check_try(expr.id, operand, expr.span),
            ExprKind::Index { base, index } => {
                let bt = self.check_expr(base, None);
                let it = self.check_expr(index, None);
                self.require_value(&it, index.span, "index");
                if it != Type::Int && it != Type::Void {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_INDEX,
                        format!("array index must be `int`, found `{}`", it),
                        index.span,
                    );
                    diag.help = Some("use an integer index such as `a[0]`".to_string());
                    self.diags.push(diag);
                }
                match &bt {
                    Type::Array(elem) => (**elem).clone(),
                    Type::Slice(elem) => (**elem).clone(),
                    Type::String => Type::Char,
                    Type::Void => {
                        self.error_exprs.insert(expr.id);
                        Type::Int
                    }
                    other => {
                        if !self.error_exprs.contains(&base.id) {
                            let mut diag = Diagnostic::new(
                                codes::INVALID_INDEX,
                                format!("cannot index a value of type `{}`", other),
                                base.span,
                            );
                            diag.help = Some(
                                "only arrays (`[T]`), slices (`Slice<T>`) and `string` values can be indexed"
                                    .to_string(),
                            );
                            self.diags.push(diag);
                        }
                        self.error_exprs.insert(expr.id);
                        Type::Int
                    }
                }
            }
            ExprKind::Field {
                base,
                name,
                name_span,
            } => {
                let bt = self.check_expr(base, None);
                self.require_value(&bt, base.span, "field access base");
                if self.error_exprs.contains(&base.id) {
                    self.error_exprs.insert(expr.id);
                    return Type::Int;
                }
                match &bt {
                    Type::Struct(sname, sargs) => {
                        let def = self.structs.get(sname).cloned();
                        match def {
                            Some(def) if !def.fields.is_empty() => match def.field(name) {
                                Some(f) => {
                                    let bindings: HashMap<String, Type> = def
                                        .params
                                        .iter()
                                        .cloned()
                                        .zip(sargs.iter().cloned())
                                        .collect();
                                    substitute_type(&f.ty, &bindings)
                                }
                                None => {
                                    let mut diag = Diagnostic::new(
                                        codes::UNKNOWN_FIELD,
                                        format!(
                                            "struct `{}` has no field `{}`",
                                            short_of(sname),
                                            name
                                        ),
                                        *name_span,
                                    );
                                    diag.help = Some(format!(
                                        "`{}` has the fields: {}",
                                        short_of(sname),
                                        def.field_names()
                                    ));
                                    self.diags.push(diag);
                                    self.error_exprs.insert(expr.id);
                                    Type::Int
                                }
                            },
                            _ => {
                                self.error_exprs.insert(expr.id);
                                Type::Int
                            }
                        }
                    }
                    Type::Void => {
                        self.error_exprs.insert(expr.id);
                        Type::Int
                    }
                    other => {
                        let mut diag = Diagnostic::new(
                            codes::NOT_A_STRUCT,
                            format!("type `{}` has no fields", other),
                            *name_span,
                        );
                        diag.help = Some(format!(
                            "field access (`.{}`) is only defined for struct values",
                            name
                        ));
                        self.diags.push(diag);
                        self.error_exprs.insert(expr.id);
                        Type::Int
                    }
                }
            }
            ExprKind::StructLit {
                name,
                name_span,
                fields,
            } => self.check_struct_lit(expr.id, name, *name_span, fields, expected),
            ExprKind::Match { scrutinee, arms } => {
                self.check_match(expr.id, scrutinee, arms, expected)
            }
            ExprKind::ArrayLit(elems) => self.check_array_lit(expr.id, elems, expr.span, expected),
        }
    }

    fn check_struct_lit(
        &mut self,
        id: ExprId,
        name: &str,
        name_span: Span,
        fields: &[StructLitField],
        expected: Option<&Type>,
    ) -> Type {
        let parts: Vec<String> = name.split("::").map(|s| s.to_string()).collect();
        match self.resolve_path(&parts) {
            PathResolution::Struct(struct_name) => {
                return self.check_struct_lit_named(id, &struct_name, name_span, fields, expected);
            }
            PathResolution::EnumVariant(enum_name, variant_name) => {
                return self.check_variant_named(
                    id,
                    &enum_name,
                    &variant_name,
                    fields,
                    name_span,
                    expected,
                );
            }
            PathResolution::Enum(enum_name) => {
                let mut diag = Diagnostic::new(
                    codes::UNKNOWN_VARIANT,
                    format!(
                        "`{}` is an enum; construct it as `{}::Variant {{ ... }}`",
                        short_of(&enum_name),
                        name
                    ),
                    name_span,
                );
                diag.help = Some(format!(
                    "available variants of `{}`: {}",
                    short_of(&enum_name),
                    self.enums[&enum_name].variant_names()
                ));
                self.diags.push(diag);
                for f in fields {
                    self.check_expr(&f.value, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
            _ => {}
        }
        let mut diag = Diagnostic::new(
            codes::UNKNOWN_TYPE,
            format!("unknown struct type `{}`", name),
            name_span,
        );
        diag.help = Some(format!("declare it with `struct {} {{ ... }}`", name));
        self.diags.push(diag);
        for f in fields {
            self.check_expr(&f.value, None);
        }
        self.error_exprs.insert(id);
        Type::Int
    }

    fn check_struct_lit_named(
        &mut self,
        id: ExprId,
        name: &str,
        name_span: Span,
        fields: &[StructLitField],
        expected: Option<&Type>,
    ) -> Type {
        let def = self.structs.get(name).cloned();
        let def = match def {
            Some(def) if !def.fields.is_empty() => def,
            Some(_) => {
                for f in fields {
                    self.check_expr(&f.value, None);
                }
                self.error_exprs.insert(id);
                return Type::Struct(name.to_string(), Vec::new());
            }
            None => {
                let mut diag = Diagnostic::new(
                    codes::UNKNOWN_TYPE,
                    format!("unknown struct type `{}`", name),
                    name_span,
                );
                diag.help = Some(format!("declare it with `struct {} {{ ... }}`", name));
                self.diags.push(diag);
                for f in fields {
                    self.check_expr(&f.value, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        let mut seen: HashMap<String, Span> = HashMap::new();
        let mut had_error = false;
        let mut constraints: Vec<(String, Type)> = Vec::new();
        let mut checked_values: Vec<(usize, Type, Span)> = Vec::new();
        let partial = Self::partial_bindings_for(name, &def.params, expected);
        for f in fields {
            if let Some(prev) = seen.get(&f.name) {
                let mut diag = Diagnostic::new(
                    codes::DUPLICATE_FIELD,
                    format!(
                        "field `{}` is initialized more than once in `{}`",
                        f.name, name
                    ),
                    f.name_span,
                );
                diag.label = Some(format!("duplicate field `{}`", f.name));
                diag.notes.push(format!(
                    "the first initialization is at line {}",
                    prev.start.line
                ));
                diag.help = Some(format!("remove one of the `{}:` entries", f.name));
                self.diags.push(diag);
                self.check_expr(&f.value, None);
                had_error = true;
                continue;
            }
            seen.insert(f.name.clone(), f.name_span);
            match def.field(&f.name) {
                Some(field) => {
                    let substituted = substitute_type(&field.ty, &partial);
                    let expected_value = if substituted.contains_param() {
                        None
                    } else {
                        Some(substituted.clone())
                    };
                    let vt = self.check_expr(&f.value, expected_value.as_ref());
                    self.require_value(&vt, f.value.span, "field value");
                    Self::collect_constraints(&field.ty, &vt, &mut constraints);
                    let idx = def
                        .fields
                        .iter()
                        .position(|d| d.name == f.name)
                        .unwrap_or(0);
                    checked_values.push((idx, vt, f.value.span));
                }
                None => {
                    let mut diag = Diagnostic::new(
                        codes::UNKNOWN_FIELD,
                        format!("struct `{}` has no field `{}`", name, f.name),
                        f.name_span,
                    );
                    diag.help = Some(format!("`{}` has the fields: {}", name, def.field_names()));
                    self.diags.push(diag);
                    self.check_expr(&f.value, None);
                    had_error = true;
                }
            }
        }
        for field in &def.fields {
            if !seen.contains_key(&field.name) {
                let mut diag = Diagnostic::new(
                    codes::MISSING_FIELD,
                    format!("missing field `{}` in `{}` value", field.name, name),
                    name_span,
                );
                diag.label = Some(format!(
                    "`{}` requires the field `{}` (type `{}`)",
                    name, field.name, field.ty
                ));
                diag.help = Some(format!(
                    "add `{}: <value>` to the struct literal",
                    field.name
                ));
                self.diags.push(diag);
                had_error = true;
            }
        }
        let type_args = if def.params.is_empty() {
            Vec::new()
        } else {
            match self.resolve_type_args(
                &format!("struct `{}`", short_of(name)),
                name,
                &def.params,
                expected,
                &constraints,
                name_span,
            ) {
                Some(args) => args,
                None => {
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
            }
        };
        let bindings: HashMap<String, Type> = def
            .params
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();
        for (idx, vt, span) in checked_values {
            if let Some(field) = def.fields.get(idx) {
                let concrete = substitute_type(&field.ty, &bindings);
                self.coerce(&vt, &concrete, span, "field", codes::TYPE_MISMATCH);
            }
        }
        if had_error {
            self.error_exprs.insert(id);
        }
        Type::Struct(name.to_string(), type_args)
    }

    fn check_array_lit(
        &mut self,
        id: ExprId,
        elems: &[Expr],
        span: Span,
        expected: Option<&Type>,
    ) -> Type {
        let expected_elem = match expected {
            Some(Type::Array(e)) => Some((**e).clone()),
            _ => None,
        };
        if elems.is_empty() {
            if let Some(elem) = expected_elem {
                if matches!(elem, Type::Array(_)) {
                    self.error(
                        codes::UNSUPPORTED,
                        "arrays of arrays are not supported in Bloom 0.4",
                        span,
                    );
                }
                return Type::Array(Box::new(elem));
            }
            let mut diag = Diagnostic::new(
                codes::TYPE_MISMATCH,
                "cannot infer the element type of an empty array literal",
                span,
            );
            diag.help = Some("add a type annotation, e.g. `let xs: [int] = [];`".to_string());
            self.diags.push(diag);
            return Type::Array(Box::new(Type::Int));
        }
        let mut elem_ty: Option<Type> = expected_elem.clone();
        for elem in elems {
            let et = if let Some(target) = &elem_ty {
                self.check_expr(elem, Some(target))
            } else {
                self.check_expr(elem, None)
            };
            self.require_value(&et, elem.span, "array element");
            match &elem_ty {
                None => {
                    elem_ty = Some(et);
                }
                Some(target) => {
                    let target = target.clone();
                    let merged = if et == target {
                        target
                    } else if et.is_numeric() && target.is_numeric() {
                        Type::Float
                    } else {
                        let start = match elems.first() {
                            Some(first) => first.span,
                            None => span,
                        };
                        let mut diag = Diagnostic::new(
                            codes::TYPE_MISMATCH,
                            format!(
                                "all elements of an array must have the same type; previous elements are `{}`, found `{}`",
                                target, et
                            ),
                            elem.span,
                        );
                        diag.label = Some(format!("expected `{}`, found `{}`", target, et));
                        diag.notes
                            .push(format!("the first element is at line {}", start.start.line));
                        self.diags.push(diag);
                        target
                    };
                    elem_ty = Some(merged);
                }
            }
        }
        let elem = elem_ty.unwrap_or(Type::Int);
        if matches!(elem, Type::Array(_)) {
            self.error(
                codes::UNSUPPORTED,
                "arrays of arrays are not supported in Bloom 0.4",
                span,
            );
        }
        if matches!(elem, Type::Struct(_, _)) {
            let mut diag = Diagnostic::new(
                codes::UNSUPPORTED,
                format!(
                    "arrays of structs (`[{}]`) are not supported in Bloom 0.4",
                    elem
                ),
                span,
            );
            diag.help = Some("store the struct fields in separate arrays for now".to_string());
            self.diags.push(diag);
        }
        if matches!(elem, Type::Enum(_, _)) {
            let mut diag = Diagnostic::new(
                codes::UNSUPPORTED,
                format!(
                    "arrays of enums (`[{}]`) are not supported in Bloom 0.4",
                    elem
                ),
                span,
            );
            diag.help = Some("store enum values in individual variables for now".to_string());
            self.diags.push(diag);
        }
        let _ = id;
        Type::Array(Box::new(elem))
    }

    fn check_assignment_target(&mut self, target: &Expr) -> Type {
        match &target.kind {
            ExprKind::Ident(name) => {
                if let Some(id) = self.lookup(name) {
                    self.ident_refs.insert(target.id, id);
                    let info = self.var_decls.get(&id).cloned();
                    match info {
                        Some(info) => {
                            if !info.mutable {
                                let word = match info.kind {
                                    DeclKind::Let => "`let`",
                                    DeclKind::Var => "`var`",
                                    DeclKind::Const => "`const`",
                                };
                                let mut diag = Diagnostic::new(
                                    codes::IMMUTABLE,
                                    format!(
                                        "cannot assign to `{}`: it was declared with {} and is immutable",
                                        name, word
                                    ),
                                    target.span,
                                );
                                diag.help = Some(format!(
                                    "declare it with `var {} = ...;` if it must change",
                                    name
                                ));
                                self.diags.push(diag);
                            }
                            self.expr_types.insert(target.id, info.ty.clone());
                            info.ty
                        }
                        None => Type::Int,
                    }
                } else {
                    let mut diag = Diagnostic::new(
                        codes::UNDEFINED_VARIABLE,
                        format!("cannot find `{}` in this scope", name),
                        target.span,
                    );
                    diag.help = Some(format!("declare it first, e.g. `var {} = ...;`", name));
                    self.diags.push(diag);
                    Type::Int
                }
            }
            ExprKind::Index { base, index } => {
                let bt = self.check_expr(base, None);
                let it = self.check_expr(index, None);
                if it != Type::Int && it != Type::Void {
                    self.error(
                        codes::INVALID_INDEX,
                        format!("array index must be `int`, found `{}`", it),
                        index.span,
                    );
                }
                match bt {
                    Type::Array(elem) => {
                        let root = self.root_ident(target);
                        if let Some(root_expr) = root {
                            if let ExprKind::Ident(name) = &root_expr.kind {
                                if let Some(id) = self.lookup(name) {
                                    if let Some(info) = self.var_decls.get(&id) {
                                        if !info.mutable {
                                            let mut diag = Diagnostic::new(
                                                codes::IMMUTABLE,
                                                format!(
                                                    "cannot modify elements of `{}`: it was declared immutable",
                                                    name
                                                ),
                                                target.span,
                                            );
                                            diag.help = Some(if name == "self" {
                                                "use `var self` as the receiver to allow element assignment"
                                                    .to_string()
                                            } else {
                                                format!(
                                                    "declare it with `var {} = ...;` to allow element assignment",
                                                    name
                                                )
                                            });
                                            self.diags.push(diag);
                                        }
                                    }
                                }
                            }
                        } else {
                            self.error(
                                codes::INVALID_LVALUE,
                                "cannot assign to an element of a temporary value",
                                target.span,
                            );
                            self.expr_types.insert(target.id, (*elem).clone());
                        }
                        (*elem).clone()
                    }
                    Type::Slice(elem) => {
                        let root = self.root_ident(target);
                        if let Some(root_expr) = root {
                            if let ExprKind::Ident(name) = &root_expr.kind {
                                if let Some(id) = self.lookup(name) {
                                    if let Some(info) = self.var_decls.get(&id) {
                                        if !info.mutable {
                                            let mut diag = Diagnostic::new(
                                                codes::IMMUTABLE,
                                                format!(
                                                    "cannot write through slice view `{}`: it was declared immutable",
                                                    name
                                                ),
                                                target.span,
                                            );
                                            diag.help = Some(format!(
                                                "declare it with `var {} = slice_view(...);` to allow element assignment",
                                                name
                                            ));
                                            self.diags.push(diag);
                                        }
                                    }
                                }
                            }
                        } else {
                            self.error(
                                codes::INVALID_LVALUE,
                                "cannot assign to an element of a temporary slice view",
                                target.span,
                            );
                            self.expr_types.insert(target.id, (*elem).clone());
                        }
                        (*elem).clone()
                    }
                    Type::String => {
                        self.error(
                            codes::IMMUTABLE,
                            "strings are immutable: their characters cannot be assigned",
                            target.span,
                        );
                        Type::Char
                    }
                    Type::Void => {
                        self.error_exprs.insert(target.id);
                        Type::Int
                    }
                    other => {
                        if !self.error_exprs.contains(&base.id) {
                            self.error(
                                codes::INVALID_INDEX,
                                format!("cannot index a value of type `{}`", other),
                                base.span,
                            );
                        }
                        self.error_exprs.insert(target.id);
                        Type::Int
                    }
                }
            }
            ExprKind::Field {
                base,
                name,
                name_span,
            } => {
                let bt = self.check_expr(base, None);
                if self.error_exprs.contains(&base.id) || bt == Type::Void {
                    self.expr_types.insert(target.id, Type::Int);
                    self.error_exprs.insert(target.id);
                    return Type::Int;
                }
                let field_ty = match &bt {
                    Type::Struct(sname, sargs) => {
                        let def = self.structs.get(sname).cloned();
                        match def {
                            Some(def) if !def.fields.is_empty() => match def.field(name) {
                                Some(f) => {
                                    let bindings: HashMap<String, Type> = def
                                        .params
                                        .iter()
                                        .cloned()
                                        .zip(sargs.iter().cloned())
                                        .collect();
                                    substitute_type(&f.ty, &bindings)
                                }
                                None => {
                                    let mut diag = Diagnostic::new(
                                        codes::UNKNOWN_FIELD,
                                        format!(
                                            "struct `{}` has no field `{}`",
                                            short_of(sname),
                                            name
                                        ),
                                        *name_span,
                                    );
                                    diag.help = Some(format!(
                                        "`{}` has the fields: {}",
                                        short_of(sname),
                                        def.field_names()
                                    ));
                                    self.diags.push(diag);
                                    self.error_exprs.insert(target.id);
                                    Type::Int
                                }
                            },
                            _ => {
                                self.error_exprs.insert(target.id);
                                Type::Int
                            }
                        }
                    }
                    other => {
                        let mut diag = Diagnostic::new(
                            codes::NOT_A_STRUCT,
                            format!("type `{}` has no fields", other),
                            *name_span,
                        );
                        diag.help = Some(format!(
                            "field access (`.{}`) is only defined for struct values",
                            name
                        ));
                        self.diags.push(diag);
                        self.error_exprs.insert(target.id);
                        Type::Int
                    }
                };
                match self.root_ident(target) {
                    Some(root_expr) => {
                        if let ExprKind::Ident(root_name) = &root_expr.kind {
                            if let Some(id) = self.lookup(root_name) {
                                if let Some(info) = self.var_decls.get(&id) {
                                    if !info.mutable {
                                        let mut diag = Diagnostic::new(
                                            codes::IMMUTABLE,
                                            format!(
                                                "cannot modify field `{}` of `{}`: it was declared immutable",
                                                name, root_name
                                            ),
                                            target.span,
                                        );
                                        diag.help = Some(if root_name == "self" {
                                            "use `var self` as the receiver to allow field assignment"
                                                .to_string()
                                        } else {
                                            format!(
                                                "declare it with `var {} = ...;` to allow field assignment",
                                                root_name
                                            )
                                        });
                                        self.diags.push(diag);
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        self.error(
                            codes::INVALID_LVALUE,
                            "cannot assign to a field of a temporary value",
                            target.span,
                        );
                    }
                }
                self.expr_types.insert(target.id, field_ty.clone());
                field_ty
            }
            other => {
                let mut diag = Diagnostic::new(
                    codes::INVALID_LVALUE,
                    "invalid assignment target",
                    target.span,
                );
                diag.help = Some(
                    "only variables, struct fields and array elements can be assigned".to_string(),
                );
                self.diags.push(diag);
                match other {
                    ExprKind::Call { .. } => Type::Int,
                    _ => Type::Int,
                }
            }
        }
    }

    fn root_ident<'a>(&self, expr: &'a Expr) -> Option<&'a Expr> {
        match &expr.kind {
            ExprKind::Ident(_) => Some(expr),
            ExprKind::Index { base, .. } => self.root_ident(base),
            ExprKind::Field { base, .. } => self.root_ident(base),
            _ => None,
        }
    }

    fn binary_result(&mut self, op: BinOp, lt: &Type, rt: &Type, span: Span) -> Type {
        use BinOp::*;
        let numeric = lt.is_numeric() && rt.is_numeric();
        let promoted = if numeric {
            if *lt == Type::Float || *rt == Type::Float {
                Type::Float
            } else {
                Type::Int
            }
        } else {
            Type::Void
        };
        match op {
            Add => {
                if numeric {
                    return promoted;
                }
                if *lt == Type::String && *rt == Type::String {
                    return Type::String;
                }
                self.op_error(op, lt, rt, span, "`+` requires two numbers or two strings");
                Type::Int
            }
            Sub | Mul => {
                if numeric {
                    return promoted;
                }
                self.op_error(
                    op,
                    lt,
                    rt,
                    span,
                    "this operator requires numeric operands (`int` or `float`)",
                );
                Type::Int
            }
            Div => {
                if numeric {
                    return promoted;
                }
                self.op_error(
                    op,
                    lt,
                    rt,
                    span,
                    "`/` requires numeric operands (`int` or `float`)",
                );
                Type::Int
            }
            Mod => {
                if *lt == Type::Int && *rt == Type::Int {
                    return Type::Int;
                }
                let mut diag = Diagnostic::new(
                    codes::INVALID_OPERANDS,
                    format!(
                        "`%` requires two `int` operands, found `{}` and `{}`",
                        lt, rt
                    ),
                    span,
                );
                diag.help = Some("modulo for `float` is not supported in Bloom 0.4".to_string());
                self.diags.push(diag);
                Type::Int
            }
            Eq | Ne => {
                if matches!(lt, Type::Struct(_, _)) || matches!(rt, Type::Struct(_, _)) {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_OPERANDS,
                        format!(
                            "operator `{}` cannot be applied to `{}` and `{}`",
                            op.symbol(),
                            lt,
                            rt
                        ),
                        span,
                    );
                    diag.help = Some(
                        "struct values cannot be compared directly; compare the fields individually"
                            .to_string(),
                    );
                    self.diags.push(diag);
                    return Type::Bool;
                }
                if matches!(lt, Type::Enum(_, _)) || matches!(rt, Type::Enum(_, _)) {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_OPERANDS,
                        format!(
                            "operator `{}` cannot be applied to `{}` and `{}`",
                            op.symbol(),
                            lt,
                            rt
                        ),
                        span,
                    );
                    diag.help = Some(
                        "enum values cannot be compared directly; use `match` instead".to_string(),
                    );
                    self.diags.push(diag);
                    return Type::Bool;
                }
                if lt == rt || numeric || (*lt == Type::Void && *rt == Type::Void) {
                    return Type::Bool;
                }
                self.op_error(
                    op,
                    lt,
                    rt,
                    span,
                    "comparison requires two values of the same type",
                );
                Type::Bool
            }
            Lt | Le | Gt | Ge => {
                if numeric || (*lt == Type::Char && *rt == Type::Char) {
                    return Type::Bool;
                }
                if *lt == Type::String && *rt == Type::String {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_OPERANDS,
                        "ordering comparisons for `string` are not supported in Bloom 0.4",
                        span,
                    );
                    diag.help = Some(
                        "only `==` and `!=` are supported for strings; string ordering is planned"
                            .to_string(),
                    );
                    self.diags.push(diag);
                    return Type::Bool;
                }
                self.op_error(
                    op,
                    lt,
                    rt,
                    span,
                    "ordering comparisons require numbers or two `char` values",
                );
                Type::Bool
            }
            And | Or => {
                if *lt == Type::Bool && *rt == Type::Bool {
                    return Type::Bool;
                }
                self.op_error(
                    op,
                    lt,
                    rt,
                    span,
                    "logical operators require two `bool` operands",
                );
                Type::Bool
            }
        }
    }

    fn op_error(&mut self, op: BinOp, lt: &Type, rt: &Type, span: Span, reason: &str) {
        let mut diag = Diagnostic::new(
            codes::INVALID_OPERANDS,
            format!(
                "operator `{}` cannot be applied to `{}` and `{}`",
                op.symbol(),
                lt,
                rt
            ),
            span,
        );
        diag.help = Some(reason.to_string());
        self.diags.push(diag);
    }

    fn check_call(
        &mut self,
        id: ExprId,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        expected: Option<&Type>,
    ) -> Type {
        if let ExprKind::Field {
            base,
            name,
            name_span,
        } = &callee.kind
        {
            return self.check_method_call(id, base, name, *name_span, args, span);
        }
        if let ExprKind::Path(segments, type_arg_exprs) = &callee.kind {
            let mut explicit_type_args: Vec<Type> = Vec::new();
            for te in type_arg_exprs {
                explicit_type_args.push(self.type_from_typeexpr(te, false));
            }
            match self.resolve_path(segments) {
                PathResolution::EnumVariant(enum_name, variant) => {
                    if !explicit_type_args.is_empty() {
                        self.error(
                            codes::INVALID_ARGUMENT,
                            "type arguments are not allowed on enum variant constructors",
                            callee.span,
                        );
                    }
                    return self
                        .check_variant_tuple(id, &enum_name, &variant, args, span, expected);
                }
                PathResolution::Function(canonical) => {
                    return self.check_user_function_call(
                        id,
                        &canonical,
                        &explicit_type_args,
                        args,
                        span,
                        expected,
                    );
                }
                PathResolution::AssociatedMethod(owner, method) => {
                    return self.check_associated_call(
                        id,
                        &owner,
                        &method,
                        &explicit_type_args,
                        args,
                        span,
                        expected,
                        callee.span,
                    );
                }
                PathResolution::Struct(canonical) => {
                    let mut diag = Diagnostic::new(
                        codes::NOT_CALLABLE,
                        format!(
                            "`{}` is a struct; construct it with `{} {{ ... }}`",
                            short_of(&canonical),
                            segments.join("::")
                        ),
                        callee.span,
                    );
                    diag.help =
                        Some("structs are created with braces, not parentheses".to_string());
                    self.diags.push(diag);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
                PathResolution::Enum(canonical) => {
                    let mut diag = Diagnostic::new(
                        codes::NOT_CALLABLE,
                        format!(
                            "`{}` is an enum; construct one of its variants",
                            short_of(&canonical)
                        ),
                        callee.span,
                    );
                    diag.help = Some(format!(
                        "available variants of `{}`: {}",
                        short_of(&canonical),
                        self.enums[&canonical].variant_names()
                    ));
                    self.diags.push(diag);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
                PathResolution::Global(canonical) => {
                    let mut diag = Diagnostic::new(
                        codes::NOT_CALLABLE,
                        format!("`{}` is a constant, not a function", short_of(&canonical)),
                        callee.span,
                    );
                    diag.help = Some("constants cannot be called".to_string());
                    self.diags.push(diag);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
                PathResolution::Unknown => {
                    let mut diag = Diagnostic::new(
                        codes::UNDEFINED_FUNCTION,
                        format!("cannot find function `{}`", segments.join("::")),
                        callee.span,
                    );
                    diag.help =
                        Some("check the spelling or import the symbol with `use`".to_string());
                    self.diags.push(diag);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
            }
        }
        let name = match &callee.kind {
            ExprKind::Ident(n) => n.clone(),
            _ => {
                let mut diag = Diagnostic::new(
                    codes::NOT_CALLABLE,
                    "only named functions and methods can be called",
                    callee.span,
                );
                diag.help = Some(
                    "calling function values is not supported in Bloom; use `value.method(...)` for methods"
                        .to_string(),
                );
                self.diags.push(diag);
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        if let Some(kind) = builtin_kind(&name) {
            self.callee_refs.insert(id, Callee::Builtin(kind));
            return self.check_builtin(kind, &name, args, span);
        }
        let canonical = self.canonical_name(&name);
        let local_fn = self.fn_index.contains_key(&canonical);
        let imported_fn = matches!(
            self.imports.get(&name),
            Some(ImportedItem::Symbol(c)) if self.fn_index.contains_key(c)
        );
        if local_fn || imported_fn {
            let target = if local_fn {
                canonical
            } else {
                match self.imports.get(&name) {
                    Some(ImportedItem::Symbol(c)) => c.clone(),
                    _ => canonical,
                }
            };
            return self.check_user_function_call(id, &target, &[], args, span, expected);
        }
        if self.lookup(&name).is_some() {
            let mut diag = Diagnostic::new(
                codes::NOT_CALLABLE,
                format!("`{}` is not a function", name),
                callee.span,
            );
            diag.help = Some("only functions declared with `fn` can be called".to_string());
            self.diags.push(diag);
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let mut diag = Diagnostic::new(
            codes::UNDEFINED_FUNCTION,
            format!("cannot find function `{}`", name),
            callee.span,
        );
        diag.help = Some(format!(
            "declare it with `fn {}(...) -> ... {{ ... }}` or check the spelling",
            name
        ));
        self.diags.push(diag);
        for arg in args {
            self.check_expr(arg, None);
        }
        self.error_exprs.insert(id);
        Type::Int
    }

    fn check_user_function_call(
        &mut self,
        id: ExprId,
        canonical: &str,
        explicit_type_args: &[Type],
        args: &[Expr],
        span: Span,
        expected: Option<&Type>,
    ) -> Type {
        let Some(&idx) = self.fn_index.get(canonical) else {
            return Type::Int;
        };
        let sig = self.functions[idx].clone();
        if sig.is_extern && self.unsafe_depth == 0 {
            let mut diag = Diagnostic::new(
                codes::UNSAFE_REQUIRED,
                format!(
                    "call to external function `{}` requires an `unsafe` block",
                    sig.c_name
                ),
                span,
            );
            diag.help = Some(format!(
                "wrap the call in `unsafe {{ ... }}` (external symbol `{}`)",
                sig.c_name
            ));
            self.diags.push(diag);
        }
        if !self.check_symbol_visibility(canonical, &sig.module, "function", sig.is_pub, span) {
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let display_name = short_of(canonical);
        if !explicit_type_args.is_empty() && explicit_type_args.len() != sig.type_params.len() {
            let mut diag = Diagnostic::new(
                codes::CANNOT_INFER_TYPE_ARGS,
                format!(
                    "function `{}` expects {} type argument{}, found {}",
                    display_name,
                    sig.type_params.len(),
                    if sig.type_params.len() == 1 { "" } else { "s" },
                    explicit_type_args.len()
                ),
                span,
            );
            diag.help = Some(format!(
                "write `{}::<{}>(...)`",
                display_name,
                sig.type_params.join(", ")
            ));
            self.diags.push(diag);
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        if args.len() != sig.params.len() {
            let mut diag = Diagnostic::new(
                codes::ARG_COUNT,
                format!(
                    "function `{}` expects {} argument{}, found {}",
                    display_name,
                    sig.params.len(),
                    if sig.params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                span,
            );
            diag.label = Some(format!(
                "expected {} argument{}, found {}",
                sig.params.len(),
                if sig.params.len() == 1 { "" } else { "s" },
                args.len()
            ));
            diag.help = Some(format!(
                "call it as `{}({})`",
                display_name,
                sig.params
                    .iter()
                    .map(|p| format!("<{}>", p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            self.diags.push(diag);
        }
        let mut constraints: Vec<(String, Type)> = Vec::new();
        let mut arg_types: Vec<Type> = Vec::new();
        if let Some(exp) = expected {
            Self::collect_constraints(&sig.ret, exp, &mut constraints);
        }
        if sig.type_params.is_empty() {
            for (i, arg) in args.iter().enumerate() {
                let expected_arg = sig.params.get(i).cloned();
                let at = match &expected_arg {
                    Some(t) => self.check_expr(arg, Some(t)),
                    None => self.check_expr(arg, None),
                };
                self.require_value(&at, arg.span, "argument");
                if let Some(target) = &expected_arg {
                    self.coerce(&at, target, arg.span, "argument", codes::TYPE_MISMATCH);
                }
                arg_types.push(at);
            }
            let ret = sig.ret.clone();
            let c_name = sig.c_name.clone();
            self.callee_refs.insert(
                id,
                Callee::User(FunctionCallInfo {
                    index: idx,
                    type_args: Vec::new(),
                    base_c_name: c_name,
                    params: sig.params.clone(),
                }),
            );
            return ret;
        }
        let partial: HashMap<String, Type> = if explicit_type_args.is_empty() {
            HashMap::new()
        } else {
            sig.type_params
                .iter()
                .cloned()
                .zip(explicit_type_args.iter().cloned())
                .collect()
        };
        for (i, arg) in args.iter().enumerate() {
            let param_ty = sig.params.get(i).cloned().unwrap_or(Type::Int);
            let substituted = substitute_type(&param_ty, &partial);
            let expected_arg = if substituted.contains_param() {
                None
            } else {
                Some(substituted)
            };
            let at = self.check_expr(arg, expected_arg.as_ref());
            self.require_value(&at, arg.span, "argument");
            Self::collect_constraints(&param_ty, &at, &mut constraints);
            arg_types.push(at);
        }
        let type_args = if explicit_type_args.is_empty() {
            match self.resolve_type_args(
                &format!("function `{}`", display_name),
                canonical,
                &sig.type_params,
                None,
                &constraints,
                span,
            ) {
                Some(a) => a,
                None => {
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
            }
        } else {
            explicit_type_args.to_vec()
        };
        let bindings: HashMap<String, Type> = sig
            .type_params
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();
        for (i, arg) in args.iter().enumerate() {
            if let Some(param_ty) = sig.params.get(i) {
                let concrete = substitute_type(param_ty, &bindings);
                if let Some(at) = arg_types.get(i) {
                    self.coerce(at, &concrete, arg.span, "argument", codes::TYPE_MISMATCH);
                }
            }
        }
        let concrete_params: Vec<Type> = sig
            .params
            .iter()
            .map(|p| substitute_type(p, &bindings))
            .collect();
        let concrete_ret = substitute_type(&sig.ret, &bindings);
        self.callee_refs.insert(
            id,
            Callee::User(FunctionCallInfo {
                index: idx,
                type_args,
                base_c_name: sig.c_name.clone(),
                params: concrete_params,
            }),
        );
        concrete_ret
    }

    fn check_method_call(
        &mut self,
        id: ExprId,
        base: &Expr,
        method_name: &str,
        name_span: Span,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let bt = self.check_expr(base, None);
        if self.error_exprs.contains(&base.id) {
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let (owner, receiver_args) = match &bt {
            Type::Struct(name, args) => (name.clone(), args.clone()),
            Type::Enum(name, args) => (name.clone(), args.clone()),
            Type::Void => {
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
            other => {
                let mut diag = Diagnostic::new(
                    codes::UNKNOWN_METHOD,
                    format!("type `{}` has no methods", other),
                    name_span,
                );
                diag.help = Some(
                    "methods can only be defined for structs and enums with an `impl` block"
                        .to_string(),
                );
                self.diags.push(diag);
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        let info = self
            .methods
            .get(&owner)
            .and_then(|ms| ms.get(method_name))
            .cloned();
        let info = match info {
            Some(info) => info,
            None => {
                let is_field = self
                    .structs
                    .get(&owner)
                    .and_then(|s| s.field(method_name))
                    .is_some();
                let is_variant = self
                    .enums
                    .get(&owner)
                    .and_then(|e| e.variant(method_name))
                    .is_some();
                if is_field || is_variant {
                    let what = if is_variant { "variant" } else { "field" };
                    let mut diag = Diagnostic::new(
                        codes::NOT_CALLABLE,
                        format!(
                            "`{}` is a {} of `{}`, not a method",
                            method_name,
                            what,
                            short_of(&owner)
                        ),
                        name_span,
                    );
                    diag.help = Some(format!(
                        "call a method or use `{}.{}` as a value",
                        short_of(&owner),
                        method_name
                    ));
                    self.diags.push(diag);
                } else {
                    let mut diag = Diagnostic::new(
                        codes::UNKNOWN_METHOD,
                        format!("`{}` has no method `{}`", short_of(&owner), method_name),
                        name_span,
                    );
                    let available = self.available_methods(&owner);
                    diag.help = Some(if available.is_empty() {
                        format!(
                            "`{}` has no methods; add an `impl {} {{ ... }}` block",
                            short_of(&owner),
                            short_of(&owner)
                        )
                    } else {
                        format!("available methods: {}", available)
                    });
                    self.diags.push(diag);
                }
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        if !self.check_symbol_visibility(
            &format!("{}::{}", info.owner, info.name),
            &info.module,
            "method",
            info.is_pub,
            name_span,
        ) {
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let bindings: HashMap<String, Type> = info
            .type_params
            .iter()
            .cloned()
            .zip(receiver_args.iter().cloned())
            .collect();
        let concrete_params: Vec<Type> = info
            .params
            .iter()
            .map(|p| substitute_type(p, &bindings))
            .collect();
        let concrete_ret = substitute_type(&info.ret, &bindings);
        if info.receiver == Some(ReceiverKind::Mutable) {
            match self.root_ident(base) {
                Some(root) => {
                    if let ExprKind::Ident(root_name) = &root.kind {
                        if let Some(decl_id) = self.lookup(root_name) {
                            if let Some(vi) = self.var_decls.get(&decl_id) {
                                if !vi.mutable {
                                    let mut diag = Diagnostic::new(
                                        codes::MUTATING_METHOD_RECEIVER,
                                        format!(
                                            "cannot call mutating method `{}` on `{}`: it was declared immutable",
                                            method_name, root_name
                                        ),
                                        name_span,
                                    );
                                    diag.help = Some(format!(
                                        "declare it with `var {} = ...;` to allow mutating method calls",
                                        root_name
                                    ));
                                    self.diags.push(diag);
                                }
                            }
                        }
                    }
                }
                None => {
                    let mut diag = Diagnostic::new(
                        codes::MUTATING_METHOD_RECEIVER,
                        format!(
                            "cannot call mutating method `{}` on a temporary value",
                            method_name
                        ),
                        name_span,
                    );
                    diag.help = Some(
                        "store the value in a `var` variable first and call the method on it"
                            .to_string(),
                    );
                    self.diags.push(diag);
                }
            }
        }
        if args.len() != concrete_params.len() {
            let mut diag = Diagnostic::new(
                codes::ARG_COUNT,
                format!(
                    "method `{}` expects {} argument{}, found {}",
                    method_name,
                    concrete_params.len(),
                    if concrete_params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                span,
            );
            diag.label = Some(format!(
                "expected {} argument{}, found {}",
                concrete_params.len(),
                if concrete_params.len() == 1 { "" } else { "s" },
                args.len()
            ));
            diag.help = Some(format!(
                "call it as `value.{}({})`",
                method_name,
                concrete_params
                    .iter()
                    .map(|p| format!("<{}>", p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            self.diags.push(diag);
        }
        for (i, arg) in args.iter().enumerate() {
            let expected = concrete_params.get(i).cloned();
            let at = match &expected {
                Some(t) => self.check_expr(arg, Some(t)),
                None => self.check_expr(arg, None),
            };
            self.require_value(&at, arg.span, "argument");
            if let Some(target) = &expected {
                self.coerce(&at, target, arg.span, "argument", codes::TYPE_MISMATCH);
            }
        }
        self.callee_refs.insert(
            id,
            Callee::Method(MethodCallInfo {
                owner: info.owner.clone(),
                method: info.name.clone(),
                base_c_name: info.c_name.clone(),
                receiver: info.receiver,
                params: concrete_params,
                type_args: receiver_args,
            }),
        );
        concrete_ret
    }

    fn check_match(
        &mut self,
        id: ExprId,
        scrutinee: &Expr,
        arms: &[MatchArm],
        expected: Option<&Type>,
    ) -> Type {
        let scrut_ty = self.check_expr(scrutinee, None);
        self.require_value(&scrut_ty, scrutinee.span, "match value");
        let pattern_types = vec![scrut_ty.clone()];
        let mut rows: Vec<Vec<NormPat>> = Vec::new();
        let mut arm_type: Option<Type> = None;
        for (index, arm) in arms.iter().enumerate() {
            self.push_scope();
            let mut bindings: Vec<BindingInfo> = Vec::new();
            self.check_pattern(&arm.pattern, &scrut_ty, &mut bindings);
            self.match_arm_bindings.insert((id, index), bindings);
            if let Some(guard) = &arm.guard {
                let gt = self.check_expr(guard, Some(&Type::Bool));
                self.require_value(&gt, guard.span, "match guard");
                if gt != Type::Bool {
                    let mut diag = Diagnostic::new(
                        codes::CONDITION_NOT_BOOL,
                        format!("match guard must be `bool`, found `{}`", gt),
                        guard.span,
                    );
                    diag.help = Some("use a comparison such as `n > 0` in the guard".to_string());
                    self.diags.push(diag);
                }
            }
            let body_ty = self.check_expr(&arm.body, expected);
            match &arm_type {
                None => arm_type = Some(body_ty),
                Some(t0) => {
                    if body_ty != *t0 {
                        let mut diag = Diagnostic::new(
                            codes::TYPE_MISMATCH,
                            format!(
                                "match arms must all have the same type: expected `{}`, found `{}`",
                                t0, body_ty
                            ),
                            arm.body.span,
                        );
                        diag.help = Some(
                            "convert the value explicitly so all arms agree on a type".to_string(),
                        );
                        self.diags.push(diag);
                    }
                }
            }
            self.pop_scope();
            if arm.guard.is_some() {
                continue;
            }
            let ctx = MatchCtx { enums: &self.enums };
            let alt_rows = matchcheck::normalize_pattern_rows(&arm.pattern, &scrut_ty, &ctx);
            for (i, row) in alt_rows.iter().enumerate() {
                if matchcheck::is_useful(&rows, std::slice::from_ref(row), &pattern_types, &ctx) {
                    continue;
                }
                let span = match &arm.pattern {
                    Pattern::Or(alts, s) => alts.get(i).map(pattern_span_of).unwrap_or(*s),
                    other => pattern_span_of(other),
                };
                let mut diag =
                    Diagnostic::new(codes::UNREACHABLE_PATTERN, "unreachable match arm", span);
                diag.help = Some("this pattern is already covered by an earlier arm".to_string());
                self.diags.push(diag);
            }
            for row in alt_rows {
                rows.push(vec![row]);
            }
        }
        let ctx = MatchCtx { enums: &self.enums };
        if let Some(witness) = matchcheck::is_exhaustive(&rows, &pattern_types, &ctx) {
            let witness_text = matchcheck::witness_to_string(&witness, &pattern_types, &ctx);
            let mut diag = Diagnostic::new(
                codes::NON_EXHAUSTIVE_MATCH,
                format!(
                    "non-exhaustive match: the pattern `{}` is not covered",
                    witness_text
                ),
                scrutinee.span,
            );
            diag.help = Some(format!(
                "add an arm for `{}` or a wildcard arm `_ => ...`",
                witness_text
            ));
            self.diags.push(diag);
        }
        arm_type.unwrap_or(Type::Void)
    }

    fn check_pattern(&mut self, pat: &Pattern, ty: &Type, bindings: &mut Vec<BindingInfo>) {
        match pat {
            Pattern::Wild(_) => {}
            Pattern::Or(alternatives, span) => {
                let mut per_alt: Vec<Vec<BindingInfo>> = Vec::new();
                for alt in alternatives {
                    let mut b: Vec<BindingInfo> = Vec::new();
                    self.push_scope();
                    self.check_pattern(alt, ty, &mut b);
                    self.pop_scope();
                    per_alt.push(b);
                }
                let first: Vec<(String, Type)> = per_alt[0]
                    .iter()
                    .map(|b| (b.name.clone(), b.ty.clone()))
                    .collect();
                let mut consistent = true;
                for b in &per_alt[1..] {
                    let this: Vec<(String, Type)> =
                        b.iter().map(|x| (x.name.clone(), x.ty.clone())).collect();
                    if this != first {
                        consistent = false;
                    }
                }
                if !consistent {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_PATTERN,
                        "all alternatives of an or-pattern must bind the same names with the same types",
                        *span,
                    );
                    diag.help = Some(
                        "bind a variable in every alternative (or in none of them)".to_string(),
                    );
                    self.diags.push(diag);
                }
                for b in &per_alt[0] {
                    let scope = self.scopes.last_mut().expect("scope stack");
                    scope.insert(b.name.clone(), b.decl_id);
                }
                bindings.extend(per_alt.into_iter().next().unwrap_or_default());
            }
            Pattern::Literal(lit, span) => {
                let lit_ty = match lit {
                    LiteralPattern::Int(_) => Type::Int,
                    LiteralPattern::Bool(_) => Type::Bool,
                    LiteralPattern::Char(_) => Type::Char,
                    LiteralPattern::Str(_) => Type::String,
                };
                if lit_ty != *ty {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_PATTERN,
                        format!(
                            "literal pattern of type `{}` cannot match a value of type `{}`",
                            lit_ty, ty
                        ),
                        *span,
                    );
                    diag.help = Some(format!("use a `{}` literal or a wildcard `_`", ty));
                    self.diags.push(diag);
                }
            }
            Pattern::Binding(name, span, id) => {
                let c_name = self.fresh_c_name(name);
                self.var_decls.insert(
                    *id,
                    VarInfo {
                        c_name: c_name.clone(),
                        ty: ty.clone(),
                        mutable: false,
                        kind: DeclKind::Let,
                    },
                );
                self.declare(name, *id, *span);
                bindings.push(BindingInfo {
                    c_name,
                    ty: ty.clone(),
                    name: name.clone(),
                    decl_id: *id,
                });
            }
            Pattern::Variant {
                path,
                name_span,
                args,
                named,
                ..
            } => {
                let variant_name = path.last().cloned().unwrap_or_default();
                let prefix = &path[..path.len().saturating_sub(1)];
                let Some(enum_name) = self.resolve_enum_name(prefix) else {
                    let mut diag = Diagnostic::new(
                        codes::UNKNOWN_VARIANT,
                        format!("unknown enum `{}` in pattern", prefix.join("::")),
                        *name_span,
                    );
                    diag.help = Some("patterns use `EnumName::Variant`".to_string());
                    self.diags.push(diag);
                    return;
                };
                let scrut_args = match ty {
                    Type::Enum(n, a) if *n == enum_name => a.clone(),
                    Type::Enum(n, _) => {
                        self.error(
                            codes::INVALID_PATTERN,
                            format!(
                                "pattern is for enum `{}` but the value has type `{}`",
                                enum_name, n
                            ),
                            *name_span,
                        );
                        return;
                    }
                    other => {
                        self.error(
                            codes::INVALID_PATTERN,
                            format!(
                                "cannot match an enum pattern against a value of type `{}`",
                                other
                            ),
                            *name_span,
                        );
                        return;
                    }
                };
                let info = self.enums.get(&enum_name).cloned().unwrap();
                let Some((_tag, variant)) = info.variant(&variant_name) else {
                    let mut diag = Diagnostic::new(
                        codes::UNKNOWN_VARIANT,
                        format!("enum `{}` has no variant `{}`", enum_name, variant_name),
                        *name_span,
                    );
                    diag.help = Some(format!(
                        "available variants of `{}`: {}",
                        enum_name,
                        info.variant_names()
                    ));
                    self.diags.push(diag);
                    return;
                };
                let bindings_map: HashMap<String, Type> = info
                    .params
                    .iter()
                    .cloned()
                    .zip(scrut_args.iter().cloned())
                    .collect();
                let field_types: Vec<Type> = variant
                    .fields
                    .iter()
                    .map(|f| substitute_type(f, &bindings_map))
                    .collect();
                if let Some(field_names) = &variant.field_names {
                    if !args.is_empty() {
                        self.error(
                            codes::INVALID_PATTERN,
                            format!(
                                "variant `{}::{}` uses named fields",
                                enum_name, variant_name
                            ),
                            *name_span,
                        );
                    }
                    let mut seen = HashSet::new();
                    for nf in named {
                        if !seen.insert(nf.name.clone()) {
                            self.error(
                                codes::INVALID_PATTERN,
                                format!("field `{}` is matched more than once", nf.name),
                                nf.name_span,
                            );
                            continue;
                        }
                        match field_names.iter().position(|n| *n == nf.name) {
                            Some(idx) => {
                                let ft = field_types.get(idx).cloned().unwrap_or(Type::Int);
                                self.check_pattern(&nf.pattern, &ft, bindings);
                            }
                            None => {
                                let mut diag = Diagnostic::new(
                                    codes::UNKNOWN_FIELD,
                                    format!(
                                        "variant `{}::{}` has no field `{}`",
                                        enum_name, variant_name, nf.name
                                    ),
                                    nf.name_span,
                                );
                                diag.help =
                                    Some(format!("available fields: {}", field_names.join(", ")));
                                self.diags.push(diag);
                            }
                        }
                    }
                    for name in field_names {
                        if !named.iter().any(|f| f.name == *name) {
                            let mut diag = Diagnostic::new(
                                codes::INVALID_PATTERN,
                                format!(
                                    "pattern must also list the field `{}` of variant `{}::{}`",
                                    name, enum_name, variant_name
                                ),
                                *name_span,
                            );
                            diag.help = Some(
                                "list every field or use `field: _` to ignore one".to_string(),
                            );
                            self.diags.push(diag);
                        }
                    }
                } else {
                    if !named.is_empty() {
                        self.error(
                            codes::INVALID_PATTERN,
                            format!(
                                "variant `{}::{}` uses positional values",
                                enum_name, variant_name
                            ),
                            *name_span,
                        );
                    }
                    if args.len() != field_types.len() {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_PATTERN,
                            format!(
                                "variant `{}::{}` expects {} value{}, found {}",
                                enum_name,
                                variant_name,
                                field_types.len(),
                                if field_types.len() == 1 { "" } else { "s" },
                                args.len()
                            ),
                            *name_span,
                        );
                        diag.help = Some(format!(
                            "write `{}::{}({})`",
                            enum_name,
                            variant_name,
                            field_types
                                .iter()
                                .map(|f| format!("<{}>", f))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                        self.diags.push(diag);
                    }
                    for (i, sub) in args.iter().enumerate() {
                        let ft = field_types.get(i).cloned().unwrap_or(Type::Int);
                        self.check_pattern(sub, &ft, bindings);
                    }
                }
            }
        }
    }

    fn check_instance_slices(
        &mut self,
        params: &[Type],
        ret: &Type,
        bindings: &HashMap<String, Type>,
        span: Span,
        what: &str,
    ) {
        let mut bad: Option<String> = None;
        for p in params {
            let t = substitute_type(p, bindings);
            if let Some(elem) = Self::slice_elem_problem(&t) {
                bad = Some(elem);
                break;
            }
        }
        if bad.is_none() {
            let t = substitute_type(ret, bindings);
            bad = Self::slice_elem_problem(&t);
        }
        if let Some(elem) = bad {
            let mut diag = Diagnostic::new(
                codes::UNKNOWN_TYPE,
                format!(
                    "cannot instantiate `{}` with `Slice<{}>`: slice views require a basic element type",
                    what, elem
                ),
                span,
            );
            diag.help = Some(
                "use `int`, `float`, `bool`, `string` or `char` as slice element type".to_string(),
            );
            self.diags.push(diag);
        }
    }

    fn slice_elem_problem(ty: &Type) -> Option<String> {
        match ty {
            Type::Slice(inner) => match &**inner {
                Type::Param(_) => None,
                t if t.is_basic() => None,
                t => Some(t.to_string()),
            },
            Type::Array(inner) => Self::slice_elem_problem(inner),
            Type::Struct(_, args) | Type::Enum(_, args) => {
                args.iter().find_map(Self::slice_elem_problem)
            }
            _ => None,
        }
    }

    fn collect_instantiations(
        &mut self,
    ) -> (Vec<Type>, Vec<FunctionInstance>, Vec<MethodInstance>) {
        let mut fn_instances: Vec<FunctionInstance> = Vec::new();
        let mut method_instances: Vec<MethodInstance> = Vec::new();
        let mut fn_seen: HashSet<(usize, Vec<Type>)> = HashSet::new();
        let mut method_seen: HashSet<(String, String, Vec<Type>)> = HashSet::new();
        let mut fn_queue: Vec<(usize, Vec<Type>)> = Vec::new();
        let mut method_queue: Vec<(String, String, Vec<Type>)> = Vec::new();
        for callee in self.callee_refs.values() {
            match callee {
                Callee::User(info) if !info.type_args.is_empty() => {
                    if !info.type_args.iter().any(|t| t.contains_param())
                        && fn_seen.insert((info.index, info.type_args.clone()))
                    {
                        fn_queue.push((info.index, info.type_args.clone()));
                    }
                }
                Callee::Method(info)
                    if !info.type_args.is_empty()
                        && !info.type_args.iter().any(|t| t.contains_param())
                        && method_seen.insert((
                            info.owner.clone(),
                            info.method.clone(),
                            info.type_args.clone(),
                        )) =>
                {
                    method_queue.push((
                        info.owner.clone(),
                        info.method.clone(),
                        info.type_args.clone(),
                    ));
                }
                _ => {}
            }
        }
        while let Some((index, type_args)) = fn_queue.pop() {
            fn_instances.push(FunctionInstance {
                index,
                type_args: type_args.clone(),
                c_name: specialized_c_name(&self.functions[index].c_name, &type_args),
            });
            let sig = self.functions[index].clone();
            let bindings: HashMap<String, Type> = sig
                .type_params
                .iter()
                .cloned()
                .zip(type_args.iter().cloned())
                .collect();
            self.check_instance_slices(&sig.params, &sig.ret, &bindings, sig.span, &sig.c_name);
            let mut call_ids = Vec::new();
            collect_call_ids(&sig.body, &mut call_ids);
            for id in call_ids {
                match self.callee_refs.get(&id) {
                    Some(Callee::User(info)) if !info.type_args.is_empty() => {
                        let new_args: Vec<Type> = info
                            .type_args
                            .iter()
                            .map(|t| substitute_type(t, &bindings))
                            .collect();
                        if !new_args.iter().any(|t| t.contains_param())
                            && fn_seen.insert((info.index, new_args.clone()))
                        {
                            fn_queue.push((info.index, new_args));
                        }
                    }
                    Some(Callee::Method(info)) if !info.type_args.is_empty() => {
                        let new_args: Vec<Type> = info
                            .type_args
                            .iter()
                            .map(|t| substitute_type(t, &bindings))
                            .collect();
                        if !new_args.iter().any(|t| t.contains_param())
                            && method_seen.insert((
                                info.owner.clone(),
                                info.method.clone(),
                                new_args.clone(),
                            ))
                        {
                            method_queue.push((info.owner.clone(), info.method.clone(), new_args));
                        }
                    }
                    _ => {}
                }
            }
        }
        while let Some((owner, method, type_args)) = method_queue.pop() {
            let info = match self.methods.get(&owner).and_then(|ms| ms.get(&method)) {
                Some(info) => info.clone(),
                None => continue,
            };
            method_instances.push(MethodInstance {
                owner: owner.clone(),
                method: method.clone(),
                type_args: type_args.clone(),
                c_name: specialized_c_name(&info.c_name, &type_args),
            });
            let bindings: HashMap<String, Type> = info
                .type_params
                .iter()
                .cloned()
                .zip(type_args.iter().cloned())
                .collect();
            self.check_instance_slices(
                &info.params,
                &info.ret,
                &bindings,
                info.body.span,
                &info.c_name,
            );
            let mut call_ids = Vec::new();
            collect_call_ids(&info.body, &mut call_ids);
            for id in call_ids {
                match self.callee_refs.get(&id) {
                    Some(Callee::User(finfo)) if !finfo.type_args.is_empty() => {
                        let new_args: Vec<Type> = finfo
                            .type_args
                            .iter()
                            .map(|t| substitute_type(t, &bindings))
                            .collect();
                        if !new_args.iter().any(|t| t.contains_param())
                            && fn_seen.insert((finfo.index, new_args.clone()))
                        {
                            fn_queue.push((finfo.index, new_args));
                        }
                    }
                    Some(Callee::Method(minfo)) if !minfo.type_args.is_empty() => {
                        let new_args: Vec<Type> = minfo
                            .type_args
                            .iter()
                            .map(|t| substitute_type(t, &bindings))
                            .collect();
                        if !new_args.iter().any(|t| t.contains_param())
                            && method_seen.insert((
                                minfo.owner.clone(),
                                minfo.method.clone(),
                                new_args.clone(),
                            ))
                        {
                            method_queue.push((
                                minfo.owner.clone(),
                                minfo.method.clone(),
                                new_args,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut sources: Vec<Type> = Vec::new();
        for sig in &self.functions {
            if sig.type_params.is_empty() {
                sources.extend(sig.params.iter().cloned());
                sources.push(sig.ret.clone());
            }
        }
        for inst in &fn_instances {
            let sig = self.functions[inst.index].clone();
            let bindings: HashMap<String, Type> = sig
                .type_params
                .iter()
                .cloned()
                .zip(inst.type_args.iter().cloned())
                .collect();
            for p in &sig.params {
                sources.push(substitute_type(p, &bindings));
            }
            sources.push(substitute_type(&sig.ret, &bindings));
        }
        for methods in self.methods.values() {
            for m in methods.values() {
                if m.type_params.is_empty() {
                    sources.extend(m.params.iter().cloned());
                    sources.push(m.ret.clone());
                }
            }
        }
        for inst in &method_instances {
            let info = match self
                .methods
                .get(&inst.owner)
                .and_then(|ms| ms.get(&inst.method))
            {
                Some(info) => info.clone(),
                None => continue,
            };
            let bindings: HashMap<String, Type> = info
                .type_params
                .iter()
                .cloned()
                .zip(inst.type_args.iter().cloned())
                .collect();
            for p in &info.params {
                sources.push(substitute_type(p, &bindings));
            }
            sources.push(substitute_type(&info.ret, &bindings));
            if info.is_enum {
                sources.push(Type::Enum(inst.owner.clone(), inst.type_args.clone()));
            } else {
                sources.push(Type::Struct(inst.owner.clone(), inst.type_args.clone()));
            }
        }
        for (name, s) in &self.structs {
            if s.params.is_empty() {
                sources.push(Type::Struct(name.clone(), Vec::new()));
            }
        }
        for v in self.var_decls.values() {
            sources.push(v.ty.clone());
        }
        for g in &self.globals {
            sources.push(g.ty.clone());
        }
        for sig in &self.functions {
            if sig.module.starts_with("std::") {
                for p in &sig.params {
                    sources.push(p.clone());
                }
                sources.push(sig.ret.clone());
            }
        }
        for t in self.expr_types.values() {
            sources.push(t.clone());
        }
        let structs = self.structs.clone();
        let enums = self.enums.clone();
        let (ordered, errors) = topo_instantiations(&sources, &structs, &enums);
        for e in errors {
            self.diags.push(e);
        }
        (ordered, fn_instances, method_instances)
    }

    fn available_methods(&self, struct_name: &str) -> String {
        let mut names: Vec<String> = self
            .methods
            .get(struct_name)
            .map(|ms| ms.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names.join(", ")
    }

    fn resolve_enum_name(&self, segments: &[String]) -> Option<String> {
        match self.resolve_path(segments) {
            PathResolution::Enum(canonical) => Some(canonical),
            _ => None,
        }
    }

    fn resolve_struct_name(&self, segments: &[String]) -> Option<String> {
        match self.resolve_path(segments) {
            PathResolution::Struct(canonical) => Some(canonical),
            _ => None,
        }
    }

    fn register_enum_names(&mut self, program: &mut Program) {
        for item in &mut program.items {
            let Item::Enum(e) = item else { continue };
            if is_basic_type_name(&e.name) || builtin_kind(&e.name).is_some() {
                self.error(
                    codes::DUPLICATE_DECL,
                    format!(
                        "`{}` is a reserved name and cannot be used as an enum name",
                        e.name
                    ),
                    e.name_span,
                );
                continue;
            }
            if e.name.contains("__") {
                self.error(
                    codes::RESERVED_IDENT,
                    format!(
                        "identifier `{}` contains `__`, which is reserved for code generation",
                        e.name
                    ),
                    e.name_span,
                );
                continue;
            }
            if e.name == "Option" || e.name == "Result" {
                self.error(
                    codes::DUPLICATE_DECL,
                    format!("`{}` is a built-in type and cannot be redefined", e.name),
                    e.name_span,
                );
                continue;
            }
            let canonical = self.canonical_name(&e.name);
            if self.enums.contains_key(&canonical) {
                self.error(
                    codes::DUPLICATE_DECL,
                    format!("enum `{}` is defined more than once", e.name),
                    e.name_span,
                );
                continue;
            }
            let mut params = Vec::new();
            let mut seen = HashSet::new();
            for p in &e.params {
                if !seen.insert(p.clone()) {
                    self.error(
                        codes::INVALID_ENUM,
                        format!("duplicate type parameter `{}` in enum `{}`", p, e.name),
                        e.name_span,
                    );
                }
                params.push(p.clone());
            }
            e.canonical = canonical.clone();
            self.enums.insert(
                canonical.clone(),
                EnumInfo {
                    params,
                    variants: Vec::new(),
                    span: e.span,
                    is_pub: e.is_pub,
                    module: self.current_module.clone(),
                },
            );
        }
    }

    fn resolve_enum_variants(&mut self, program: &Program) {
        for item in &program.items {
            let Item::Enum(e) = item else { continue };
            let registered = self
                .enums
                .get(&e.canonical)
                .map(|info| info.span == e.span && info.variants.is_empty())
                .unwrap_or(false);
            if !registered {
                continue;
            }
            if e.variants.is_empty() {
                self.error(
                    codes::INVALID_ENUM,
                    format!("enum `{}` must declare at least one variant", e.name),
                    e.span,
                );
                continue;
            }
            let params: HashSet<String> = e.params.iter().cloned().collect();
            let mut variants = Vec::new();
            let mut variant_names = HashSet::new();
            for v in &e.variants {
                if !variant_names.insert(v.name.clone()) {
                    self.error(
                        codes::INVALID_ENUM,
                        format!("duplicate variant `{}` in enum `{}`", v.name, e.name),
                        v.name_span,
                    );
                    continue;
                }
                self.enum_params = Some(params.clone());
                let (fields, field_names) = match &v.payload {
                    EnumPayload::Unit => (Vec::new(), None),
                    EnumPayload::Tuple(tys) => {
                        let mut fs = Vec::new();
                        for t in tys {
                            fs.push(self.type_from_typeexpr(t, false));
                        }
                        (fs, None)
                    }
                    EnumPayload::Named(fields) => {
                        let mut fs = Vec::new();
                        let mut names = Vec::new();
                        let mut fseen = HashSet::new();
                        for f in fields {
                            if !fseen.insert(f.name.clone()) {
                                self.error(
                                    codes::INVALID_ENUM,
                                    format!("duplicate field `{}` in variant `{}`", f.name, v.name),
                                    f.name_span,
                                );
                                continue;
                            }
                            names.push(f.name.clone());
                            fs.push(self.type_from_typeexpr(&f.ty, false));
                        }
                        (fs, Some(names))
                    }
                };
                self.enum_params = None;
                variants.push(EnumVariantInfo {
                    name: v.name.clone(),
                    fields,
                    field_names,
                });
            }
            if let Some(info) = self.enums.get_mut(&e.canonical) {
                info.variants = variants;
            }
        }
    }

    fn enum_field_has_param(ty: &Type) -> bool {
        match ty {
            Type::Param(_) => true,
            Type::Enum(_, args) => args.iter().any(Self::enum_field_has_param),
            Type::Array(inner) => Self::enum_field_has_param(inner),
            _ => false,
        }
    }

    fn collect_constraints(field_ty: &Type, arg_ty: &Type, out: &mut Vec<(String, Type)>) {
        match (field_ty, arg_ty) {
            (Type::Param(p), t) => out.push((p.clone(), t.clone())),
            (Type::Enum(n1, a1), Type::Enum(n2, a2)) if n1 == n2 && a1.len() == a2.len() => {
                for (f, a) in a1.iter().zip(a2.iter()) {
                    Self::collect_constraints(f, a, out);
                }
            }
            (Type::Array(f), Type::Array(a)) => Self::collect_constraints(f, a, out),
            (Type::Slice(f), Type::Slice(a)) => Self::collect_constraints(f, a, out),
            _ => {}
        }
    }

    fn resolve_type_args(
        &mut self,
        what: &str,
        name: &str,
        params: &[String],
        expected: Option<&Type>,
        constraints: &[(String, Type)],
        span: Span,
    ) -> Option<Vec<Type>> {
        if params.is_empty() {
            return Some(Vec::new());
        }
        let mut bindings: HashMap<String, Type> = HashMap::new();
        let expected_args = match expected {
            Some(Type::Enum(n, args)) | Some(Type::Struct(n, args))
                if n == name && args.len() == params.len() =>
            {
                Some(args.clone())
            }
            _ => None,
        };
        if let Some(args) = expected_args {
            for (p, a) in params.iter().zip(args.iter()) {
                bindings.insert(p.clone(), a.clone());
            }
        }
        for (p, ty) in constraints {
            bindings.entry(p.clone()).or_insert_with(|| ty.clone());
        }
        let mut out = Vec::new();
        let mut missing = Vec::new();
        for p in params {
            match bindings.get(p) {
                Some(t) => out.push(t.clone()),
                None => missing.push(p.clone()),
            }
        }
        if !missing.is_empty() {
            let mut diag = Diagnostic::new(
                codes::CANNOT_INFER_TYPE_ARGS,
                format!(
                    "cannot infer type parameter{} {} of {}",
                    if missing.len() == 1 { "" } else { "s" },
                    missing
                        .iter()
                        .map(|m| format!("`{}`", m))
                        .collect::<Vec<_>>()
                        .join(", "),
                    what
                ),
                span,
            );
            diag.help = Some(format!(
                "add a type annotation, e.g. `let x: {}<{}> = ...;`",
                short_of(name),
                params
                    .iter()
                    .map(|p| if missing.contains(p) {
                        "int".to_string()
                    } else {
                        p.clone()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            self.diags.push(diag);
            return None;
        }
        Some(out)
    }

    fn resolve_enum_args(
        &mut self,
        enum_name: &str,
        expected: Option<&Type>,
        constraints: &[(String, Type)],
        span: Span,
    ) -> Option<Vec<Type>> {
        let info = self.enums.get(enum_name).cloned()?;
        let what = format!("enum `{}`", short_of(enum_name));
        self.resolve_type_args(&what, enum_name, &info.params, expected, constraints, span)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_variant_ref(
        &mut self,
        id: ExprId,
        enum_name: &str,
        args: &[Type],
        tag: usize,
        variant_name: &str,
        field_names: Option<Vec<String>>,
        raw_fields: &[Type],
    ) {
        let info = match self.enums.get(enum_name) {
            Some(info) => info.clone(),
            None => return,
        };
        let bindings: HashMap<String, Type> = info
            .params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let field_types = raw_fields
            .iter()
            .map(|f| substitute_type(f, &bindings))
            .collect();
        self.variant_refs.insert(
            id,
            VariantRef {
                enum_name: enum_name.to_string(),
                enum_args: args.to_vec(),
                tag,
                variant_name: variant_name.to_string(),
                field_names,
                field_types,
            },
        );
    }

    fn check_try(&mut self, id: ExprId, operand: &Expr, span: Span) -> Type {
        let operand_ty = self.check_expr(operand, None);
        if self.error_exprs.contains(&operand.id) {
            self.error_exprs.insert(id);
            return Type::Int;
        }
        self.require_value(&operand_ty, operand.span, "operand");
        enum TryKind {
            Result(Type, Type),
            Option(Type),
        }
        let kind = match &operand_ty {
            Type::Enum(name, args) if name == "Result" && args.len() == 2 => {
                TryKind::Result(args[0].clone(), args[1].clone())
            }
            Type::Enum(name, args) if name == "Option" && args.len() == 1 => {
                TryKind::Option(args[0].clone())
            }
            other => {
                let mut diag = Diagnostic::new(
                    codes::TRY_INVALID_OPERAND,
                    format!(
                        "the `?` operator requires a `Result<T, E>` or `Option<T>` value, found `{}`",
                        other
                    ),
                    span,
                );
                diag.help = Some(
                    "use `?` only on expressions that produce a `Result` or `Option`".to_string(),
                );
                self.diags.push(diag);
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        let ret_ty = self.current_ret.clone().unwrap_or(Type::Void);
        let result_ret_err = match &ret_ty {
            Type::Enum(name, args) if name == "Result" && args.len() == 2 => Some(args[1].clone()),
            _ => None,
        };
        let option_ret =
            matches!(&ret_ty, Type::Enum(name, args) if name == "Option" && args.len() == 1);
        match &kind {
            TryKind::Result(_, err_ty) => match result_ret_err {
                Some(ret_err) => {
                    if ret_err != *err_ty {
                        let mut diag = Diagnostic::new(
                            codes::TRY_RETURN_MISMATCH,
                            format!(
                                "the `?` operator propagates `{}`, but the function returns `{}`",
                                operand_ty, ret_ty
                            ),
                            span,
                        );
                        diag.help = Some(format!(
                            "the function must return `Result<_, {}>` to use `?` here",
                            err_ty
                        ));
                        self.diags.push(diag);
                    }
                }
                None => {
                    self.try_mismatch_diag(&operand_ty, &ret_ty, "Result<_, _>", span);
                }
            },
            TryKind::Option(_) => {
                if !option_ret {
                    self.try_mismatch_diag(&operand_ty, &ret_ty, "Option<_>", span);
                }
            }
        }
        match kind {
            TryKind::Result(ok, _) => ok,
            TryKind::Option(ok) => ok,
        }
    }

    fn try_mismatch_diag(&mut self, operand_ty: &Type, ret_ty: &Type, expected: &str, span: Span) {
        let mut diag = Diagnostic::new(
            codes::TRY_RETURN_MISMATCH,
            format!(
                "the `?` operator propagates `{}`, but the function returns `{}`",
                operand_ty, ret_ty
            ),
            span,
        );
        diag.help = Some(format!(
            "change the return type to `{}` or handle the case with `match`",
            expected
        ));
        self.diags.push(diag);
    }

    fn check_path_expr(
        &mut self,
        id: ExprId,
        segments: &[String],
        span: Span,
        expected: Option<&Type>,
    ) -> Type {
        match self.resolve_path(segments) {
            PathResolution::EnumVariant(enum_name, variant_name) => {
                self.check_variant_unit(id, &enum_name, &variant_name, span, expected)
            }
            PathResolution::Global(canonical) => {
                let index = match self.global_index.get(&canonical) {
                    Some(&i) => i,
                    None => {
                        self.error_exprs.insert(id);
                        return Type::Int;
                    }
                };
                let g = self.globals[index].clone();
                if !self.check_symbol_visibility(&canonical, &g.module, "constant", g.is_pub, span)
                {
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
                self.global_refs.insert(id, index);
                g.ty
            }
            PathResolution::Function(canonical) => {
                let mut diag = Diagnostic::new(
                    codes::NOT_CALLABLE,
                    format!(
                        "`{}` is a function; functions are not values",
                        short_of(&canonical)
                    ),
                    span,
                );
                diag.help = Some(format!("call it as `{}(...)`", short_of(&canonical)));
                self.diags.push(diag);
                self.error_exprs.insert(id);
                Type::Int
            }
            PathResolution::AssociatedMethod(owner, method) => {
                let mut diag = Diagnostic::new(
                    codes::NOT_CALLABLE,
                    format!(
                        "`{}::{}` is an associated function; functions are not values",
                        short_of(&owner),
                        method
                    ),
                    span,
                );
                diag.help = Some(format!(
                    "call it as `{}::{}(...)`",
                    short_of(&owner),
                    method
                ));
                self.diags.push(diag);
                self.error_exprs.insert(id);
                Type::Int
            }
            PathResolution::Struct(canonical) => {
                let mut diag = Diagnostic::new(
                    codes::UNKNOWN_VARIANT,
                    format!(
                        "`{}` is a struct; construct it with `{} {{ ... }}`",
                        short_of(&canonical),
                        segments.join("::")
                    ),
                    span,
                );
                diag.help = Some("structs have no variants".to_string());
                self.diags.push(diag);
                self.error_exprs.insert(id);
                Type::Int
            }
            PathResolution::Enum(canonical) => {
                let mut diag = Diagnostic::new(
                    codes::UNKNOWN_VARIANT,
                    format!(
                        "`{}` is an enum; select one of its variants",
                        short_of(&canonical)
                    ),
                    span,
                );
                diag.help = Some(format!(
                    "available variants of `{}`: {}",
                    short_of(&canonical),
                    self.enums[&canonical].variant_names()
                ));
                self.diags.push(diag);
                self.error_exprs.insert(id);
                Type::Int
            }
            PathResolution::Unknown => {
                let mut diag = Diagnostic::new(
                    codes::UNDEFINED_VARIABLE,
                    format!("cannot resolve `{}`", segments.join("::")),
                    span,
                );
                diag.help = Some(
                    "qualified names must refer to an enum variant or an imported symbol"
                        .to_string(),
                );
                self.diags.push(diag);
                self.error_exprs.insert(id);
                Type::Int
            }
        }
    }

    fn check_variant_unit(
        &mut self,
        id: ExprId,
        enum_name: &str,
        variant_name: &str,
        span: Span,
        expected: Option<&Type>,
    ) -> Type {
        let info = match self.enums.get(enum_name).cloned() {
            Some(info) => info,
            None => {
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        if !self.check_symbol_visibility(enum_name, &info.module, "enum", info.is_pub, span) {
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let Some((tag, variant)) = info.variant(variant_name) else {
            let mut diag = Diagnostic::new(
                codes::UNKNOWN_VARIANT,
                format!("enum `{}` has no variant `{}`", enum_name, variant_name),
                span,
            );
            diag.help = Some(format!(
                "available variants of `{}`: {}",
                enum_name,
                info.variant_names()
            ));
            self.diags.push(diag);
            self.error_exprs.insert(id);
            return Type::Int;
        };
        if !variant.fields.is_empty() {
            let mut diag = Diagnostic::new(
                codes::INVALID_PATTERN,
                format!("variant `{}::{}` needs a value", enum_name, variant_name),
                span,
            );
            diag.help = Some(format!(
                "construct it as `{}::{}(...)`",
                enum_name, variant_name
            ));
            self.diags.push(diag);
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let Some(args) = self.resolve_enum_args(enum_name, expected, &[], span) else {
            self.error_exprs.insert(id);
            return Type::Int;
        };
        let field_names = variant.field_names.clone();
        let raw_fields = variant.fields.clone();
        self.record_variant_ref(
            id,
            enum_name,
            &args,
            tag,
            variant_name,
            field_names,
            &raw_fields,
        );
        Type::Enum(enum_name.to_string(), args)
    }

    fn partial_bindings(
        enum_name: &str,
        info: &EnumInfo,
        expected: Option<&Type>,
    ) -> HashMap<String, Type> {
        Self::partial_bindings_for(enum_name, &info.params, expected)
    }

    fn partial_bindings_for(
        name: &str,
        params: &[String],
        expected: Option<&Type>,
    ) -> HashMap<String, Type> {
        let mut bindings = HashMap::new();
        let expected_args = match expected {
            Some(Type::Enum(n, args)) | Some(Type::Struct(n, args))
                if n == name && args.len() == params.len() =>
            {
                Some(args.clone())
            }
            _ => None,
        };
        if let Some(args) = expected_args {
            for (p, a) in params.iter().zip(args.iter()) {
                bindings.insert(p.clone(), a.clone());
            }
        }
        bindings
    }

    #[allow(clippy::too_many_arguments)]
    fn check_associated_call(
        &mut self,
        id: ExprId,
        owner: &str,
        method: &str,
        explicit_type_args: &[Type],
        args: &[Expr],
        span: Span,
        expected: Option<&Type>,
        callee_span: Span,
    ) -> Type {
        let info = match self
            .methods
            .get(owner)
            .and_then(|ms| ms.get(method))
            .cloned()
        {
            Some(info) => info,
            None => {
                self.error(
                    codes::UNDEFINED_FUNCTION,
                    format!(
                        "cannot find associated function `{}` on `{}`",
                        method,
                        short_of(owner)
                    ),
                    callee_span,
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        if !self.check_symbol_visibility(
            &format!("{}::{}", info.owner, info.name),
            &info.module,
            "method",
            info.is_pub,
            callee_span,
        ) {
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let display = format!("{}::{}", short_of(owner), method);
        if !explicit_type_args.is_empty() && explicit_type_args.len() != info.type_params.len() {
            let mut diag = Diagnostic::new(
                codes::CANNOT_INFER_TYPE_ARGS,
                format!(
                    "`{}` expects {} type argument{}, found {}",
                    display,
                    info.type_params.len(),
                    if info.type_params.len() == 1 { "" } else { "s" },
                    explicit_type_args.len()
                ),
                span,
            );
            diag.help = Some(format!(
                "write `{}::<{}>(...)`",
                display,
                info.type_params.join(", ")
            ));
            self.diags.push(diag);
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        if args.len() != info.params.len() {
            let mut diag = Diagnostic::new(
                codes::ARG_COUNT,
                format!(
                    "`{}` expects {} argument{}, found {}",
                    display,
                    info.params.len(),
                    if info.params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                span,
            );
            diag.help = Some(format!(
                "call it as `{}({})`",
                display,
                info.params
                    .iter()
                    .map(|p| format!("<{}>", p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            self.diags.push(diag);
        }
        if info.type_params.is_empty() {
            for (i, arg) in args.iter().enumerate() {
                let expected_arg = info.params.get(i).cloned();
                let at = match &expected_arg {
                    Some(t) => self.check_expr(arg, Some(t)),
                    None => self.check_expr(arg, None),
                };
                self.require_value(&at, arg.span, "argument");
                if let Some(target) = &expected_arg {
                    self.coerce(&at, target, arg.span, "argument", codes::TYPE_MISMATCH);
                }
            }
            let ret = info.ret.clone();
            self.callee_refs.insert(
                id,
                Callee::Method(MethodCallInfo {
                    owner: info.owner.clone(),
                    method: info.name.clone(),
                    base_c_name: info.c_name.clone(),
                    receiver: None,
                    params: info.params.clone(),
                    type_args: Vec::new(),
                }),
            );
            return ret;
        }
        let partial: HashMap<String, Type> = if explicit_type_args.is_empty() {
            HashMap::new()
        } else {
            info.type_params
                .iter()
                .cloned()
                .zip(explicit_type_args.iter().cloned())
                .collect()
        };
        let mut constraints: Vec<(String, Type)> = Vec::new();
        let mut arg_types: Vec<Type> = Vec::new();
        if let Some(exp) = expected {
            Self::collect_constraints(&info.ret, exp, &mut constraints);
        }
        for (i, arg) in args.iter().enumerate() {
            let param_ty = info.params.get(i).cloned().unwrap_or(Type::Int);
            let substituted = substitute_type(&param_ty, &partial);
            let expected_arg = if substituted.contains_param() {
                None
            } else {
                Some(substituted)
            };
            let at = self.check_expr(arg, expected_arg.as_ref());
            self.require_value(&at, arg.span, "argument");
            Self::collect_constraints(&param_ty, &at, &mut constraints);
            arg_types.push(at);
        }
        let type_args = if explicit_type_args.is_empty() {
            match self.resolve_type_args(
                &format!("`{}`", display),
                owner,
                &info.type_params,
                None,
                &constraints,
                span,
            ) {
                Some(a) => a,
                None => {
                    self.error_exprs.insert(id);
                    return Type::Int;
                }
            }
        } else {
            explicit_type_args.to_vec()
        };
        let bindings: HashMap<String, Type> = info
            .type_params
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();
        for (i, arg) in args.iter().enumerate() {
            if let Some(param_ty) = info.params.get(i) {
                let concrete = substitute_type(param_ty, &bindings);
                if let Some(at) = arg_types.get(i) {
                    self.coerce(at, &concrete, arg.span, "argument", codes::TYPE_MISMATCH);
                }
            }
        }
        let concrete_params: Vec<Type> = info
            .params
            .iter()
            .map(|p| substitute_type(p, &bindings))
            .collect();
        let concrete_ret = substitute_type(&info.ret, &bindings);
        self.callee_refs.insert(
            id,
            Callee::Method(MethodCallInfo {
                owner: info.owner.clone(),
                method: info.name.clone(),
                base_c_name: info.c_name.clone(),
                receiver: None,
                params: concrete_params,
                type_args,
            }),
        );
        concrete_ret
    }

    fn check_variant_tuple(
        &mut self,
        id: ExprId,
        enum_name: &str,
        variant_name: &str,
        args: &[Expr],
        span: Span,
        expected: Option<&Type>,
    ) -> Type {
        let enum_name = enum_name.to_string();
        let variant_name = variant_name.to_string();
        let info = match self.enums.get(&enum_name).cloned() {
            Some(info) => info,
            None => {
                self.error(
                    codes::UNKNOWN_VARIANT,
                    format!("unknown enum `{}`", short_of(&enum_name)),
                    span,
                );
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        if !self.check_symbol_visibility(&enum_name, &info.module, "enum", info.is_pub, span) {
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let Some((tag, variant)) = info.variant(&variant_name) else {
            let mut diag = Diagnostic::new(
                codes::UNKNOWN_VARIANT,
                format!(
                    "enum `{}` has no variant `{}`",
                    short_of(&enum_name),
                    variant_name
                ),
                span,
            );
            diag.help = Some(format!(
                "available variants of `{}`: {}",
                short_of(&enum_name),
                info.variant_names()
            ));
            self.diags.push(diag);
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        };
        if variant.field_names.is_some() {
            let mut diag = Diagnostic::new(
                codes::INVALID_PATTERN,
                format!(
                    "variant `{}::{}` uses named fields",
                    enum_name, variant_name
                ),
                span,
            );
            diag.help = Some(format!(
                "construct it as `{}::{} {{ ... }}`",
                enum_name, variant_name
            ));
            self.diags.push(diag);
            for arg in args {
                self.check_expr(arg, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        if args.len() != variant.fields.len() {
            let mut diag = Diagnostic::new(
                codes::ARG_COUNT,
                format!(
                    "variant `{}::{}` expects {} value{}, found {}",
                    enum_name,
                    variant_name,
                    variant.fields.len(),
                    if variant.fields.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                span,
            );
            diag.help = Some(format!(
                "construct it as `{}::{}({})`",
                enum_name,
                variant_name,
                variant
                    .fields
                    .iter()
                    .map(|f| format!("<{}>", f))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            self.diags.push(diag);
        }
        let mut constraints = Vec::new();
        let mut arg_types = Vec::new();
        let partial = Self::partial_bindings(&enum_name, &info, expected);
        for (i, arg) in args.iter().enumerate() {
            let field_ty = variant.fields.get(i).cloned().unwrap_or(Type::Int);
            let substituted = substitute_type(&field_ty, &partial);
            let expected_arg = if Self::enum_field_has_param(&substituted) {
                None
            } else {
                Some(substituted.clone())
            };
            let at = self.check_expr(arg, expected_arg.as_ref());
            self.require_value(&at, arg.span, "variant value");
            Self::collect_constraints(&field_ty, &at, &mut constraints);
            arg_types.push(at);
        }
        let Some(enum_args) = self.resolve_enum_args(&enum_name, expected, &constraints, span)
        else {
            self.error_exprs.insert(id);
            return Type::Int;
        };
        let bindings: HashMap<String, Type> = info
            .params
            .iter()
            .cloned()
            .zip(enum_args.iter().cloned())
            .collect();
        for (i, arg) in args.iter().enumerate() {
            if let Some(field_ty) = variant.fields.get(i) {
                let concrete = substitute_type(field_ty, &bindings);
                if let Some(at) = arg_types.get(i) {
                    self.coerce(at, &concrete, arg.span, "argument", codes::TYPE_MISMATCH);
                }
            }
        }
        self.record_variant_ref(
            id,
            &enum_name,
            &enum_args,
            tag,
            &variant_name,
            None,
            &variant.fields,
        );
        Type::Enum(enum_name, enum_args)
    }

    fn check_variant_named(
        &mut self,
        id: ExprId,
        enum_name: &str,
        variant_name: &str,
        fields: &[StructLitField],
        name_span: Span,
        expected: Option<&Type>,
    ) -> Type {
        let info = match self.enums.get(enum_name).cloned() {
            Some(info) => info,
            None => {
                self.error(
                    codes::UNKNOWN_VARIANT,
                    format!("unknown enum `{}`", short_of(enum_name)),
                    name_span,
                );
                for f in fields {
                    self.check_expr(&f.value, None);
                }
                self.error_exprs.insert(id);
                return Type::Int;
            }
        };
        if !self.check_symbol_visibility(enum_name, &info.module, "enum", info.is_pub, name_span) {
            for f in fields {
                self.check_expr(&f.value, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        }
        let Some((tag, variant)) = info.variant(variant_name) else {
            let mut diag = Diagnostic::new(
                codes::UNKNOWN_VARIANT,
                format!("enum `{}` has no variant `{}`", enum_name, variant_name),
                name_span,
            );
            diag.help = Some(format!(
                "available variants of `{}`: {}",
                enum_name,
                info.variant_names()
            ));
            self.diags.push(diag);
            for f in fields {
                self.check_expr(&f.value, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        };
        let Some(field_names) = variant.field_names.clone() else {
            let mut diag = Diagnostic::new(
                codes::INVALID_PATTERN,
                format!(
                    "variant `{}::{}` uses positional values",
                    enum_name, variant_name
                ),
                name_span,
            );
            diag.help = Some(format!(
                "construct it as `{}::{}(...)`",
                enum_name, variant_name
            ));
            self.diags.push(diag);
            for f in fields {
                self.check_expr(&f.value, None);
            }
            self.error_exprs.insert(id);
            return Type::Int;
        };
        let mut seen: HashMap<String, Span> = HashMap::new();
        let mut constraints = Vec::new();
        let mut had_error = false;
        let partial = Self::partial_bindings(enum_name, &info, expected);
        for f in fields {
            if let Some(prev) = seen.get(&f.name) {
                let mut diag = Diagnostic::new(
                    codes::DUPLICATE_FIELD,
                    format!(
                        "field `{}` is initialized more than once in `{}::{}`",
                        f.name, enum_name, variant_name
                    ),
                    f.name_span,
                );
                diag.notes.push(format!(
                    "the first initialization is at line {}",
                    prev.start.line
                ));
                self.diags.push(diag);
                self.check_expr(&f.value, None);
                had_error = true;
                continue;
            }
            seen.insert(f.name.clone(), f.name_span);
            match field_names.iter().position(|n| *n == f.name) {
                Some(idx) => {
                    let field_ty = variant.fields.get(idx).cloned().unwrap_or(Type::Int);
                    let substituted = substitute_type(&field_ty, &partial);
                    let expected_value = if Self::enum_field_has_param(&substituted) {
                        None
                    } else {
                        Some(substituted.clone())
                    };
                    let vt = self.check_expr(&f.value, expected_value.as_ref());
                    self.require_value(&vt, f.value.span, "field value");
                    Self::collect_constraints(&field_ty, &vt, &mut constraints);
                    self.pending_variant_field_types
                        .push((idx, vt, f.value.span));
                }
                None => {
                    let mut diag = Diagnostic::new(
                        codes::UNKNOWN_FIELD,
                        format!(
                            "variant `{}::{}` has no field `{}`",
                            enum_name, variant_name, f.name
                        ),
                        f.name_span,
                    );
                    diag.help = Some(format!("available fields: {}", field_names.join(", ")));
                    self.diags.push(diag);
                    self.check_expr(&f.value, None);
                    had_error = true;
                }
            }
        }
        for (idx, name) in field_names.iter().enumerate() {
            if !seen.contains_key(name) {
                let mut diag = Diagnostic::new(
                    codes::MISSING_FIELD,
                    format!(
                        "missing field `{}` in `{}::{}` value",
                        name, enum_name, variant_name
                    ),
                    name_span,
                );
                diag.help = Some(format!("add `{}: <value>` to the initializer", name));
                self.diags.push(diag);
                had_error = true;
            }
            let _ = idx;
        }
        let pending = std::mem::take(&mut self.pending_variant_field_types);
        let Some(enum_args) = self.resolve_enum_args(enum_name, expected, &constraints, name_span)
        else {
            self.error_exprs.insert(id);
            return Type::Int;
        };
        let bindings: HashMap<String, Type> = info
            .params
            .iter()
            .cloned()
            .zip(enum_args.iter().cloned())
            .collect();
        for (idx, vt, span) in pending {
            if let Some(field_ty) = variant.fields.get(idx) {
                let concrete = substitute_type(field_ty, &bindings);
                self.coerce(&vt, &concrete, span, "field", codes::TYPE_MISMATCH);
            }
        }
        if had_error {
            self.error_exprs.insert(id);
        }
        self.record_variant_ref(
            id,
            enum_name,
            &enum_args,
            tag,
            variant_name,
            Some(field_names),
            &variant.fields,
        );
        Type::Enum(enum_name.to_string(), enum_args)
    }

    fn arg_count_error(&mut self, name: &str, expected: &str, found: usize, span: Span) {
        let mut diag = Diagnostic::new(
            codes::ARG_COUNT,
            format!(
                "`{}` expects {}, found {} argument{}",
                name,
                expected,
                found,
                if found == 1 { "" } else { "s" }
            ),
            span,
        );
        diag.help = Some(format!("call it as `{}({})`", name, expected));
        self.diags.push(diag);
    }

    fn check_builtin(&mut self, kind: BuiltinKind, name: &str, args: &[Expr], span: Span) -> Type {
        match kind {
            BuiltinKind::Print => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                self.require_printable(&t, args[0].span);
                Type::Void
            }
            BuiltinKind::Println => {
                if args.is_empty() {
                    return Type::Void;
                }
                if args.len() != 1 {
                    self.arg_count_error(name, "0 or 1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                self.require_printable(&t, args[0].span);
                Type::Void
            }
            BuiltinKind::Input => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument (a prompt)", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::String;
                }
                let t = self.check_expr(&args[0], Some(&Type::String));
                self.require_value(&t, args[0].span, "argument");
                self.builtin_arg_type(name, &t, &Type::String, args[0].span);
                Type::String
            }
            BuiltinKind::Len => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Int;
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                match t {
                    Type::String | Type::Array(_) | Type::Slice(_) => Type::Int,
                    other => {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_ARGUMENT,
                            format!(
                                "`len` expects `string`, an array or a slice view, found `{}`",
                                other
                            ),
                            args[0].span,
                        );
                        diag.help = Some(
                            "for strings, `len` returns the number of Unicode scalar values"
                                .to_string(),
                        );
                        self.diags.push(diag);
                        Type::Int
                    }
                }
            }
            BuiltinKind::Str => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::String;
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                if !t.is_basic() {
                    self.error(
                        codes::INVALID_ARGUMENT,
                        format!("`str` cannot convert `{}` to a string", t),
                        args[0].span,
                    );
                }
                Type::String
            }
            BuiltinKind::Int => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Int;
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                match t {
                    Type::Int | Type::Float | Type::Bool | Type::Char | Type::String => Type::Int,
                    Type::Ffi(f) if f.is_integer() || f == crate::types::FfiKind::F32 => Type::Int,
                    other => {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_ARGUMENT,
                            format!("`int` cannot convert `{}`", other),
                            args[0].span,
                        );
                        diag.help = Some(
                            "`int(...)` accepts int, float, bool, char, string or ABI scalars"
                                .to_string(),
                        );
                        self.diags.push(diag);
                        Type::Int
                    }
                }
            }
            BuiltinKind::Float => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Float;
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                match t {
                    Type::Float | Type::Int | Type::String => Type::Float,
                    Type::Ffi(f) if f.is_integer() || f == crate::types::FfiKind::F32 => {
                        Type::Float
                    }
                    other => {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_ARGUMENT,
                            format!("`float` cannot convert `{}`", other),
                            args[0].span,
                        );
                        diag.help = Some(
                            "`float(...)` accepts float, int, string or ABI scalars".to_string(),
                        );
                        self.diags.push(diag);
                        Type::Float
                    }
                }
            }
            BuiltinKind::Char => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Char;
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                if t != Type::Int {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_ARGUMENT,
                        format!("`char` expects an `int` code point, found `{}`", t),
                        args[0].span,
                    );
                    diag.help = Some("e.g. `char(65)` is `'A'`".to_string());
                    self.diags.push(diag);
                }
                Type::Char
            }
            BuiltinKind::Exit => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument (exit code)", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let t = self.check_expr(&args[0], Some(&Type::Int));
                self.require_value(&t, args[0].span, "argument");
                self.builtin_arg_type(name, &t, &Type::Int, args[0].span);
                Type::Void
            }
            BuiltinKind::Panic => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                self.check_string_arg(name, &args[0]);
                Type::Void
            }
            BuiltinKind::Push => {
                if args.len() != 2 {
                    self.arg_count_error(name, "2 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let target_ty = self.check_expr(&args[0], None);
                if self.error_exprs.contains(&args[0].id) {
                    self.check_expr(&args[1], None);
                    return Type::Void;
                }
                self.require_value(&target_ty, args[0].span, "argument");
                let elem = match &target_ty {
                    Type::Array(e) => (**e).clone(),
                    other => {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_ARGUMENT,
                            format!("`push` expects an array, found `{}`", other),
                            args[0].span,
                        );
                        diag.help = Some("e.g. `push(values, 42);`".to_string());
                        self.diags.push(diag);
                        self.check_expr(&args[1], None);
                        return Type::Void;
                    }
                };
                match self.root_ident(&args[0]) {
                    Some(root_expr) => {
                        if let ExprKind::Ident(root_name) = &root_expr.kind {
                            if let Some(id) = self.lookup(root_name) {
                                if let Some(info) = self.var_decls.get(&id) {
                                    if !info.mutable {
                                        let mut diag = Diagnostic::new(
                                            codes::IMMUTABLE,
                                            format!(
                                                "cannot append to `{}`: it was declared immutable",
                                                root_name
                                            ),
                                            args[0].span,
                                        );
                                        diag.help = Some(if root_name == "self" {
                                            "use `var self` as the receiver to allow `push`"
                                                .to_string()
                                        } else {
                                            format!(
                                                "declare it with `var {} = ...;` to allow `push`",
                                                root_name
                                            )
                                        });
                                        self.diags.push(diag);
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_ARGUMENT,
                            "the first argument of `push` must be an array variable or field",
                            args[0].span,
                        );
                        diag.help = Some(
                            "`push` modifies the array in place, so it needs a variable declared with `var`"
                                .to_string(),
                        );
                        self.diags.push(diag);
                    }
                }
                let vt = self.check_expr(&args[1], Some(&elem));
                self.require_value(&vt, args[1].span, "argument");
                self.coerce(&vt, &elem, args[1].span, "argument", codes::TYPE_MISMATCH);
                Type::Void
            }
            BuiltinKind::Pop => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                match self.check_mutable_array_target(name, &args[0]) {
                    Some(elem) => Type::Enum("Option".to_string(), vec![elem]),
                    None => Type::Void,
                }
            }
            BuiltinKind::Insert => {
                if args.len() != 3 {
                    self.arg_count_error(name, "3 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let elem = match self.check_mutable_array_target(name, &args[0]) {
                    Some(e) => e,
                    None => {
                        self.check_expr(&args[1], None);
                        self.check_expr(&args[2], None);
                        return Type::Void;
                    }
                };
                let it = self.check_expr(&args[1], Some(&Type::Int));
                self.require_value(&it, args[1].span, "argument");
                self.builtin_arg_type(name, &it, &Type::Int, args[1].span);
                let vt = self.check_expr(&args[2], Some(&elem));
                self.require_value(&vt, args[2].span, "argument");
                self.coerce(&vt, &elem, args[2].span, "argument", codes::TYPE_MISMATCH);
                Type::Void
            }
            BuiltinKind::Remove => {
                if args.len() != 2 {
                    self.arg_count_error(name, "2 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let elem = match self.check_mutable_array_target(name, &args[0]) {
                    Some(e) => e,
                    None => {
                        self.check_expr(&args[1], None);
                        return Type::Void;
                    }
                };
                let it = self.check_expr(&args[1], Some(&Type::Int));
                self.require_value(&it, args[1].span, "argument");
                self.builtin_arg_type(name, &it, &Type::Int, args[1].span);
                Type::Enum("Option".to_string(), vec![elem])
            }
            BuiltinKind::Slice => {
                if args.len() != 3 {
                    self.arg_count_error(name, "3 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let target_ty = self.check_expr(&args[0], None);
                self.require_value(&target_ty, args[0].span, "argument");
                if !matches!(target_ty, Type::Array(_)) {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_ARGUMENT,
                        format!("`slice` expects an array, found `{}`", target_ty),
                        args[0].span,
                    );
                    diag.help = Some("e.g. `slice(values, 1, 3)`".to_string());
                    self.diags.push(diag);
                    self.check_expr(&args[1], None);
                    self.check_expr(&args[2], None);
                    return Type::Void;
                }
                for arg in &args[1..] {
                    let t = self.check_expr(arg, Some(&Type::Int));
                    self.require_value(&t, arg.span, "argument");
                    self.builtin_arg_type(name, &t, &Type::Int, arg.span);
                }
                target_ty
            }
            BuiltinKind::Join => {
                if args.len() != 2 {
                    self.arg_count_error(name, "2 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::String;
                }
                let target_ty = self.check_expr(&args[0], None);
                self.require_value(&target_ty, args[0].span, "argument");
                let ok = matches!(&target_ty, Type::Array(elem) if **elem == Type::String);
                if !ok {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_ARGUMENT,
                        format!("`join` expects an array of strings, found `{}`", target_ty),
                        args[0].span,
                    );
                    diag.help = Some("e.g. `join(words, \", \")`".to_string());
                    self.diags.push(diag);
                    self.check_expr(&args[1], None);
                    return Type::String;
                }
                self.check_string_arg(name, &args[1]);
                Type::String
            }
            BuiltinKind::SliceView => {
                if args.len() != 3 {
                    self.arg_count_error(name, "3 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Void;
                }
                let target_ty = self.check_expr(&args[0], None);
                self.require_value(&target_ty, args[0].span, "argument");
                let elem = match &target_ty {
                    Type::Array(e) => (**e).clone(),
                    other => {
                        let mut diag = Diagnostic::new(
                            codes::INVALID_ARGUMENT,
                            format!("`slice_view` expects an array, found `{}`", other),
                            args[0].span,
                        );
                        diag.help = Some(
                            "slice views are created from arrays: `slice_view(values, 1, 3)`"
                                .to_string(),
                        );
                        self.diags.push(diag);
                        self.check_expr(&args[1], None);
                        self.check_expr(&args[2], None);
                        return Type::Void;
                    }
                };
                for arg in &args[1..] {
                    let t = self.check_expr(arg, Some(&Type::Int));
                    self.require_value(&t, arg.span, "argument");
                    self.builtin_arg_type(name, &t, &Type::Int, arg.span);
                }
                Type::Slice(Box::new(elem))
            }
            BuiltinKind::RawPtrNull => {
                if !args.is_empty() {
                    self.arg_count_error(name, "0 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                }
                Type::Ffi(crate::types::FfiKind::RawPtr)
            }
            BuiltinKind::CIntConv
            | BuiltinKind::CUIntConv
            | BuiltinKind::CLongConv
            | BuiltinKind::CULongConv
            | BuiltinKind::CLongLongConv
            | BuiltinKind::CULongLongConv
            | BuiltinKind::F32Conv => {
                let kind = match kind {
                    BuiltinKind::CIntConv => crate::types::FfiKind::CInt,
                    BuiltinKind::CUIntConv => crate::types::FfiKind::CUInt,
                    BuiltinKind::CLongConv => crate::types::FfiKind::CLong,
                    BuiltinKind::CULongConv => crate::types::FfiKind::CULong,
                    BuiltinKind::CLongLongConv => crate::types::FfiKind::CLongLong,
                    BuiltinKind::CULongLongConv => crate::types::FfiKind::CULongLong,
                    _ => crate::types::FfiKind::F32,
                };
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Ffi(kind);
                }
                let t = self.check_expr(&args[0], None);
                self.require_value(&t, args[0].span, "argument");
                let ok = match &t {
                    Type::Int | Type::Float => true,
                    Type::Ffi(f) => match kind {
                        crate::types::FfiKind::F32 => true,
                        _ => f.is_integer(),
                    },
                    _ => false,
                };
                if !ok {
                    let mut diag = Diagnostic::new(
                        codes::INVALID_FFI_TYPE,
                        format!("`{}` cannot convert a value of type `{}`", name, t),
                        args[0].span,
                    );
                    diag.help = Some(
                        "convert numbers (`int`, `float`) or other ABI scalars explicitly"
                            .to_string(),
                    );
                    self.diags.push(diag);
                }
                Type::Ffi(kind)
            }
            BuiltinKind::Contains => {
                if args.len() != 2 {
                    self.arg_count_error(name, "2 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Bool;
                }
                self.check_string_arg(name, &args[0]);
                self.check_string_arg(name, &args[1]);
                Type::Bool
            }
            BuiltinKind::StartsWith => {
                if args.len() != 2 {
                    self.arg_count_error(name, "2 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Bool;
                }
                self.check_string_arg(name, &args[0]);
                self.check_string_arg(name, &args[1]);
                Type::Bool
            }
            BuiltinKind::EndsWith => {
                if args.len() != 2 {
                    self.arg_count_error(name, "2 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Bool;
                }
                self.check_string_arg(name, &args[0]);
                self.check_string_arg(name, &args[1]);
                Type::Bool
            }
            BuiltinKind::Trim | BuiltinKind::ToUpper | BuiltinKind::ToLower => {
                if args.len() != 1 {
                    self.arg_count_error(name, "1 argument", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::String;
                }
                self.check_string_arg(name, &args[0]);
                Type::String
            }
            BuiltinKind::Replace => {
                if args.len() != 3 {
                    self.arg_count_error(name, "3 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::String;
                }
                for arg in &args[..3] {
                    self.check_string_arg(name, arg);
                }
                Type::String
            }
            BuiltinKind::Split => {
                if args.len() != 2 {
                    self.arg_count_error(name, "2 arguments", args.len(), span);
                    for arg in args {
                        self.check_expr(arg, None);
                    }
                    return Type::Array(Box::new(Type::String));
                }
                self.check_string_arg(name, &args[0]);
                self.check_string_arg(name, &args[1]);
                Type::Array(Box::new(Type::String))
            }
        }
    }

    fn check_string_arg(&mut self, name: &str, arg: &Expr) {
        let t = self.check_expr(arg, Some(&Type::String));
        self.require_value(&t, arg.span, "argument");
        self.builtin_arg_type(name, &t, &Type::String, arg.span);
    }

    fn check_mutable_array_target(&mut self, name: &str, arg: &Expr) -> Option<Type> {
        let target_ty = self.check_expr(arg, None);
        if self.error_exprs.contains(&arg.id) {
            return None;
        }
        self.require_value(&target_ty, arg.span, "argument");
        let elem = match &target_ty {
            Type::Array(e) => (**e).clone(),
            other => {
                let mut diag = Diagnostic::new(
                    codes::INVALID_ARGUMENT,
                    format!("`{}` expects an array, found `{}`", name, other),
                    arg.span,
                );
                diag.help = Some(format!(
                    "`{}` modifies the array in place, so it needs an array variable",
                    name
                ));
                self.diags.push(diag);
                return None;
            }
        };
        match self.root_ident(arg) {
            Some(root_expr) => {
                if let ExprKind::Ident(root_name) = &root_expr.kind {
                    if let Some(id) = self.lookup(root_name) {
                        if let Some(info) = self.var_decls.get(&id) {
                            if !info.mutable {
                                let mut diag = Diagnostic::new(
                                    codes::IMMUTABLE,
                                    format!(
                                        "cannot modify `{}`: it was declared immutable",
                                        root_name
                                    ),
                                    arg.span,
                                );
                                diag.help = Some(if root_name == "self" {
                                    "use `var self` as the receiver to allow mutation".to_string()
                                } else {
                                    format!(
                                        "declare it with `var {} = ...;` to allow mutation",
                                        root_name
                                    )
                                });
                                self.diags.push(diag);
                            }
                        }
                    }
                }
            }
            None => {
                let mut diag = Diagnostic::new(
                    codes::INVALID_ARGUMENT,
                    format!(
                        "the first argument of `{}` must be an array variable or field",
                        name
                    ),
                    arg.span,
                );
                diag.help = Some(format!(
                    "`{}` modifies the array in place, so it needs a variable declared with `var`",
                    name
                ));
                self.diags.push(diag);
            }
        }
        Some(elem)
    }

    fn require_printable(&mut self, ty: &Type, span: Span) {
        if !ty.is_basic() {
            let mut diag = Diagnostic::new(
                codes::INVALID_ARGUMENT,
                format!("cannot print a value of type `{}`", ty),
                span,
            );
            diag.help = Some(match ty {
                Type::Struct(_, _) => {
                    "print the individual fields instead, e.g. `println(p.x);`".to_string()
                }
                Type::Enum(_, _) => {
                    "match on the value and print each variant's payload instead".to_string()
                }
                Type::Array(_) => {
                    "print supports int, float, bool, string and char; loop over the array instead"
                        .to_string()
                }
                _ => "print supports int, float, bool, string and char".to_string(),
            });
            self.diags.push(diag);
        }
    }

    fn builtin_arg_type(&mut self, name: &str, found: &Type, expected: &Type, span: Span) {
        if found != expected {
            let mut diag = Diagnostic::new(
                codes::INVALID_ARGUMENT,
                format!("`{}` expects `{}` here, found `{}`", name, expected, found),
                span,
            );
            diag.help = Some(format!("pass a `{}` value", expected));
            self.diags.push(diag);
        }
    }

    #[allow(clippy::result_large_err)]
    fn eval_const(&mut self, expr: &Expr) -> Result<ConstValue, Diagnostic> {
        match &expr.kind {
            ExprKind::Int(v) => Ok(ConstValue::Int(*v)),
            ExprKind::Float(v) => Ok(ConstValue::Float(*v)),
            ExprKind::Bool(v) => Ok(ConstValue::Bool(*v)),
            ExprKind::Str(v) => Ok(ConstValue::Str(v.clone())),
            ExprKind::Char(v) => Ok(ConstValue::Char(*v)),
            ExprKind::Ident(name) => {
                if let Some(id) = self.lookup(name) {
                    if let Some(value) = self.consts.get(&id) {
                        return Ok(value.clone());
                    }
                    let mut diag = Diagnostic::new(
                        codes::NOT_CONSTANT,
                        format!("`{}` is not a compile-time constant", name),
                        expr.span,
                    );
                    diag.help =
                        Some("only `const` declarations and literals can be used here".to_string());
                    return Err(diag);
                }
                Err(Diagnostic::new(
                    codes::UNDEFINED_VARIABLE,
                    format!("cannot find `{}` in this scope", name),
                    expr.span,
                ))
            }
            ExprKind::Unary { op, operand } => {
                let v = self.eval_const(operand)?;
                match (op, v) {
                    (UnaryOp::Neg, ConstValue::Int(i)) => Ok(ConstValue::Int(i.wrapping_neg())),
                    (UnaryOp::Neg, ConstValue::Float(f)) => Ok(ConstValue::Float(-f)),
                    (UnaryOp::Not, ConstValue::Bool(b)) => Ok(ConstValue::Bool(!b)),
                    (op, other) => Err(Diagnostic::new(
                        codes::NOT_CONSTANT,
                        format!(
                            "operator `{}` cannot be applied to constant `{:?}`",
                            match op {
                                UnaryOp::Neg => "-",
                                UnaryOp::Not => "!",
                            },
                            other
                        ),
                        expr.span,
                    )),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lv = self.eval_const(lhs)?;
                let rv = self.eval_const(rhs)?;
                self.eval_const_binary(*op, lv, rv, expr.span)
            }
            _ => {
                let mut diag = Diagnostic::new(
                    codes::NOT_CONSTANT,
                    "this expression is not allowed in a constant declaration",
                    expr.span,
                );
                diag.help = Some(
                    "constant expressions support literals, other constants, and arithmetic/logic operators"
                        .to_string(),
                );
                Err(diag)
            }
        }
    }

    #[allow(clippy::result_large_err)]
    fn eval_const_binary(
        &mut self,
        op: BinOp,
        lv: ConstValue,
        rv: ConstValue,
        span: Span,
    ) -> Result<ConstValue, Diagnostic> {
        use BinOp::*;
        match (lv, rv) {
            (ConstValue::Int(a), ConstValue::Int(b)) => match op {
                Add => Ok(ConstValue::Int(a.wrapping_add(b))),
                Sub => Ok(ConstValue::Int(a.wrapping_sub(b))),
                Mul => Ok(ConstValue::Int(a.wrapping_mul(b))),
                Div => {
                    if b == 0 {
                        Err(Diagnostic::new(
                            codes::NOT_CONSTANT,
                            "constant evaluation: integer division by zero",
                            span,
                        ))
                    } else if a == i64::MIN && b == -1 {
                        Ok(ConstValue::Int(i64::MIN))
                    } else {
                        Ok(ConstValue::Int(a / b))
                    }
                }
                Mod => {
                    if b == 0 {
                        Err(Diagnostic::new(
                            codes::NOT_CONSTANT,
                            "constant evaluation: modulo by zero",
                            span,
                        ))
                    } else if a == i64::MIN && b == -1 {
                        Ok(ConstValue::Int(0))
                    } else {
                        Ok(ConstValue::Int(a % b))
                    }
                }
                Eq => Ok(ConstValue::Bool(a == b)),
                Ne => Ok(ConstValue::Bool(a != b)),
                Lt => Ok(ConstValue::Bool(a < b)),
                Le => Ok(ConstValue::Bool(a <= b)),
                Gt => Ok(ConstValue::Bool(a > b)),
                Ge => Ok(ConstValue::Bool(a >= b)),
                And | Or => Err(Diagnostic::new(
                    codes::NOT_CONSTANT,
                    "logical operators need `bool` operands in constant expressions",
                    span,
                )),
            },
            (ConstValue::Float(a), ConstValue::Float(b)) => match op {
                Add => Ok(ConstValue::Float(a + b)),
                Sub => Ok(ConstValue::Float(a - b)),
                Mul => Ok(ConstValue::Float(a * b)),
                Div => Ok(ConstValue::Float(a / b)),
                Mod => Err(Diagnostic::new(
                    codes::NOT_CONSTANT,
                    "modulo is not supported for `float`",
                    span,
                )),
                Eq => Ok(ConstValue::Bool(a == b)),
                Ne => Ok(ConstValue::Bool(a != b)),
                Lt => Ok(ConstValue::Bool(a < b)),
                Le => Ok(ConstValue::Bool(a <= b)),
                Gt => Ok(ConstValue::Bool(a > b)),
                Ge => Ok(ConstValue::Bool(a >= b)),
                And | Or => Err(Diagnostic::new(
                    codes::NOT_CONSTANT,
                    "logical operators need `bool` operands in constant expressions",
                    span,
                )),
            },
            (ConstValue::Int(a), ConstValue::Float(b)) => {
                self.eval_const_binary(op, ConstValue::Float(a as f64), ConstValue::Float(b), span)
            }
            (ConstValue::Float(a), ConstValue::Int(b)) => {
                self.eval_const_binary(op, ConstValue::Float(a), ConstValue::Float(b as f64), span)
            }
            (ConstValue::Bool(a), ConstValue::Bool(b)) => match op {
                And => Ok(ConstValue::Bool(a && b)),
                Or => Ok(ConstValue::Bool(a || b)),
                Eq => Ok(ConstValue::Bool(a == b)),
                Ne => Ok(ConstValue::Bool(a != b)),
                _ => Err(Diagnostic::new(
                    codes::NOT_CONSTANT,
                    "unsupported boolean operator in constant expression",
                    span,
                )),
            },
            (ConstValue::Str(a), ConstValue::Str(b)) => match op {
                Add => Ok(ConstValue::Str(format!("{}{}", a, b))),
                Eq => Ok(ConstValue::Bool(a == b)),
                Ne => Ok(ConstValue::Bool(a != b)),
                _ => Err(Diagnostic::new(
                    codes::NOT_CONSTANT,
                    "unsupported string operator in constant expression",
                    span,
                )),
            },
            (ConstValue::Char(a), ConstValue::Char(b)) => match op {
                Eq => Ok(ConstValue::Bool(a == b)),
                Ne => Ok(ConstValue::Bool(a != b)),
                Lt => Ok(ConstValue::Bool(a < b)),
                Le => Ok(ConstValue::Bool(a <= b)),
                Gt => Ok(ConstValue::Bool(a > b)),
                Ge => Ok(ConstValue::Bool(a >= b)),
                _ => Err(Diagnostic::new(
                    codes::NOT_CONSTANT,
                    "unsupported char operator in constant expression",
                    span,
                )),
            },
            (a, b) => Err(Diagnostic::new(
                codes::NOT_CONSTANT,
                format!(
                    "type mismatch in constant expression: `{}` and `{}`",
                    a.ty(),
                    b.ty()
                ),
                span,
            )),
        }
    }
}

fn is_basic_type_name(name: &str) -> bool {
    matches!(name, "int" | "float" | "bool" | "string" | "char" | "void")
}

fn pattern_span_of(pat: &Pattern) -> Span {
    match pat {
        Pattern::Wild(span) => *span,
        Pattern::Binding(_, span, _) => *span,
        Pattern::Literal(_, span) => *span,
        Pattern::Or(_, span) => *span,
        Pattern::Variant { span, .. } => *span,
    }
}

fn collect_call_ids(block: &Block, out: &mut Vec<ExprId>) {
    for stmt in &block.stmts {
        collect_call_ids_stmt(stmt, out);
    }
}

fn collect_call_ids_stmt(stmt: &Stmt, out: &mut Vec<ExprId>) {
    match stmt {
        Stmt::VarDecl(d) => {
            if let Some(init) = &d.init {
                collect_call_ids_expr(init, out);
            }
        }
        Stmt::Expr { expr, .. } => collect_call_ids_expr(expr, out),
        Stmt::If {
            cond,
            then_block,
            else_branch,
            ..
        } => {
            collect_call_ids_expr(cond, out);
            collect_call_ids(then_block, out);
            match else_branch {
                Some(ElseBranch::If(s)) => collect_call_ids_stmt(s, out),
                Some(ElseBranch::Block(b)) => collect_call_ids(b, out),
                None => {}
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_call_ids_expr(cond, out);
            collect_call_ids(body, out);
        }
        Stmt::For {
            range_start,
            range_end,
            body,
            ..
        } => {
            collect_call_ids_expr(range_start, out);
            collect_call_ids_expr(range_end, out);
            collect_call_ids(body, out);
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                collect_call_ids_expr(v, out);
            }
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => collect_call_ids(b, out),
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
    }
}

fn collect_call_ids_expr(expr: &Expr, out: &mut Vec<ExprId>) {
    match &expr.kind {
        ExprKind::Call { callee, args, .. } => {
            out.push(expr.id);
            collect_call_ids_expr(callee, out);
            for a in args {
                collect_call_ids_expr(a, out);
            }
        }
        ExprKind::Unary { operand, .. } => collect_call_ids_expr(operand, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_call_ids_expr(lhs, out);
            collect_call_ids_expr(rhs, out);
        }
        ExprKind::Assign { target, value } => {
            collect_call_ids_expr(target, out);
            collect_call_ids_expr(value, out);
        }
        ExprKind::Index { base, index } => {
            collect_call_ids_expr(base, out);
            collect_call_ids_expr(index, out);
        }
        ExprKind::Field { base, .. } => collect_call_ids_expr(base, out),
        ExprKind::StructLit { fields, .. } => {
            for f in fields {
                collect_call_ids_expr(&f.value, out);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_call_ids_expr(scrutinee, out);
            for arm in arms {
                collect_call_ids_expr(&arm.body, out);
            }
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                collect_call_ids_expr(e, out);
            }
        }
        ExprKind::Try(operand) => collect_call_ids_expr(operand, out),
        _ => {}
    }
}

fn collect_nested_types(ty: &Type, out: &mut Vec<Type>) {
    match ty {
        Type::Enum(_, _) | Type::Struct(_, _) => out.push(ty.clone()),
        Type::Array(inner) => collect_nested_types(inner, out),
        _ => {}
    }
}

fn topo_instantiations(
    sources: &[Type],
    structs: &HashMap<String, StructInfo>,
    enums: &HashMap<String, EnumInfo>,
) -> (Vec<Type>, Vec<Diagnostic>) {
    fn visit(
        ty: &Type,
        structs: &HashMap<String, StructInfo>,
        enums: &HashMap<String, EnumInfo>,
        state: &mut HashMap<Type, u8>,
        ordered: &mut Vec<Type>,
        errors: &mut Vec<Diagnostic>,
        steps: &mut usize,
    ) {
        if ty.contains_param() {
            return;
        }
        match state.get(ty) {
            Some(2) => return,
            Some(1) => {
                return;
            }
            _ => {}
        }
        *steps += 1;
        if *steps > 512 {
            if errors.len() < 8 {
                errors.push(Diagnostic::new(
                    codes::RECURSIVE_ENUM,
                    "too many type instantiations (recursive generic type?)",
                    Span::point(1, 1),
                ));
            }
            return;
        }
        state.insert(ty.clone(), 1);
        match ty {
            Type::Enum(name, args) => {
                if let Some(info) = enums.get(name) {
                    let bindings: HashMap<String, Type> = info
                        .params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    for v in &info.variants {
                        for f in &v.fields {
                            let ft = substitute_type(f, &bindings);
                            let mut deps = Vec::new();
                            collect_nested_types(&ft, &mut deps);
                            for d in deps {
                                visit(&d, structs, enums, state, ordered, errors, steps);
                            }
                        }
                    }
                }
            }
            Type::Struct(name, args) => {
                if let Some(info) = structs.get(name) {
                    let bindings: HashMap<String, Type> = info
                        .params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    for f in &info.fields {
                        let ft = substitute_type(&f.ty, &bindings);
                        let mut deps = Vec::new();
                        collect_nested_types(&ft, &mut deps);
                        for d in deps {
                            visit(&d, structs, enums, state, ordered, errors, steps);
                        }
                    }
                }
            }
            _ => {}
        }
        state.insert(ty.clone(), 2);
        ordered.push(ty.clone());
    }

    let mut state: HashMap<Type, u8> = HashMap::new();
    let mut ordered = Vec::new();
    let mut errors = Vec::new();
    let mut steps = 0usize;
    for s in sources {
        let mut deps = Vec::new();
        collect_nested_types(s, &mut deps);
        for d in deps {
            visit(
                &d,
                structs,
                enums,
                &mut state,
                &mut ordered,
                &mut errors,
                &mut steps,
            );
        }
    }
    (ordered, errors)
}

fn collect_named_types(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Struct(name, args) => {
            out.push(name.clone());
            for a in args {
                collect_named_types(a, out);
            }
        }
        Type::Enum(name, args) => {
            out.push(name.clone());
            for a in args {
                collect_named_types(a, out);
            }
        }
        Type::Array(inner) => collect_named_types(inner, out),
        _ => {}
    }
}

fn find_cycles_in_graph(graph: &HashMap<String, Vec<String>>) -> Vec<String> {
    fn visit(
        name: &str,
        graph: &HashMap<String, Vec<String>>,
        state: &mut HashMap<String, u8>,
        stack: &mut Vec<String>,
        cyclic: &mut HashSet<String>,
    ) {
        match state.get(name) {
            Some(2) => return,
            Some(1) => {
                if let Some(pos) = stack.iter().position(|n| n == name) {
                    for n in &stack[pos..] {
                        cyclic.insert(n.clone());
                    }
                }
                cyclic.insert(name.to_string());
                return;
            }
            _ => {}
        }
        state.insert(name.to_string(), 1);
        stack.push(name.to_string());
        if let Some(deps) = graph.get(name) {
            for dep in deps {
                if graph.contains_key(dep) {
                    visit(dep, graph, state, stack, cyclic);
                }
            }
        }
        stack.pop();
        state.insert(name.to_string(), 2);
    }

    let mut state: HashMap<String, u8> = HashMap::new();
    let mut cyclic: HashSet<String> = HashSet::new();
    for name in graph.keys() {
        let mut stack = Vec::new();
        visit(name, graph, &mut state, &mut stack, &mut cyclic);
    }
    let mut result: Vec<String> = cyclic.into_iter().collect();
    result.sort();
    result
}

fn always_returns(block: &Block) -> bool {
    block.stmts.iter().any(stmt_always_returns)
}

fn stmt_always_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::Expr { expr, .. } => {
            matches!(&expr.kind, ExprKind::Call { callee, .. } if is_panic_callee(callee))
        }
        Stmt::If {
            then_block,
            else_branch: Some(else_branch),
            ..
        } => {
            let else_returns = match else_branch {
                ElseBranch::If(s) => stmt_always_returns(s),
                ElseBranch::Block(b) => always_returns(b),
            };
            else_returns && always_returns(then_block)
        }
        Stmt::While { cond, body, .. } => {
            matches!(cond.kind, ExprKind::Bool(true)) && !block_contains_break(body)
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => always_returns(b),
        _ => false,
    }
}

fn is_panic_callee(callee: &Expr) -> bool {
    match &callee.kind {
        ExprKind::Ident(name) => name == "panic",
        ExprKind::Path(segments, type_args) => {
            segments.len() == 1 && segments[0] == "panic" && type_args.is_empty()
        }
        _ => false,
    }
}

fn block_contains_break(block: &Block) -> bool {
    block.stmts.iter().any(stmt_contains_break)
}

fn stmt_contains_break(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Break { .. } => true,
        Stmt::If {
            then_block,
            else_branch,
            ..
        } => {
            if block_contains_break(then_block) {
                return true;
            }
            match else_branch {
                Some(ElseBranch::If(s)) => stmt_contains_break(s),
                Some(ElseBranch::Block(b)) => block_contains_break(b),
                None => false,
            }
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => block_contains_break(b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn check_src(src: &str) -> Result<CheckedProgram, Vec<Diagnostic>> {
        let tokens = lex(src).tokens;
        let out = parse(tokens);
        assert!(
            out.diagnostics.is_empty(),
            "parser errors: {:?}",
            out.diagnostics
        );
        check(out.program)
    }

    fn errors(src: &str) -> Vec<Diagnostic> {
        match check_src(src) {
            Ok(_) => panic!("expected errors, got Ok"),
            Err(d) => d,
        }
    }

    #[test]
    fn accepts_valid_program() {
        let program = check_src(
            r#"
fn add(a: int, b: int) -> int {
    return a + b;
}

fn main() -> int {
    let x = add(1, 2);
    var y: float = 1.5;
    y = y + x;
    println(str(y));
    if x > 0 {
        println("positive");
    } else {
        println("not positive");
    }
    for i in 0..3 {
        println(i);
    }
    while false {
        break;
    }
    return 0;
}
"#,
        )
        .expect("should typecheck");
        assert_eq!(program.functions.len(), 2);
    }

    #[test]
    fn detects_type_mismatch() {
        let diags = errors("fn main() -> int { let age: int = \"hello\"; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn detects_undefined_variable() {
        let diags = errors("fn main() -> int { println(missing); return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNDEFINED_VARIABLE));
    }

    #[test]
    fn detects_immutable_assignment() {
        let diags = errors("fn main() -> int { let x = 1; x = 2; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::IMMUTABLE));
    }

    #[test]
    fn allows_var_assignment() {
        check_src("fn main() -> int { var x = 1; x = 2; return x; }").expect("ok");
    }

    #[test]
    fn detects_wrong_arg_count() {
        let diags = errors(
            "fn add(a: int, b: int) -> int { return a + b; }\nfn main() -> int { return add(1); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::ARG_COUNT));
    }

    #[test]
    fn detects_wrong_arg_type() {
        let diags = errors(
            "fn add(a: int, b: int) -> int { return a + b; }\nfn main() -> int { return add(1, \"x\"); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn detects_missing_return() {
        let diags = errors("fn f() -> int { let x = 1; }\nfn main() -> int { return f(); }");
        assert!(diags.iter().any(|d| d.code == codes::MISSING_RETURN));
    }

    #[test]
    fn accepts_always_returning_via_else() {
        check_src("fn f(x: bool) -> int { if x { return 1; } else { return 2; } }\nfn main() -> int { return f(true); }")
            .expect("ok");
    }

    #[test]
    fn detects_non_bool_condition() {
        let diags = errors("fn main() -> int { if 1 { return 0; } return 1; }");
        assert!(diags.iter().any(|d| d.code == codes::CONDITION_NOT_BOOL));
    }

    #[test]
    fn detects_break_outside_loop() {
        let diags = errors("fn main() -> int { break; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::BREAK_OUTSIDE_LOOP));
    }

    #[test]
    fn allows_shadowing() {
        check_src(
            "fn main() -> int { let x = 1; { let x = 2; println(x); } println(x); return 0; }",
        )
        .expect("ok");
    }

    #[test]
    fn detects_duplicate_declaration() {
        let diags = errors("fn main() -> int { let x = 1; let x = 2; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_DECL));
    }

    #[test]
    fn detects_missing_main() {
        let diags = errors("fn other() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::MISSING_MAIN));
    }

    #[test]
    fn detects_bad_main_signature() {
        let diags = errors("fn main() -> void { }");
        assert!(diags.iter().any(|d| d.code == codes::BAD_MAIN));
    }

    #[test]
    fn constant_folding_works() {
        let program = check_src(
            "const MAX_ITEMS: int = 50;\nconst TOTAL = MAX_ITEMS * 2 + 1;\nfn main() -> int { return TOTAL; }",
        )
        .expect("ok");
        assert_eq!(program.globals.len(), 2);
        assert_eq!(program.globals[1].value, ConstValue::Int(101));
    }

    #[test]
    fn rejects_non_constant_const() {
        let diags = errors("fn main() -> int { let x = 1; const Y = x; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::NOT_CONSTANT));
    }

    #[test]
    fn array_literal_infers_type() {
        let program =
            check_src("fn main() -> int { let xs = [1, 2, 3]; return len(xs); }").expect("ok");
        assert_eq!(program.functions.len(), 1);
    }

    #[test]
    fn rejects_mixed_array() {
        let diags = errors("fn main() -> int { let xs = [1, \"two\"]; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn rejects_empty_array_without_annotation() {
        let diags = errors("fn main() -> int { let xs = []; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn accepts_empty_array_with_annotation() {
        check_src("fn main() -> int { let xs: [int] = []; return len(xs); }").expect("ok");
    }

    #[test]
    fn indexing_string_gives_char() {
        check_src("fn main() -> int { let s = \"abc\"; let c: char = s[0]; return 0; }")
            .expect("ok");
    }

    #[test]
    fn detects_string_ordering() {
        let diags = errors("fn main() -> int { let a = \"x\" < \"y\"; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_OPERANDS));
    }

    #[test]
    fn detects_calling_non_function() {
        let diags = errors("fn main() -> int { let x = 1; return x(); }");
        assert!(diags.iter().any(|d| d.code == codes::NOT_CALLABLE));
    }

    #[test]
    fn detects_void_in_expression() {
        let diags = errors("fn main() -> int { let x = println(\"hi\"); return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn recursion_is_allowed() {
        check_src(
            "fn fact(n: int) -> int { if n <= 1 { return 1; } return n * fact(n - 1); }\nfn main() -> int { return fact(5); }",
        )
        .expect("ok");
    }

    #[test]
    fn widening_int_to_float_allowed() {
        check_src("fn main() -> int { var f: float = 1; f = 2; return int(f); }").expect("ok");
    }

    #[test]
    fn cannot_assign_float_to_int() {
        let diags = errors("fn main() -> int { var x: int = 1; x = 2.5; return x; }");
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn modulo_float_rejected() {
        let diags = errors("fn main() -> int { let x = 1.5 % 1.0; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_OPERANDS));
    }

    #[test]
    fn global_constant_usable_in_function() {
        check_src("const GREETING = \"hi\";\nfn main() -> int { println(GREETING); return 0; }")
            .expect("ok");
    }

    #[test]
    fn duplicate_function_rejected() {
        let diags = errors("fn f() -> int { return 1; }\nfn f() -> int { return 2; }\nfn main() -> int { return f(); }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_DECL));
    }

    #[test]
    fn redefining_builtin_rejected() {
        let diags = errors("fn println(x: int) -> void { }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_DECL));
    }

    #[test]
    fn string_concat_ok() {
        check_src("fn main() -> int { let s = \"a\" + \"b\"; println(s); return 0; }").expect("ok");
    }

    #[test]
    fn string_plus_int_rejected() {
        let diags = errors("fn main() -> int { let s = \"a\" + 1; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_OPERANDS));
    }

    #[test]
    fn accepts_struct_declaration_and_literal() {
        check_src(
            "struct Point {\n    x: int,\n    y: int\n}\nfn main() -> int {\n    let p = Point { x: 1, y: 2 };\n    println(p.x);\n    return 0;\n}",
        )
        .expect("should typecheck");
    }

    #[test]
    fn field_access_has_field_type() {
        let program = check_src(
            "struct P {\n    n: int,\n    f: float,\n    s: string\n}\nfn main() -> int {\n    let p = P { n: 1, f: 2, s: \"x\" };\n    let a: int = p.n;\n    let b: float = p.f;\n    let c: string = p.s;\n    return a;\n}",
        )
        .expect("ok");
        assert_eq!(program.structs.len(), 1);
    }

    #[test]
    fn unknown_field_access_rejected() {
        let diags =
            errors("struct P { n: int }\nfn main() -> int { let p = P { n: 1 }; return p.z; }");
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_FIELD));
    }

    #[test]
    fn unknown_field_in_literal_rejected() {
        let diags =
            errors("struct P { n: int }\nfn main() -> int { let p = P { n: 1, z: 2 }; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_FIELD));
    }

    #[test]
    fn missing_field_rejected() {
        let diags = errors(
            "struct P {\n    x: int,\n    y: int\n}\nfn main() -> int { let p = P { x: 1 }; return 0; }",
        );
        let missing: Vec<_> = diags
            .iter()
            .filter(|d| d.code == codes::MISSING_FIELD)
            .collect();
        assert_eq!(missing.len(), 1);
        assert!(missing[0].message.contains("`y`"));
    }

    #[test]
    fn duplicate_field_in_literal_rejected() {
        let diags =
            errors("struct P { n: int }\nfn main() -> int { let p = P { n: 1, n: 2 }; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_FIELD));
    }

    #[test]
    fn wrong_field_type_rejected() {
        let diags =
            errors("struct P { n: int }\nfn main() -> int { let p = P { n: \"x\" }; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn int_literal_widens_to_float_field() {
        check_src(
            "struct P { f: float }\nfn main() -> int { let p = P { f: 1 }; return int(p.f); }",
        )
        .expect("ok");
    }

    #[test]
    fn unknown_struct_in_literal_rejected() {
        let diags = errors("fn main() -> int { let p = Missing { x: 1 }; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_TYPE));
    }

    #[test]
    fn field_access_on_non_struct_rejected() {
        let diags = errors("fn main() -> int { let n = 1; return n.x; }");
        assert!(diags.iter().any(|d| d.code == codes::NOT_A_STRUCT));
    }

    #[test]
    fn field_access_on_undefined_variable_reports_once() {
        let diags = errors("fn main() -> int { return unknown.x; }");
        assert!(diags.iter().any(|d| d.code == codes::UNDEFINED_VARIABLE));
        assert!(!diags.iter().any(|d| d.code == codes::NOT_A_STRUCT));
    }

    #[test]
    fn immutable_field_assignment_rejected() {
        let diags = errors(
            "struct P { x: int }\nfn main() -> int { let p = P { x: 1 }; p.x = 2; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMMUTABLE));
    }

    #[test]
    fn var_field_assignment_allowed() {
        check_src(
            "struct P { x: int }\nfn main() -> int { var p = P { x: 1 }; p.x = 2; return p.x; }",
        )
        .expect("ok");
    }

    #[test]
    fn nested_field_assignment_allowed_for_var() {
        check_src(
            "struct I { v: int }\nstruct O { i: I }\nfn main() -> int { var o = O { i: I { v: 1 } }; o.i.v = 5; return o.i.v; }",
        )
        .expect("ok");
    }

    #[test]
    fn nested_field_assignment_rejected_for_let() {
        let diags = errors(
            "struct I { v: int }\nstruct O { i: I }\nfn main() -> int { let o = O { i: I { v: 1 } }; o.i.v = 5; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMMUTABLE));
    }

    #[test]
    fn field_assignment_wrong_type_rejected() {
        let diags = errors(
            "struct P { x: int }\nfn main() -> int { var p = P { x: 1 }; p.x = \"no\"; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn field_assignment_unknown_field_rejected() {
        let diags = errors(
            "struct P { x: int }\nfn main() -> int { var p = P { x: 1 }; p.z = 2; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_FIELD));
    }

    #[test]
    fn field_assignment_on_non_struct_rejected() {
        let diags = errors("fn main() -> int { var n = 1; n.x = 2; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::NOT_A_STRUCT));
    }

    #[test]
    fn struct_as_parameter_and_return_type() {
        check_src(
            "struct P { x: int }\nfn make(x: int) -> P { return P { x: x }; }\nfn get(p: P) -> int { return p.x; }\nfn main() -> int { return get(make(3)); }",
        )
        .expect("ok");
    }

    #[test]
    fn passing_wrong_struct_type_rejected() {
        let diags = errors(
            "struct A { x: int }\nstruct B { x: int }\nfn get(a: A) -> int { return a.x; }\nfn main() -> int { return get(B { x: 1 }); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn returning_wrong_struct_type_rejected() {
        let diags = errors(
            "struct A { x: int }\nstruct B { x: int }\nfn make() -> A { return B { x: 1 }; }\nfn main() -> int { return make().x; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn duplicate_struct_rejected() {
        let diags =
            errors("struct P { x: int }\nstruct P { y: int }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_DECL));
    }

    #[test]
    fn duplicate_field_declaration_rejected() {
        let diags = errors("struct P { x: int, x: int }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_DECL));
    }

    #[test]
    fn recursive_struct_rejected() {
        let diags = errors("struct Node { next: Node }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::RECURSIVE_STRUCT));
    }

    #[test]
    fn indirect_recursive_struct_rejected() {
        let diags = errors("struct A { b: B }\nstruct B { a: A }\nfn main() -> int { return 0; }");
        let rec: Vec<_> = diags
            .iter()
            .filter(|d| d.code == codes::RECURSIVE_STRUCT)
            .collect();
        assert_eq!(rec.len(), 2);
    }

    #[test]
    fn array_of_structs_rejected() {
        let diags = errors("struct P { x: int }\nfn main() -> int { let a: [P] = []; return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNSUPPORTED));
    }

    #[test]
    fn struct_array_literal_rejected() {
        let diags = errors(
            "struct P { x: int }\nfn main() -> int { let a = [P { x: 1 }, P { x: 2 }]; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNSUPPORTED));
    }

    #[test]
    fn struct_field_of_array_type_allowed() {
        check_src(
            "struct Bag { items: [int], n: int }\nfn main() -> int { var b = Bag { items: [1, 2], n: 2 }; push(b.items, 3); return b.n; }",
        )
        .expect("ok");
    }

    #[test]
    fn struct_equality_rejected() {
        let diags = errors(
            "struct P { x: int }\nfn main() -> int { let a = P { x: 1 }; let b = P { x: 1 }; if a == b { return 1; } return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_OPERANDS));
    }

    #[test]
    fn empty_struct_rejected() {
        let diags = errors("struct Empty { }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::UNSUPPORTED));
    }

    #[test]
    fn reserved_struct_name_rejected() {
        let diags = errors("struct int { x: int }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_DECL));
    }

    #[test]
    fn struct_global_rejected() {
        let diags =
            errors("struct P { x: int }\nlet p = P { x: 1 };\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::NOT_CONSTANT));
    }

    #[test]
    fn nested_struct_forward_reference_allowed() {
        check_src(
            "struct Outer { inner: Inner }\nstruct Inner { v: int }\nfn main() -> int { let o = Outer { inner: Inner { v: 1 } }; return o.inner.v; }",
        )
        .expect("ok");
    }

    #[test]
    fn print_struct_rejected_with_help() {
        let diags = errors(
            "struct P { x: int }\nfn main() -> int { let p = P { x: 1 }; println(p); return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
    }

    #[test]
    fn assigning_to_field_of_call_result_rejected() {
        let diags = errors(
            "struct P { x: int }\nfn make() -> P { return P { x: 1 }; }\nfn main() -> int { make().x = 5; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_LVALUE));
    }

    #[test]
    fn field_types_may_be_char_and_bool() {
        check_src(
            "struct Flags { on: bool, letter: char }\nfn main() -> int { let f = Flags { on: true, letter: 'A' }; if f.on { return int(f.letter); } return 0; }",
        )
        .expect("ok");
    }

    #[test]
    fn shadowing_struct_name_by_variable_is_allowed() {
        check_src("struct P { x: int }\nfn main() -> int { let P = 5; return P; }").expect("ok");
    }

    #[test]
    fn methods_typecheck() {
        check_src(
            "struct P { x: int, y: int }\nimpl P {\n    fn sum(self) -> int {\n        return self.x + self.y;\n    }\n    fn moved(self, dx: int) -> P {\n        return P { x: self.x + dx, y: self.y };\n    }\n    fn shift(var self, dx: int) -> void {\n        self.x = self.x + dx;\n    }\n}\nfn main() -> int {\n    var p = P { x: 1, y: 2 };\n    p.shift(3);\n    let q = p.moved(1);\n    return q.sum() + p.sum();\n}",
        )
        .expect("ok");
    }

    #[test]
    fn method_on_unknown_type_rejected() {
        let diags = errors(
            "impl Missing { fn f(self) -> int { return 0; } }\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMPL_UNKNOWN_TYPE));
    }

    #[test]
    fn duplicate_method_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn f(self) -> int { return 0; } }\nimpl P { fn f(self) -> int { return 1; } }\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_METHOD));
    }

    #[test]
    fn method_field_name_collision_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn x(self) -> int { return 0; } }\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_METHOD));
    }

    #[test]
    fn unknown_method_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn known(self) -> int { return 0; } }\nfn main() -> int { let p = P { x: 1 }; return p.missing(); }",
        );
        let d = diags
            .iter()
            .find(|d| d.code == codes::UNKNOWN_METHOD)
            .expect("unknown method");
        assert!(d.help.as_ref().unwrap().contains("known"));
    }

    #[test]
    fn mutating_method_on_let_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn set(var self) -> void { self.x = 1; } }\nfn main() -> int { let p = P { x: 1 }; p.set(); return 0; }",
        );
        assert!(diags
            .iter()
            .any(|d| d.code == codes::MUTATING_METHOD_RECEIVER));
    }

    #[test]
    fn mutating_method_on_temporary_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P {\n    fn set(var self) -> void { self.x = 1; }\n    fn copy_of(self) -> P { return P { x: self.x }; }\n}\nfn main() -> int { let p = P { x: 1 }; p.copy_of().set(); return 0; }",
        );
        assert!(diags
            .iter()
            .any(|d| d.code == codes::MUTATING_METHOD_RECEIVER));
    }

    #[test]
    fn mutating_method_on_temporary_value_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P {\n    fn set(var self) -> void { self.x = 1; }\n    fn clone_p(self) -> P { return P { x: self.x }; }\n}\nfn main() -> int { clone_p_inner().set(); return 0; }\nfn clone_p_inner() -> P { return P { x: 1 }; }",
        );
        assert!(diags
            .iter()
            .any(|d| d.code == codes::MUTATING_METHOD_RECEIVER));
    }

    #[test]
    fn method_argument_types_checked() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn add(self, v: int) -> int { return self.x + v; } }\nfn main() -> int { let p = P { x: 1 }; return p.add(\"x\"); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn method_argument_count_checked() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn add(self, v: int) -> int { return self.x + v; } }\nfn main() -> int { let p = P { x: 1 }; return p.add(); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::ARG_COUNT));
    }

    #[test]
    fn method_missing_return_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn get(self) -> int { let y = 1; } }\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::MISSING_RETURN));
    }

    #[test]
    fn method_calls_method_of_same_type() {
        check_src(
            "struct P { x: int }\nimpl P {\n    fn double(self) -> int { return self.x * 2; }\n    fn quad(self) -> int { return self.double() + self.double(); }\n}\nfn main() -> int { let p = P { x: 3 }; return p.quad(); }",
        )
        .expect("ok");
    }

    #[test]
    fn method_on_self_field_allowed() {
        check_src(
            "struct I { v: int }\nstruct O { inner: I }\nimpl I { fn get(self) -> int { return self.v; } }\nimpl O { fn inner_get(self) -> int { return self.inner.get(); } }\nfn main() -> int { let o = O { inner: I { v: 4 } }; return o.inner.get(); }",
        )
        .expect("ok");
    }

    #[test]
    fn mutating_method_on_self_field_requires_var_root() {
        let diags = errors(
            "struct I { v: int }\nstruct O { inner: I }\nimpl I { fn set(var self) -> void { self.v = 1; } }\nfn main() -> int { let o = O { inner: I { v: 4 } }; o.inner.set(); return 0; }",
        );
        assert!(diags
            .iter()
            .any(|d| d.code == codes::MUTATING_METHOD_RECEIVER));
    }

    #[test]
    fn field_assignment_in_value_receiver_rejected() {
        let diags = errors(
            "struct P { x: int }\nimpl P { fn bad(self) -> void { self.x = 1; } }\nfn main() -> int { var p = P { x: 1 }; p.bad(); return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMMUTABLE));
        let d = diags.iter().find(|d| d.code == codes::IMMUTABLE).unwrap();
        assert!(d.help.as_ref().unwrap().contains("var self"));
    }

    #[test]
    fn self_outside_method_rejected_with_help() {
        let diags = errors("fn main() -> int { return self.x; }");
        let d = diags
            .iter()
            .find(|d| d.code == codes::UNDEFINED_VARIABLE)
            .expect("undefined self");
        assert!(d.help.as_ref().unwrap().contains("impl"));
    }

    #[test]
    fn method_call_on_non_struct_rejected() {
        let diags = errors("fn main() -> int { let n = 1; return n.foo(); }");
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_METHOD));
    }

    #[test]
    fn calling_field_as_method_rejected() {
        let diags =
            errors("struct P { x: int }\nfn main() -> int { let p = P { x: 1 }; return p.x(); }");
        assert!(diags.iter().any(|d| d.code == codes::NOT_CALLABLE));
    }

    #[test]
    fn method_returning_struct_usable_in_expression() {
        check_src(
            "struct P { x: int }\nimpl P {\n    fn inc(self) -> P { return P { x: self.x + 1 }; }\n}\nfn main() -> int {\n    let a = P { x: 1 };\n    return a.inc().inc().inc().x;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn methods_from_multiple_impl_blocks_allowed() {
        check_src(
            "struct P { x: int }\nimpl P { fn a(self) -> int { return self.x; } }\nimpl P { fn b(self) -> int { return self.a() + 1; } }\nfn main() -> int { let p = P { x: 1 }; return p.b(); }",
        )
        .expect("ok");
    }

    #[test]
    fn enums_construct_and_match() {
        check_src(
            "enum Color { Red, Green, Blue }\nfn name(c: Color) -> string {\n    return match c {\n        Color::Red => \"red\",\n        Color::Green => \"green\",\n        Color::Blue => \"blue\"\n    };\n}\nfn main() -> int {\n    let c = Color::Green;\n    let s = name(c);\n    println(s);\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn enum_payload_variants_typecheck() {
        check_src(
            "enum Message {\n    Quit,\n    Move { x: int, y: int },\n    Write(string)\n}\nfn size(m: Message) -> int {\n    return match m {\n        Message::Quit => 0,\n        Message::Move { x, y } => x + y,\n        Message::Write(text) => len(text)\n    };\n}\nfn main() -> int {\n    let a = Message::Quit;\n    let b = Message::Move { x: 1, y: 2 };\n    let c = Message::Write(\"abc\");\n    return size(a) + size(b) + size(c);\n}",
        )
        .expect("ok");
    }

    #[test]
    fn option_and_result_inference() {
        check_src(
            "fn find(xs: [int], target: int) -> Option<int> {\n    for i in 0..len(xs) {\n        if xs[i] == target {\n            return Option::Some(i);\n        }\n    }\n    return Option::None;\n}\nfn safe_div(a: int, b: int) -> Result<int, string> {\n    if b == 0 {\n        return Result::Err(\"zero\");\n    }\n    return Result::Ok(a / b);\n}\nfn main() -> int {\n    let r = safe_div(6, 3);\n    let value = match r {\n        Result::Ok(v) => v,\n        Result::Err(m) => len(m)\n    };\n    let f = find([1, 2, 3], 2);\n    return match f {\n        Option::Some(i) => i + value,\n        Option::None => value\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn nested_option_exhaustive() {
        check_src(
            "fn f(o: Option<Option<int>>) -> int {\n    return match o {\n        Option::Some(Option::Some(v)) => v,\n        Option::Some(Option::None) => 0,\n        Option::None => -1\n    };\n}\nfn main() -> int { return f(Option::None); }",
        )
        .expect("ok");
    }

    #[test]
    fn nested_option_missing_case_rejected() {
        let diags = errors(
            "fn f(o: Option<Option<int>>) -> int {\n    return match o {\n        Option::Some(Option::Some(v)) => v,\n        Option::None => -1\n    };\n}\nfn main() -> int { return f(Option::None); }",
        );
        let d = diags
            .iter()
            .find(|d| d.code == codes::NON_EXHAUSTIVE_MATCH)
            .expect("non-exhaustive");
        assert!(d.message.contains("Option::Some(Option::None)"));
    }

    #[test]
    fn wildcard_makes_match_exhaustive() {
        check_src(
            "enum E { A, B }\nfn f(e: E) -> int {\n    return match e {\n        E::A => 1,\n        _ => 0\n    };\n}\nfn main() -> int { return f(E::A); }",
        )
        .expect("ok");
    }

    #[test]
    fn binding_pattern_covers_rest() {
        check_src(
            "enum E { A, B }\nfn f(e: E) -> int {\n    return match e {\n        E::A => 1,\n        other => 2\n    };\n}\nfn main() -> int { return f(E::B); }",
        )
        .expect("ok");
    }

    #[test]
    fn non_exhaustive_rejected_with_witness() {
        let diags = errors(
            "enum E { A, B, C }\nfn f(e: E) -> int {\n    return match e {\n        E::A => 1,\n        E::B => 2\n    };\n}\nfn main() -> int { return f(E::A); }",
        );
        let d = diags
            .iter()
            .find(|d| d.code == codes::NON_EXHAUSTIVE_MATCH)
            .expect("non-exhaustive");
        assert!(d.message.contains("E::C"));
    }

    #[test]
    fn unreachable_arm_rejected() {
        let diags = errors(
            "enum E { A, B }\nfn f(e: E) -> int {\n    return match e {\n        E::A => 1,\n        E::B => 2,\n        E::A => 3\n    };\n}\nfn main() -> int { return f(E::A); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNREACHABLE_PATTERN));
    }

    #[test]
    fn unreachable_after_wildcard_rejected() {
        let diags = errors(
            "enum E { A, B }\nfn f(e: E) -> int {\n    return match e {\n        _ => 0,\n        E::A => 1\n    };\n}\nfn main() -> int { return f(E::A); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNREACHABLE_PATTERN));
    }

    #[test]
    fn unknown_variant_rejected() {
        let diags =
            errors("enum E { A }\nfn main() -> int {\n    let x = E::Missing;\n    return 0;\n}");
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_VARIANT));
    }

    #[test]
    fn unknown_variant_in_pattern_rejected() {
        let diags = errors(
            "enum E { A }\nfn f(e: E) -> int {\n    return match e {\n        E::Missing => 1,\n        _ => 0\n    };\n}\nfn main() -> int { return f(E::A); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_VARIANT));
    }

    #[test]
    fn wrong_payload_type_rejected() {
        let diags = errors(
            "enum E { V(int) }\nfn main() -> int {\n    let x = E::V(\"text\");\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn wrong_payload_count_rejected() {
        let diags = errors(
            "enum E { V(int, int) }\nfn main() -> int {\n    let x = E::V(1);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::ARG_COUNT));
    }

    #[test]
    fn unit_variant_with_value_rejected() {
        let diags =
            errors("enum E { A }\nfn main() -> int {\n    let x = E::A(1);\n    return 0;\n}");
        assert!(diags.iter().any(|d| d.code == codes::ARG_COUNT));
    }

    #[test]
    fn tuple_variant_without_value_rejected() {
        let diags =
            errors("enum E { V(int) }\nfn main() -> int {\n    let x = E::V;\n    return 0;\n}");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_PATTERN));
    }

    #[test]
    fn named_variant_field_errors() {
        let diags = errors(
            "enum E { V { x: int } }\nfn main() -> int {\n    let a = E::V { x: 1, y: 2 };\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_FIELD));
        let diags = errors(
            "enum E { V { x: int, y: int } }\nfn main() -> int {\n    let a = E::V { x: 1 };\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::MISSING_FIELD));
    }

    #[test]
    fn duplicate_variant_rejected() {
        let diags = errors("enum E { A, A }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ENUM));
    }

    #[test]
    fn empty_enum_rejected() {
        let diags = errors("enum E { }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ENUM));
    }

    #[test]
    fn recursive_enum_rejected() {
        let diags = errors("enum List { Nil, Cons(int, List) }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::RECURSIVE_ENUM));
    }

    #[test]
    fn struct_enum_cycle_rejected() {
        let diags = errors("struct S { e: E }\nenum E { V(S) }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::RECURSIVE_ENUM));
        assert!(diags.iter().any(|d| d.code == codes::RECURSIVE_STRUCT));
    }

    #[test]
    fn builtin_enum_redefinition_rejected() {
        let diags = errors("enum Option { A }\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_DECL));
    }

    #[test]
    fn pattern_for_wrong_enum_rejected() {
        let diags = errors(
            "enum A { X }\nenum B { Y }\nfn f(a: A) -> int {\n    return match a {\n        B::Y => 1\n    };\n}\nfn main() -> int { return f(A::X); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_PATTERN));
    }

    #[test]
    fn pattern_arity_mismatch_rejected() {
        let diags = errors(
            "enum E { V(int, int) }\nfn f(e: E) -> int {\n    return match e {\n        E::V(x) => x\n    };\n}\nfn main() -> int { return f(E::V(1, 2)); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_PATTERN));
    }

    #[test]
    fn cannot_infer_type_argument_rejected() {
        let diags = errors("fn main() -> int {\n    let x = Option::None;\n    return 0;\n}");
        assert!(diags
            .iter()
            .any(|d| d.code == codes::CANNOT_INFER_TYPE_ARGS));
    }

    #[test]
    fn match_arm_type_mismatch_rejected() {
        let diags = errors(
            "enum E { A, B }\nfn f(e: E) -> int {\n    return match e {\n        E::A => 1,\n        E::B => \"two\"\n    };\n}\nfn main() -> int { return f(E::A); }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn enum_equality_rejected() {
        let diags = errors(
            "enum E { A }\nfn main() -> int {\n    let a = E::A;\n    let b = E::A;\n    if a == b { return 1; }\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_OPERANDS));
    }

    #[test]
    fn print_enum_rejected() {
        let diags =
            errors("enum E { A }\nfn main() -> int {\n    println(E::A);\n    return 0;\n}");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
    }

    #[test]
    fn array_of_enums_rejected() {
        let diags = errors(
            "enum E { A }\nfn main() -> int {\n    let xs = [E::A, E::A];\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNSUPPORTED));
    }

    #[test]
    fn enum_in_struct_field_allowed() {
        check_src(
            "enum State { On, Off }\nstruct Switch { state: State, label: string }\nfn main() -> int {\n    var s = Switch { state: State::On, label: \"main\" };\n    let is_on = match s.state {\n        State::On => true,\n        State::Off => false\n    };\n    if is_on {\n        s.state = State::Off;\n    }\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn struct_in_enum_payload_allowed() {
        check_src(
            "struct Point { x: int, y: int }\nenum Shape { Dot(Point), Empty }\nfn area(s: Shape) -> int {\n    return match s {\n        Shape::Dot(p) => p.x * p.y,\n        Shape::Empty => 0\n    };\n}\nfn main() -> int {\n    let p = Point { x: 2, y: 3 };\n    return area(Shape::Dot(p));\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_enum_two_params_work() {
        check_src(
            "fn parse(s: string) -> Result<int, string> {\n    return Result::Ok(len(s));\n}\nfn main() -> int {\n    let r = parse(\"abc\");\n    return match r {\n        Result::Ok(n) => n,\n        Result::Err(msg) => len(msg)\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn user_generic_enum_works() {
        check_src(
            "enum Pair<A, B> { Both(A, B), Empty }\nfn swap(p: Pair<int, string>) -> Pair<string, int> {\n    return match p {\n        Pair::Both(a, b) => Pair::Both(b, a),\n        Pair::Empty => Pair::Empty\n    };\n}\nfn main() -> int {\n    let p = Pair::Both(1, \"one\");\n    let q = swap(p);\n    return match q {\n        Pair::Both(s, n) => len(s) + n,\n        Pair::Empty => 0\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_enum_instantiation_mismatch_rejected() {
        let diags = errors(
            "fn takes(o: Option<int>) -> int { return 0; }\nfn main() -> int {\n    let o: Option<string> = Option::Some(\"x\");\n    return takes(o);\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn match_on_non_enum_with_wildcard_allowed() {
        check_src(
            "fn main() -> int {\n    let x = 5;\n    return match x {\n        _ => 0\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn match_on_non_enum_with_variant_rejected() {
        let diags = errors(
            "enum E { A }\nfn main() -> int {\n    let x = 5;\n    return match x {\n        E::A => 1\n    };\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_PATTERN));
    }

    #[test]
    fn enum_forward_reference_allowed() {
        check_src(
            "enum Outer { Wrap(Inner), None }\nenum Inner { One }\nfn main() -> int {\n    let o = Outer::Wrap(Inner::One);\n    return match o {\n        Outer::Wrap(i) => 1,\n        Outer::None => 0\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn enum_global_rejected() {
        let diags = errors("enum E { A }\nlet x = E::A;\nfn main() -> int { return 0; }");
        assert!(!diags.is_empty());
    }

    #[test]
    fn match_arms_with_void_bodies_allowed() {
        check_src(
            "enum E { A, B }\nfn f(e: E) -> void {\n    match e {\n        E::A => println(\"a\"),\n        E::B => println(\"b\")\n    }\n}\nfn main() -> int {\n    f(E::A);\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_struct_inference_works() {
        check_src(
            "struct Pair<A, B> {\n    first: A,\n    second: B\n}\nfn main() -> int {\n    let p = Pair { first: 42, second: \"x\" };\n    let n: int = p.first;\n    let s: string = p.second;\n    return n;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_struct_explicit_annotation_works() {
        check_src(
            "struct Box<T> {\n    value: T\n}\nfn main() -> int {\n    let b: Box<int> = Box { value: 1 };\n    let v: int = b.value;\n    return v;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_struct_wrong_field_type_rejected() {
        let diags = errors(
            "struct Box<T> {\n    value: T\n}\nfn main() -> int {\n    let b: Box<int> = Box { value: \"wrong\" };\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn generic_struct_missing_type_args_rejected() {
        let diags = errors(
            "struct Box<T> {\n    value: T\n}\nfn main() -> int {\n    let b: Box = Box { value: 1 };\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_TYPE));
    }

    #[test]
    fn generic_struct_cannot_infer_rejected() {
        let diags = errors(
            "struct Tagged<T> {\n    label: string\n}\nfn main() -> int {\n    let t = Tagged { label: \"x\" };\n    return 0;\n}",
        );
        assert!(diags
            .iter()
            .any(|d| d.code == codes::CANNOT_INFER_TYPE_ARGS));
    }

    #[test]
    fn nested_generic_struct_works() {
        check_src(
            "struct Box<T> {\n    value: T\n}\nfn main() -> int {\n    let inner: Box<int> = Box { value: 1 };\n    let outer: Box<Box<int>> = Box { value: inner };\n    let v: int = outer.value.value;\n    return v;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_struct_different_instantiations_are_distinct() {
        let diags = errors(
            "struct Box<T> {\n    value: T\n}\nfn main() -> int {\n    let a: Box<int> = Box { value: 1 };\n    let b: Box<string> = Box { value: \"x\" };\n    a = b;\n    return 0;\n}",
        );
        assert!(!diags.is_empty());
    }

    #[test]
    fn generic_function_identity_works() {
        check_src(
            "fn identity<T>(value: T) -> T {\n    return value;\n}\nfn main() -> int {\n    let n = identity(42);\n    let s = identity(\"text\");\n    println(n);\n    println(s);\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_function_multiple_params_works() {
        check_src(
            "struct Pair<A, B> {\n    first: A,\n    second: B\n}\nfn make_pair<A, B>(first: A, second: B) -> Pair<A, B> {\n    return Pair { first: first, second: second };\n}\nfn main() -> int {\n    let p = make_pair(1, \"two\");\n    let n: int = p.first;\n    let s: string = p.second;\n    return n;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_function_inference_from_expected_return() {
        check_src(
            "fn empty<T>() -> Option<T> {\n    return Option::None;\n}\nfn main() -> int {\n    let o: Option<int> = empty();\n    return match o {\n        Option::Some(v) => v,\n        Option::None => 0\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_function_not_inferable_rejected() {
        let diags = errors(
            "fn empty<T>() -> Option<T> {\n    return Option::None;\n}\nfn main() -> int {\n    let o = empty();\n    return 0;\n}",
        );
        assert!(diags
            .iter()
            .any(|d| d.code == codes::CANNOT_INFER_TYPE_ARGS));
    }

    #[test]
    fn generic_function_turbofish_works() {
        check_src(
            "fn empty<T>() -> Option<T> {\n    return Option::None;\n}\nfn main() -> int {\n    let o = empty::<int>();\n    return match o {\n        Option::Some(v) => v,\n        Option::None => 0\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_function_turbofish_wrong_arity_rejected() {
        let diags = errors(
            "fn f<A, B>(a: A, b: B) -> A {\n    return a;\n}\nfn main() -> int {\n    return f::<int>(1, 2);\n}",
        );
        assert!(diags
            .iter()
            .any(|d| d.code == codes::CANNOT_INFER_TYPE_ARGS));
    }

    #[test]
    fn generic_function_conflicting_args_rejected() {
        let diags = errors(
            "fn same<T>(a: T, b: T) -> T {\n    return a;\n}\nfn main() -> int {\n    let x = same(1, \"two\");\n    return 0;\n}",
        );
        assert!(!diags.is_empty());
    }

    #[test]
    fn generic_method_on_generic_struct_works() {
        check_src(
            "struct Box<T> {\n    value: T\n}\nimpl<T> Box<T> {\n    fn get(self) -> T {\n        return self.value;\n    }\n}\nfn main() -> int {\n    let b = Box { value: 7 };\n    let v: int = b.get();\n    let t = Box { value: \"x\" };\n    let s: string = t.get();\n    return v;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_method_calls_generic_method_works() {
        check_src(
            "struct Box<T> {\n    value: T\n}\nimpl<T> Box<T> {\n    fn get(self) -> T {\n        return self.value;\n    }\n    fn get_twice(self) -> T {\n        return self.get();\n    }\n}\nfn main() -> int {\n    let b = Box { value: 3 };\n    return b.get_twice();\n}",
        )
        .expect("ok");
    }

    #[test]
    fn generic_impl_requires_matching_params() {
        let diags = errors(
            "struct Box<T> {\n    value: T\n}\nimpl<T> Box<int> {\n    fn get(self) -> int {\n        return self.value;\n    }\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMPL_UNKNOWN_TYPE));
    }

    #[test]
    fn enum_method_works() {
        check_src(
            "enum Shape {\n    Circle(float),\n    Rectangle { width: float, height: float }\n}\nimpl Shape {\n    fn is_circle(self) -> bool {\n        return match self {\n            Shape::Circle(_) => true,\n            Shape::Rectangle { width: _, height: _ } => false\n        };\n    }\n}\nfn main() -> int {\n    let s = Shape::Circle(1.0);\n    if s.is_circle() {\n        return 1;\n    }\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn enum_associated_function_works() {
        check_src(
            "enum Shape {\n    Circle(float)\n}\nimpl Shape {\n    fn circle(radius: float) -> Shape {\n        return Shape::Circle(radius);\n    }\n    fn unit() -> Shape {\n        return Shape::Circle(1.0);\n    }\n}\nfn main() -> int {\n    let a = Shape::circle(2.0);\n    let b = Shape::unit();\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn associated_function_conflicts_with_variant_rejected() {
        let diags = errors(
            "enum E {\n    A\n}\nimpl E {\n    fn A() -> E {\n        return E::A;\n    }\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::DUPLICATE_METHOD));
    }

    #[test]
    fn generic_enum_method_on_builtin_option_works() {
        check_src(
            "impl<T> Option<T> {\n    fn is_some(self) -> bool {\n        return match self {\n            Option::Some(_) => true,\n            Option::None => false\n        };\n    }\n}\nfn main() -> int {\n    let a: Option<int> = Option::Some(1);\n    let b: Option<int> = Option::None;\n    if a.is_some() && !b.is_some() {\n        return 1;\n    }\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn mutable_enum_receiver_rejected() {
        let diags = errors(
            "enum E {\n    A\n}\nimpl E {\n    fn bump(var self) -> void {\n        return;\n    }\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMPL_UNKNOWN_TYPE));
    }

    #[test]
    fn try_operator_propagates_result() {
        check_src(
            "fn parse(text: string) -> Result<int, string> {\n    if len(text) == 0 {\n        return Result::Err(\"empty\");\n    }\n    return Result::Ok(len(text));\n}\nfn doubled(text: string) -> Result<int, string> {\n    let v = parse(text)?;\n    return Result::Ok(v * 2);\n}\nfn main() -> int {\n    let r = doubled(\"abc\");\n    return match r {\n        Result::Ok(v) => v,\n        Result::Err(e) => 0\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn try_operator_requires_result_operand() {
        let diags = errors(
            "fn f() -> Result<int, string> {\n    let x = 5;\n    let y = x?;\n    return Result::Ok(y);\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TRY_INVALID_OPERAND));
    }

    #[test]
    fn try_operator_requires_result_return() {
        let diags = errors(
            "fn f() -> int {\n    let r: Result<int, string> = Result::Ok(1);\n    return r?;\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TRY_RETURN_MISMATCH));
    }

    #[test]
    fn try_operator_error_type_mismatch_rejected() {
        let diags = errors(
            "fn f(r: Result<int, string>) -> Result<int, int> {\n    let v = r?;\n    return Result::Ok(v);\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TRY_RETURN_MISMATCH));
    }

    #[test]
    fn slice_view_typechecks() {
        check_src(
            "fn main() -> int {\n    let xs = [1, 2, 3];\n    let v = slice_view(xs, 0, 2);\n    let n = len(v);\n    return v[0] + n;\n}",
        )
        .expect("ok");
        check_src(
            "fn main() -> int {\n    let ws = [\"a\", \"b\"];\n    let v: Slice<string> = slice_view(ws, 0, 1);\n    return len(v[0]);\n}",
        )
        .expect("ok");
        check_src(
            "fn main() -> int {\n    let fs = [1.5, 2.5];\n    let v = slice_view(fs, 0, 2);\n    return int(v[1]);\n}",
        )
        .expect("ok");
    }

    #[test]
    fn slice_view_requires_array() {
        let diags =
            errors("fn main() -> int {\n    let v = slice_view(5, 0, 1);\n    return 0;\n}");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
    }

    #[test]
    fn slice_write_requires_var_binding() {
        check_src(
            "fn main() -> int {\n    var xs = [1, 2];\n    var v = slice_view(xs, 0, 2);\n    v[0] = 9;\n    return v[0];\n}",
        )
        .expect("ok");
        let diags = errors(
            "fn main() -> int {\n    var xs = [1, 2];\n    let v = slice_view(xs, 0, 2);\n    v[0] = 9;\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMMUTABLE));
    }

    #[test]
    fn slice_rejects_bad_element_type() {
        let diags = errors(
            "struct P { x: int }\nfn main() -> int {\n    let p = P { x: 1 };\n    let v: Slice<P> = slice_view([1], 0, 1);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNKNOWN_TYPE));
    }

    #[test]
    fn slice_rejects_array_operations() {
        let diags = errors(
            "fn main() -> int {\n    var xs = [1, 2];\n    let v = slice_view(xs, 0, 2);\n    push(v, 3);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
        let diags = errors(
            "fn main() -> int {\n    var xs = [1, 2];\n    let v = slice_view(xs, 0, 2);\n    let r = pop(v);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
    }

    #[test]
    fn generic_slice_function_typechecks() {
        check_src(
            "fn first_of<T>(s: Slice<T>) -> T {\n    return s[0];\n}\nfn main() -> int {\n    let a = first_of(slice_view([7, 8], 0, 2));\n    let b = first_of(slice_view([\"x\", \"y\"], 1, 2));\n    return a + len(b);\n}",
        )
        .expect("ok");
    }

    #[test]
    fn global_struct_declarations_rejected_for_varself_safety() {
        // var self receivers are struct/enum lvalues. Structs cannot live in
        // globals, so a receiver slot cannot be reachable and overwritten
        // from other functions while a method call is in progress. If this
        // ever becomes legal, the var self alias analysis must be redone.
        let diags =
            errors("struct P { x: int }\nvar g = P { x: 1 };\nfn main() -> int { return g.x; }");
        assert!(diags.iter().any(|d| d.code == codes::NOT_CONSTANT));
    }

    #[test]
    fn try_operator_in_generic_function_works() {
        check_src(
            "fn pass<T>(r: Result<T, string>) -> Result<T, string> {\n    let v = r?;\n    return Result::Ok(v);\n}\nfn main() -> int {\n    let r = pass(Result::Ok(5));\n    return match r {\n        Result::Ok(v) => v,\n        Result::Err(e) => 0\n    };\n}",
        )
        .expect("ok");
    }

    #[test]
    fn option_try_operator_typechecks() {
        check_src(
            "fn get() -> Option<int> {\n    return Option::Some(1);\n}\nfn calc() -> Option<int> {\n    let v = get()?;\n    return Option::Some(v * 2);\n}\nfn main() -> int {\n    return match calc() {\n        Option::Some(v) => v,\n        Option::None => 0\n    };\n}",
        )
        .expect("ok");
        let diags = errors(
            "fn calc(o: Option<int>) -> int {\n    let v = o?;\n    return v;\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::TRY_RETURN_MISMATCH));
    }

    #[test]
    fn panic_is_diverging_for_return_analysis() {
        check_src(
            "fn f(n: int) -> int {\n    if n < 0 {\n        panic(\"negative\");\n    }\n    return n;\n}\nfn g() -> int {\n    panic(\"always\");\n}\nfn main() -> int {\n    if false {\n        let x = g();\n        return x;\n    }\n    return f(1);\n}",
        )
        .expect("ok");
    }

    #[test]
    fn or_pattern_bindings_must_match() {
        check_src(
            "enum E { A(int), B(int), C }\nfn f(e: E) -> int {\n    return match e {\n        E::A(v) | E::B(v) => v,\n        E::C => 0\n    };\n}\nfn main() -> int { return f(E::C); }",
        )
        .expect("ok");
        let diags = errors(
            "fn f(o: Option<int>) -> int {\n    return match o {\n        Option::Some(v) | Option::None => v,\n        _ => 0\n    };\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_PATTERN));
    }

    #[test]
    fn or_pattern_expands_for_exhaustiveness() {
        check_src(
            "enum E { A, B, C }\nfn f(e: E) -> int {\n    return match e {\n        E::A | E::B => 1,\n        E::C => 2\n    };\n}\nfn main() -> int { return f(E::A); }",
        )
        .expect("ok");
        let diags = errors(
            "enum E { A, B, C }\nfn f(e: E) -> int {\n    return match e {\n        E::A | E::B => 1\n    };\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::NON_EXHAUSTIVE_MATCH));
    }

    #[test]
    fn join_typechecks() {
        check_src(
            "fn main() -> int {\n    let parts = [\"a\", \"b\"];\n    let s = join(parts, \", \");\n    return len(s);\n}",
        )
        .expect("ok");
        let diags = errors(
            "fn main() -> int {\n    let xs = [1, 2];\n    let s = join(xs, \",\");\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
    }

    #[test]
    fn math_module_functions_typecheck() {
        let tokens = lex("use std::math;\nfn main() -> int {\n    let x = math::sqrt(4.0);\n    let y = math::clamp_int(5, 0, 3);\n    return int(x) + y;\n}");
        let out = parse(tokens.tokens);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        let mut loaded = crate::modules::LoadedProgram {
            modules: vec![crate::modules::LoadedModule {
                name: String::new(),
                path: std::path::PathBuf::from("input.blm"),
                source: String::new(),
                program: out.program,
                imports: Vec::new(),
                builtin: None,
            }],
            entry_index: 0,
        };
        loaded.modules[0].imports.push(crate::modules::ImportEdge {
            local_name: "math".to_string(),
            target: ImportedItem::Module("std::math".to_string()),
            path_span: Span::point(1, 1),
        });
        let builtin = crate::modules::LoadedModule {
            name: "std::math".to_string(),
            path: std::path::PathBuf::from("<builtin:std::math>"),
            source: String::new(),
            program: Program { items: Vec::new() },
            imports: Vec::new(),
            builtin: Some(crate::modules::BuiltinModule::Math),
        };
        loaded.modules.push(builtin);
        check_modules(loaded, &[]).expect("ok");
    }

    #[test]
    fn generic_enum_user_defined_with_method_works() {
        check_src(
            "enum Pair<A, B> {\n    Both(A, B),\n    Empty\n}\nimpl<A, B> Pair<A, B> {\n    fn is_empty(self) -> bool {\n        return match self {\n            Pair::Both(a, b) => false,\n            Pair::Empty => true\n        };\n    }\n}\nfn main() -> int {\n    let p = Pair::Both(1, \"x\");\n    let e: Pair<int, string> = Pair::Empty;\n    if !p.is_empty() && e.is_empty() {\n        return 1;\n    }\n    return 0;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn recursive_generic_struct_rejected() {
        let diags = errors("struct W<T> {\n    v: W<W<T>>\n}\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::RECURSIVE_STRUCT));
    }

    #[test]
    fn generic_struct_value_semantics_preserved() {
        check_src(
            "struct Box<T> {\n    value: T\n}\nfn main() -> int {\n    var a = Box { value: 1 };\n    var b = a;\n    b.value = 2;\n    return a.value + b.value;\n}",
        )
        .expect("ok");
    }

    #[test]
    fn literal_patterns_and_guards_typecheck() {
        check_src(
            "fn f(e: int) -> int {\n    return match e {\n        0 => 1,\n        -3 => 2,\n        n if n > 100 => 3,\n        _ => 0\n    };\n}\nfn main() -> int { return f(1); }",
        )
        .expect("ok");
    }

    #[test]
    fn literal_pattern_type_mismatch_rejected() {
        let diags = errors(
            "fn f(e: int) -> int {\n    return match e {\n        \"text\" => 1,\n        _ => 0\n    };\n}\nfn main() -> int { return 0; }",
        );
        assert!(!diags.is_empty());
    }

    #[test]
    fn guard_must_be_bool() {
        let diags = errors(
            "fn f(e: int) -> int {\n    return match e {\n        n if n => 1,\n        _ => 0\n    };\n}\nfn main() -> int { return 0; }",
        );
        assert!(!diags.is_empty());
    }

    #[test]
    fn pop_requires_mutable_array() {
        check_src(
            "fn main() -> int {\n    var xs: [int] = [1, 2];\n    let last = pop(xs);\n    return match last {\n        Option::Some(v) => v,\n        Option::None => 0\n    };\n}",
        )
        .expect("ok");
        let diags = errors(
            "fn main() -> int {\n    let xs: [int] = [1, 2];\n    let last = pop(xs);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMMUTABLE));
        let diags = errors("fn main() -> int {\n    let n = pop(5);\n    return 0;\n}");
        assert!(!diags.is_empty());
    }

    #[test]
    fn insert_checks_index_and_value() {
        check_src(
            "fn main() -> int {\n    var xs: [int] = [1];\n    insert(xs, 0, 7);\n    insert(xs, len(xs), 8);\n    return len(xs);\n}",
        )
        .expect("ok");
        let diags = errors(
            "fn main() -> int {\n    var xs: [int] = [1];\n    insert(xs, 0, \"x\");\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
        let diags = errors(
            "fn main() -> int {\n    var xs: [int] = [1];\n    insert(xs, \"x\", 1);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
    }

    #[test]
    fn remove_and_slice_typecheck() {
        check_src(
            "fn main() -> int {\n    var xs: [int] = [1, 2];\n    let r = remove(xs, 0);\n    let s = slice(xs, 0, len(xs));\n    return len(s) + match r {\n        Option::Some(v) => v,\n        Option::None => 0\n    };\n}",
        )
        .expect("ok");
        let diags = errors(
            "fn main() -> int {\n    let xs: [int] = [1];\n    let r = remove(xs, 0);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::IMMUTABLE));
        let diags = errors("fn main() -> int {\n    let s = slice(5, 0, 1);\n    return 0;\n}");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_ARGUMENT));
        let diags = errors(
            "fn main() -> int {\n    let xs: [int] = [1];\n    let s = slice(xs, 0);\n    return 0;\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::ARG_COUNT));
    }

    #[test]
    fn string_builtins_typecheck() {
        check_src(
            "fn main() -> int {\n    let s = \"Hello\";\n    let a = contains(s, \"ell\");\n    let b = starts_with(s, \"He\");\n    let c = ends_with(s, \"lo\");\n    let d = trim(s);\n    let e = replace(s, \"l\", \"L\");\n    let f = to_upper(s);\n    let g = to_lower(s);\n    let parts = split(s, \"e\");\n    if a && b && c && len(d) > 0 && len(e) > 0 && len(f) > 0 && len(g) > 0 && len(parts) == 2 {\n        return 1;\n    }\n    return 0;\n}",
        )
        .expect("ok");
        let diags = errors("fn main() -> int {\n    let a = contains(5, \"x\");\n    return 0;\n}");
        assert!(!diags.is_empty());
        let diags = errors("fn main() -> int {\n    let a = split(\"x\");\n    return 0;\n}");
        assert!(!diags.is_empty());
    }

    #[test]
    fn ffi_types_and_conversions_typecheck() {
        check_src(
            "extern \"C\" {\n    fn f(a: c_int, p: rawptr) -> c_uint;\n}\nfn main() -> int {\n    unsafe {\n        let r = f(c_int(1), rawptr_null());\n        return int(r);\n    }\n}",
        )
        .expect("ok");
        let diags = errors(
            "extern \"C\" {\n    fn f(a: c_int) -> c_int;\n}\nfn main() -> int {\n    unsafe {\n        return f(1);\n    }\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::TYPE_MISMATCH));
    }

    #[test]
    fn unsafe_is_required_for_extern_calls() {
        let diags = errors(
            "extern \"C\" {\n    fn f() -> c_int;\n}\nfn main() -> int {\n    let x = f();\n    return int(x);\n}",
        );
        assert!(diags.iter().any(|d| d.code == codes::UNSAFE_REQUIRED));
    }

    #[test]
    fn rawptr_rejects_unsupported_operations() {
        let diags = errors(
            "extern \"C\" {\n    fn f() -> rawptr;\n}\nfn main() -> int {\n    unsafe {\n        let p = f();\n        return int(p);\n    }\n}",
        );
        assert!(!diags.is_empty());
    }

    #[test]
    fn extern_requires_valid_abi_and_types() {
        let diags = errors("extern \"cpp\" {\n    fn f();\n}\nfn main() -> int { return 0; }");
        assert!(diags.iter().any(|d| d.code == codes::INVALID_EXTERN));
        let diags = errors(
            "extern \"C\" {\n    fn f(s: string) -> bool;\n}\nfn main() -> int { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == codes::INVALID_FFI_TYPE));
    }
}
