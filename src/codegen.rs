use crate::ast::*;
use crate::check::{substitute_type, BindingInfo, Callee, CheckedProgram, ConstValue};
use crate::types::{c_ident, mangle_type, Type};
use std::collections::{HashMap, HashSet};

pub fn emit(checked: &CheckedProgram) -> String {
    let mut pointer_vars: HashSet<DeclId> = HashSet::new();
    for module in &checked.modules {
        for item in &module.program.items {
            if let Item::Impl(b) = item {
                for m in &b.methods {
                    if m.receiver == Some(ReceiverKind::Mutable) {
                        pointer_vars.insert(m.self_id);
                    }
                }
            }
        }
    }
    let entry_file = checked
        .modules
        .get(checked.entry_index)
        .map(|m| m.path_display.clone())
        .unwrap_or_else(|| "input.blm".to_string());
    let mut emitter = CEmitter {
        checked,
        out: String::new(),
        indent: 0,
        temp_counter: 0,
        prelude: Vec::new(),
        file_lit: c_string_literal(entry_file.as_bytes()),
        current_ret: Type::Void,
        pointer_vars,
        current_subst: HashMap::new(),
        scopes: Vec::new(),
        stmt_temps: Vec::new(),
        loop_marks: Vec::new(),
        literals: Vec::new(),
        literal_index: HashMap::new(),
        lit_insert_pos: 0,
    };
    emitter.out.push_str(include_str!("../runtime/bloom_rt.c"));
    emitter.out.push('\n');
    emitter.lit_insert_pos = emitter.out.len();
    emitter.emit_structs();
    emitter.emit_composite_helpers();
    emitter.emit_stdlib_runtime();
    emitter.emit_prototypes();
    emitter.emit_globals();
    emitter.emit_functions();
    emitter.emit_methods();
    emitter.emit_main_wrapper();
    let decls: String = emitter
        .literals
        .iter()
        .map(|(name, bytes)| {
            format!(
                "static const char bloom_lit_data_{n}[] = {lit};\nstatic bloom_str bloom_lit_{n} = {{ BLOOM_STR_IMMORTAL, (char *)bloom_lit_data_{n}, {len} }};\n",
                n = name,
                lit = c_string_literal(bytes),
                len = bytes.len()
            )
        })
        .collect();
    if !decls.is_empty() {
        emitter.out.insert_str(emitter.lit_insert_pos, &decls);
    }
    emitter.out
}

struct CEmitter<'a> {
    checked: &'a CheckedProgram,
    out: String,
    indent: usize,
    temp_counter: u32,
    prelude: Vec<String>,
    file_lit: String,
    current_ret: Type,
    pointer_vars: HashSet<DeclId>,
    current_subst: HashMap<String, Type>,
    scopes: Vec<Vec<(String, Type)>>,
    stmt_temps: Vec<(String, Type)>,
    loop_marks: Vec<usize>,
    literals: Vec<(u32, Vec<u8>)>,
    literal_index: HashMap<Vec<u8>, u32>,
    lit_insert_pos: usize,
}

fn c_type(ty: &Type) -> String {
    match ty {
        Type::Int => "int64_t".to_string(),
        Type::Float => "double".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "bloom_str".to_string(),
        Type::Char => "uint32_t".to_string(),
        Type::Void => "void".to_string(),
        Type::Array(elem) => format!("bloom_arr_{}", arr_suffix(elem)),
        Type::Slice(elem) => format!("bloom_slice_{}", arr_suffix(elem)),
        Type::Ffi(kind) => kind.c_type().to_string(),
        Type::Json => "bloom_json_handle".to_string(),
        Type::Web => "bloom_web_handle".to_string(),
        Type::Html => "bloom_html_handle".to_string(),
        Type::Bytes => "bloom_fs_handle".to_string(),
        Type::Form => "bloom_form_handle".to_string(),
        Type::Cookies => "bloom_cookies_handle".to_string(),
        Type::SessionStore => "bloom_session_store_handle".to_string(),
        Type::Session => "bloom_session_handle".to_string(),
        Type::Db => "bloom_db_handle".to_string(),
        Type::Stmt => "bloom_stmt_handle".to_string(),
        Type::Limiter => "bloom_auth_limiter_handle".to_string(),
        Type::Ui => "bloom_ui_handle".to_string(),
        Type::DesktopWindow => "bloom_desktop_window_handle".to_string(),
        Type::Struct(name, args) => {
            format!("bloom_struct_{}", named_mangle(name, args))
        }
        Type::Enum(name, args) => format!("bloom_enum_{}", named_mangle(name, args)),
        Type::Param(name) => format!("bloom_param_{}", c_ident(name)),
    }
}

fn named_mangle(name: &str, args: &[Type]) -> String {
    let mut s = c_ident(name);
    for a in args {
        s.push('_');
        s.push_str(&mangle_type(a));
    }
    s
}

fn enum_c_name(name: &str, args: &[Type]) -> String {
    format!("bloom_enum_{}", named_mangle(name, args))
}

fn specialized_c_name(base: &str, args: &[Type]) -> String {
    if args.is_empty() {
        base.to_string()
    } else {
        let suffix: Vec<String> = args.iter().map(mangle_type).collect();
        format!("{}_{}", base, suffix.join("_"))
    }
}

#[derive(Default)]
struct PatternPlan {
    conds: Vec<String>,
    decls: Vec<String>,
    applies: Vec<String>,
}

impl PatternPlan {
    fn extend(&mut self, other: PatternPlan) {
        self.conds.extend(other.conds);
        self.decls.extend(other.decls);
        self.applies.extend(other.applies);
    }
}

fn arr_suffix(elem: &Type) -> &'static str {
    match elem {
        Type::Int => "i64",
        Type::Float => "f64",
        Type::Bool => "bool",
        Type::String => "str",
        Type::Char => "char",
        _ => "i64",
    }
}

fn field_c_name(name: &str) -> String {
    format!("f_{}", name)
}

fn c_string_literal(bytes: &[u8]) -> String {
    let mut s = String::from("\"");
    for &b in bytes {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'?' => s.push_str("\\?"),
            0x20..=0x7E => s.push(b as char),
            _ => s.push_str(&format!("\\{:03o}", b)),
        }
    }
    s.push('"');
    s
}

impl<'a> CEmitter<'a> {
    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }

    fn fresh_temp(&mut self) -> String {
        let name = format!("t_{}", self.temp_counter);
        self.temp_counter += 1;
        name
    }

    fn flush_prelude(&mut self) {
        let lines = std::mem::take(&mut self.prelude);
        for l in lines {
            self.line(&l);
        }
    }

    fn subst(&self, ty: &Type) -> Type {
        if self.current_subst.is_empty() || !ty.contains_param() {
            ty.clone()
        } else {
            substitute_type(ty, &self.current_subst)
        }
    }

    fn expr_type(&self, expr: &Expr) -> Type {
        let ty = self
            .checked
            .expr_types
            .get(&expr.id)
            .cloned()
            .unwrap_or(Type::Int);
        self.subst(&ty)
    }

    fn var_c_name(&self, id: DeclId) -> String {
        self.checked
            .var_decls
            .get(&id)
            .map(|v| v.c_name.clone())
            .unwrap_or_else(|| "v_unknown_0".to_string())
    }

    fn var_type(&self, id: DeclId) -> Type {
        let ty = self
            .checked
            .var_decls
            .get(&id)
            .map(|v| v.ty.clone())
            .unwrap_or(Type::Int);
        self.subst(&ty)
    }

    fn expr_type_by_id(&self, id: ExprId) -> Type {
        let ty = self
            .checked
            .expr_types
            .get(&id)
            .cloned()
            .unwrap_or(Type::Int);
        self.subst(&ty)
    }

    /// True if values of this type own heap memory and therefore need
    /// clone/drop handling (strings, arrays, and composites containing them).
    fn is_managed(&self, ty: &Type) -> bool {
        match ty {
            Type::String
            | Type::Array(_)
            | Type::Slice(_)
            | Type::Json
            | Type::Web
            | Type::Html
            | Type::Bytes
            | Type::Form
            | Type::Cookies
            | Type::SessionStore
            | Type::Session
            | Type::Db
            | Type::Stmt
            | Type::Limiter
            | Type::Ui
            | Type::DesktopWindow => true,
            Type::Struct(name, args) => {
                if let Some(info) = self.checked.structs.get(name) {
                    let bindings: HashMap<String, Type> = info
                        .params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    info.fields
                        .iter()
                        .any(|f| self.is_managed(&substitute_type(&f.ty, &bindings)))
                } else {
                    false
                }
            }
            Type::Enum(name, args) => {
                if let Some(info) = self.checked.enums.get(name) {
                    let bindings: HashMap<String, Type> = info
                        .params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    info.variants.iter().any(|v| {
                        v.fields
                            .iter()
                            .any(|f| self.is_managed(&substitute_type(f, &bindings)))
                    })
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn clone_fn_name(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::String => Some("bloom_str_clone".to_string()),
            Type::Json => Some("bloom_json_retain".to_string()),
            Type::Web => Some("bloom_web_retain".to_string()),
            Type::Html => Some("bloom_html_retain".to_string()),
            Type::Bytes => Some("bloom_fs_bytes_retain".to_string()),
            Type::Form => Some("bloom_web_form_retain".to_string()),
            Type::Cookies => Some("bloom_web_cookies_retain".to_string()),
            Type::SessionStore => Some("bloom_web_session_store_retain".to_string()),
            Type::Session => Some("bloom_web_session_retain".to_string()),
            Type::Db => Some("bloom_db_retain".to_string()),
            Type::Stmt => Some("bloom_db_stmt_retain".to_string()),
            Type::Limiter => Some("bloom_auth_limiter_retain".to_string()),
            Type::Ui => Some("bloom_ui_retain".to_string()),
            Type::DesktopWindow => Some("bloom_desktop_window_retain".to_string()),
            Type::Array(elem) => Some(format!("bloom_arr_{}_clone", arr_suffix(elem))),
            Type::Slice(elem) => Some(format!("bloom_slice_{}_clone", arr_suffix(elem))),
            Type::Struct(name, args) | Type::Enum(name, args) => {
                if self.is_managed(ty) {
                    Some(format!("bloom_clone_{}", named_mangle(name, args)))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn drop_fn_name(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::String => Some("bloom_str_drop".to_string()),
            Type::Json => Some("bloom_json_release".to_string()),
            Type::Web => Some("bloom_web_release".to_string()),
            Type::Html => Some("bloom_html_release".to_string()),
            Type::Bytes => Some("bloom_fs_bytes_release".to_string()),
            Type::Form => Some("bloom_web_form_release".to_string()),
            Type::Cookies => Some("bloom_web_cookies_release".to_string()),
            Type::SessionStore => Some("bloom_web_session_store_release".to_string()),
            Type::Session => Some("bloom_web_session_release".to_string()),
            Type::Db => Some("bloom_db_release".to_string()),
            Type::Stmt => Some("bloom_db_stmt_release".to_string()),
            Type::Limiter => Some("bloom_auth_limiter_release".to_string()),
            Type::Ui => Some("bloom_ui_release".to_string()),
            Type::DesktopWindow => Some("bloom_desktop_window_release".to_string()),
            Type::Array(elem) => Some(format!("bloom_arr_{}_drop", arr_suffix(elem))),
            Type::Slice(elem) => Some(format!("bloom_slice_{}_drop", arr_suffix(elem))),
            Type::Struct(name, args) | Type::Enum(name, args) => {
                if self.is_managed(ty) {
                    Some(format!("bloom_drop_{}", named_mangle(name, args)))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Wraps `text` in a clone call if the type is managed.
    fn clone_text(&self, ty: &Type, text: &str) -> String {
        match self.clone_fn_name(ty) {
            Some(f) => format!("{}({})", f, text),
            None => text.to_string(),
        }
    }

    /// Produces a drop statement for a place expression, if the type is managed.
    fn drop_line(&self, ty: &Type, place: &str) -> Option<String> {
        self.drop_fn_name(ty)
            .map(|f| format!("{}(&({}));", f, place))
    }

    /// Creates a new owned temporary holding `init` and records it so it is
    /// dropped at the end of the current statement.
    fn owned_temp(&mut self, ty: &Type, init: &str) -> String {
        let t = self.fresh_temp();
        self.prelude
            .push(format!("{} {} = {};", c_type(ty), t, init));
        if self.is_managed(ty) {
            self.stmt_temps.push((t.clone(), ty.clone()));
        }
        t
    }

    /// If `text` names an owned temporary of the current statement, removes it
    /// from the drop list and returns true (ownership is transferred).
    fn take_owned(&mut self, text: &str) -> bool {
        if let Some(pos) = self.stmt_temps.iter().position(|(n, _)| n == text) {
            self.stmt_temps.remove(pos);
            true
        } else {
            false
        }
    }

    fn literal_name(&mut self, bytes: &[u8]) -> String {
        if let Some(&id) = self.literal_index.get(bytes) {
            return format!("bloom_lit_{}", id);
        }
        let id = self.literals.len() as u32;
        self.literals.push((id, bytes.to_vec()));
        self.literal_index.insert(bytes.to_vec(), id);
        format!("bloom_lit_{}", id)
    }

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn register_local(&mut self, name: &str, ty: &Type) {
        if self.is_managed(ty) {
            if let Some(scope) = self.scopes.last_mut() {
                scope.push((name.to_string(), ty.clone()));
            }
        }
    }

    fn pop_scope(&mut self) {
        if let Some(slots) = self.scopes.pop() {
            for (name, ty) in slots.into_iter().rev() {
                if let Some(l) = self.drop_line(&ty, &name) {
                    self.line(&l);
                }
            }
        }
    }

    /// Emits drop statements for all scopes of the current function. The
    /// scope stack is intentionally left intact: the drops belong to one
    /// exit path (return/`?` error), while fall-through paths emit their
    /// own drops when their scopes are closed.
    fn drop_all_scopes(&mut self) {
        for l in self.scope_drop_lines() {
            self.line(&l);
        }
    }

    /// Emits drop statements for scopes at `index` and above (break/continue
    /// exit paths), leaving the scope stack intact for fall-through code.
    fn drop_scopes_from(&mut self, index: usize) {
        let mut lines = Vec::new();
        for scope in self.scopes[index..].iter().rev() {
            for (name, ty) in scope.iter().rev() {
                if let Some(l) = self.drop_line(ty, name) {
                    lines.push(l);
                }
            }
        }
        for l in lines {
            self.line(&l);
        }
    }

    fn scope_drop_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for scope in self.scopes.iter().rev() {
            for (name, ty) in scope.iter().rev() {
                if let Some(l) = self.drop_line(ty, name) {
                    lines.push(l);
                }
            }
        }
        lines
    }

    fn temp_drop_lines(&self, from: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for (name, ty) in self.stmt_temps[from..].iter().rev() {
            if let Some(l) = self.drop_line(ty, name) {
                lines.push(l);
            }
        }
        lines
    }

    fn begin_stmt(&mut self) -> usize {
        self.prelude.clear();
        self.stmt_temps.len()
    }

    fn end_stmt(&mut self, mark: usize) {
        let temps = self.stmt_temps.split_off(mark);
        for (name, ty) in temps.into_iter().rev() {
            if let Some(l) = self.drop_line(&ty, &name) {
                self.line(&l);
            }
        }
    }

    fn emit_block_scoped(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        self.pop_scope();
    }

    fn emit_structs(&mut self) {
        let instantiations = self.checked.instantiations.clone();
        for ty in &instantiations {
            match ty {
                Type::Struct(name, args) => self.emit_struct_typedef(name, args),
                Type::Enum(name, args) => self.emit_enum_typedef(name, args),
                _ => {}
            }
        }
    }

    fn emit_struct_typedef(&mut self, name: &str, args: &[Type]) {
        let info = match self.checked.structs.get(name) {
            Some(info) => info.clone(),
            None => return,
        };
        let bindings: HashMap<String, Type> = info
            .params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        self.line("typedef struct {");
        self.indent += 1;
        for f in &info.fields {
            let ft = substitute_type(&f.ty, &bindings);
            self.line(&format!("{} {};", c_type(&ft), field_c_name(&f.name)));
        }
        self.indent -= 1;
        self.line(&format!("}} bloom_struct_{};", named_mangle(name, args)));
        self.blank();
    }

    fn emit_enum_typedef(&mut self, name: &str, args: &[Type]) {
        let info = match self.checked.enums.get(name) {
            Some(info) => info.clone(),
            None => return,
        };
        let bindings: HashMap<String, Type> = info
            .params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        self.line("typedef struct {");
        self.indent += 1;
        self.line("int32_t tag;");
        self.line("union {");
        self.indent += 1;
        for v in &info.variants {
            let member = format!("m_{}", c_ident(&v.name));
            if v.fields.is_empty() {
                self.line(&format!("char {};", member));
            } else {
                self.line("struct {");
                self.indent += 1;
                for (i, f) in v.fields.iter().enumerate() {
                    let ft = substitute_type(f, &bindings);
                    if let Some(names) = &v.field_names {
                        let fname = names.get(i).cloned().unwrap_or_else(|| format!("_{}", i));
                        self.line(&format!("{} {};", c_type(&ft), field_c_name(&fname)));
                    } else {
                        self.line(&format!("{} _{};", c_type(&ft), i));
                    }
                }
                self.indent -= 1;
                self.line(&format!("}} {};", member));
            }
        }
        self.indent -= 1;
        self.line("} data;");
        self.indent -= 1;
        self.line(&format!("}} bloom_enum_{};", named_mangle(name, args)));
        self.blank();
    }

    /// Emits the C implementations of loaded standard library modules.
    /// These come after the type definitions because the helpers use the
    /// monomorphized `Result`/struct types.
    fn emit_stdlib_runtime(&mut self) {
        let mut loaded: HashSet<String> = HashSet::new();
        for sig in &self.checked.functions {
            if sig.module.starts_with("std::") {
                loaded.insert(sig.module.clone());
            }
        }
        if loaded.contains("std::time") {
            self.out.push_str(include_str!("../runtime/bloom_time.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::fs") {
            self.out.push_str(include_str!("../runtime/bloom_fs.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::db") {
            self.out.push_str(include_str!("../runtime/bloom_db.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::process") {
            self.out
                .push_str(include_str!("../runtime/bloom_process.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::win32") {
            self.out.push_str(include_str!("../runtime/bloom_win32.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::net")
            || loaded.contains("std::http")
            || loaded.contains("std::tls")
        {
            self.out.push_str(include_str!("../runtime/bloom_net.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::tls") || loaded.contains("std::http") {
            self.out.push_str(include_str!("../runtime/bloom_tls.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::http") {
            self.out.push_str(include_str!("../runtime/bloom_http.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::path") {
            self.out.push_str(include_str!("../runtime/bloom_path.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::json")
            || loaded.contains("std::web")
            || loaded.contains("std::html")
        {
            self.out.push_str(include_str!("../runtime/bloom_json.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::web") {
            self.out.push_str(include_str!("../runtime/bloom_web.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::auth") {
            self.out.push_str(include_str!("../runtime/bloom_auth.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::html") {
            self.out.push_str(include_str!("../runtime/bloom_html.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::ui") {
            self.out.push_str(include_str!("../runtime/bloom_ui.c"));
            self.out.push('\n');
        }
        if loaded.contains("std::desktop") {
            self.out
                .push_str(include_str!("../runtime/bloom_desktop.c"));
            self.out.push('\n');
        }
    }

    fn emit_globals(&mut self) {
        for g in &self.checked.globals {
            let line = format!("{} {};", c_type(&g.ty), g.c_name);
            self.line(&line);
        }
        if !self.checked.globals.is_empty() {
            self.blank();
        }
    }

    /// Emits clone/drop helpers for structs and enums that contain managed
    /// (heap-owning) fields. Instantiations are in topological order, so a
    /// helper only references helpers emitted earlier.
    fn emit_composite_helpers(&mut self) {
        // The JSON/web runtimes are emitted after the composite helpers, but
        // the generated clone/drop helpers for `std::json::JsonValue` and
        // `std::web::Router` reference their retain/release functions —
        // declare them up front.
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::json")
        {
            self.out.push_str(
                "static bloom_json_handle bloom_json_retain(bloom_json_handle h);\n\
                 static void bloom_json_release(bloom_json_handle *h);\n\n",
            );
        }
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::web")
        {
            self.out.push_str(
                "static bloom_web_handle bloom_web_retain(bloom_web_handle h);\n\
                 static void bloom_web_release(bloom_web_handle *h);\n\
                 static bloom_form_handle bloom_web_form_retain(bloom_form_handle h);\n\
                 static void bloom_web_form_release(bloom_form_handle *h);\n\
                 static bloom_cookies_handle bloom_web_cookies_retain(bloom_cookies_handle h);\n\
                 static void bloom_web_cookies_release(bloom_cookies_handle *h);\n\
                 static bloom_session_store_handle bloom_web_session_store_retain(bloom_session_store_handle h);\n\
                 static void bloom_web_session_store_release(bloom_session_store_handle *h);\n\
                 static bloom_session_handle bloom_web_session_retain(bloom_session_handle h);\n\
                 static void bloom_web_session_release(bloom_session_handle *h);\n\n",
            );
        }
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::html")
        {
            self.out.push_str(
                "static bloom_html_handle bloom_html_retain(bloom_html_handle h);\n\
                 static void bloom_html_release(bloom_html_handle *h);\n\n",
            );
        }
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::fs")
        {
            self.out.push_str(
                "bloom_fs_handle bloom_fs_bytes_retain(bloom_fs_handle h);\n\
                 void bloom_fs_bytes_release(bloom_fs_handle *h);\n\n",
            );
        }
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::ui")
        {
            self.out.push_str(
                "static bloom_ui_handle bloom_ui_retain(bloom_ui_handle h);\n\
                 static void bloom_ui_release(bloom_ui_handle *h);\n\n",
            );
        }
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::desktop")
        {
            self.out.push_str(
                "static bloom_desktop_window_handle bloom_desktop_window_retain(bloom_desktop_window_handle h);\n\
                 static void bloom_desktop_window_release(bloom_desktop_window_handle *h);\n\n",
            );
        }
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::auth")
        {
            self.out.push_str(
                "static bloom_auth_limiter_handle bloom_auth_limiter_retain(bloom_auth_limiter_handle h);\n\
                 static void bloom_auth_limiter_release(bloom_auth_limiter_handle *h);\n\n",
            );
        }
        if self
            .checked
            .functions
            .iter()
            .any(|sig| sig.module == "std::db")
        {
            self.out.push_str(
                "static bloom_db_handle bloom_db_retain(bloom_db_handle h);\n\
                 static void bloom_db_release(bloom_db_handle *h);\n\
                 static bloom_stmt_handle bloom_db_stmt_retain(bloom_stmt_handle h);\n\
                 static void bloom_db_stmt_release(bloom_stmt_handle *h);\n\n",
            );
        }
        let instantiations = self.checked.instantiations.clone();
        for ty in &instantiations {
            match ty {
                Type::Struct(name, args) => {
                    if !self.is_managed(ty) {
                        continue;
                    }
                    let info = match self.checked.structs.get(name) {
                        Some(i) => i.clone(),
                        None => continue,
                    };
                    let bindings: HashMap<String, Type> = info
                        .params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    let ct = c_type(ty);
                    let mangled = named_mangle(name, args);
                    self.line(&format!(
                        "static {} bloom_clone_{}({} v) {{",
                        ct, mangled, ct
                    ));
                    self.indent += 1;
                    for f in &info.fields {
                        let ft = substitute_type(&f.ty, &bindings);
                        if let Some(fname) = self.clone_fn_name(&ft) {
                            self.line(&format!(
                                "v.{} = {}(v.{});",
                                field_c_name(&f.name),
                                fname,
                                field_c_name(&f.name)
                            ));
                        }
                    }
                    self.line("return v;");
                    self.indent -= 1;
                    self.line("}");
                    self.line(&format!("static void bloom_drop_{}({} *p) {{", mangled, ct));
                    self.indent += 1;
                    for f in &info.fields {
                        let ft = substitute_type(&f.ty, &bindings);
                        if let Some(fname) = self.drop_fn_name(&ft) {
                            self.line(&format!("{}(&p->{});", fname, field_c_name(&f.name)));
                        }
                    }
                    self.indent -= 1;
                    self.line("}");
                    self.blank();
                }
                Type::Enum(name, args) => {
                    if !self.is_managed(ty) {
                        continue;
                    }
                    let info = match self.checked.enums.get(name) {
                        Some(i) => i.clone(),
                        None => continue,
                    };
                    let bindings: HashMap<String, Type> = info
                        .params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    let ct = c_type(ty);
                    let mangled = named_mangle(name, args);
                    self.line(&format!(
                        "static {} bloom_clone_{}({} v) {{",
                        ct, mangled, ct
                    ));
                    self.indent += 1;
                    self.line("switch (v.tag) {");
                    self.indent += 1;
                    for (tag, var) in info.variants.iter().enumerate() {
                        let mut clones = Vec::new();
                        for (i, f) in var.fields.iter().enumerate() {
                            let ft = substitute_type(f, &bindings);
                            if let Some(fname) = self.clone_fn_name(&ft) {
                                let access = match &var.field_names {
                                    Some(names) => format!(
                                        "v.data.m_{}.{}",
                                        c_ident(&var.name),
                                        field_c_name(&names[i])
                                    ),
                                    None => format!("v.data.m_{}._{}", c_ident(&var.name), i),
                                };
                                clones.push(format!("{} = {}({});", access, fname, access));
                            }
                        }
                        if !clones.is_empty() {
                            self.line(&format!("case {}:", tag));
                            self.indent += 1;
                            for c in clones {
                                self.line(&c);
                            }
                            self.line("break;");
                            self.indent -= 1;
                        }
                    }
                    self.indent -= 1;
                    self.line("}");
                    self.line("return v;");
                    self.indent -= 1;
                    self.line("}");
                    self.line(&format!("static void bloom_drop_{}({} *p) {{", mangled, ct));
                    self.indent += 1;
                    self.line("switch (p->tag) {");
                    self.indent += 1;
                    for (tag, var) in info.variants.iter().enumerate() {
                        let mut drops = Vec::new();
                        for (i, f) in var.fields.iter().enumerate() {
                            let ft = substitute_type(f, &bindings);
                            if let Some(fname) = self.drop_fn_name(&ft) {
                                let access = match &var.field_names {
                                    Some(names) => format!(
                                        "p->data.m_{}.{}",
                                        c_ident(&var.name),
                                        field_c_name(&names[i])
                                    ),
                                    None => format!("p->data.m_{}._{}", c_ident(&var.name), i),
                                };
                                drops.push(format!("{}(&{});", fname, access));
                            }
                        }
                        if !drops.is_empty() {
                            self.line(&format!("case {}:", tag));
                            self.indent += 1;
                            for d in drops {
                                self.line(&d);
                            }
                            self.line("break;");
                            self.indent -= 1;
                        }
                    }
                    self.indent -= 1;
                    self.line("}");
                    self.line("p->tag = -1;");
                    self.indent -= 1;
                    self.line("}");
                    self.blank();
                }
                _ => {}
            }
        }
    }

    fn emit_prototypes(&mut self) {
        let mut emitted = false;
        for sig in &self.checked.functions {
            if !sig.type_params.is_empty() {
                continue;
            }
            if sig.module.starts_with("std::") {
                continue;
            }
            let params: Vec<String> = if sig.params.is_empty() {
                vec!["void".to_string()]
            } else {
                sig.params.iter().map(c_type).collect()
            };
            if sig.is_extern {
                if sig.system_abi {
                    self.line(&format!(
                        "extern {} BLOOM_SYSTEM_ABI {}({});",
                        c_type(&sig.ret),
                        sig.c_name,
                        params.join(", ")
                    ));
                } else {
                    self.line(&format!(
                        "extern {} {}({});",
                        c_type(&sig.ret),
                        sig.c_name,
                        params.join(", ")
                    ));
                }
                emitted = true;
                continue;
            }
            self.line(&format!(
                "{} {}({});",
                c_type(&sig.ret),
                sig.c_name,
                params.join(", ")
            ));
            emitted = true;
        }
        for inst in &self.checked.fn_instantiations {
            let sig = &self.checked.functions[inst.index];
            let bindings: HashMap<String, Type> = sig
                .type_params
                .iter()
                .cloned()
                .zip(inst.type_args.iter().cloned())
                .collect();
            let params: Vec<String> = if sig.params.is_empty() {
                vec!["void".to_string()]
            } else {
                sig.params
                    .iter()
                    .map(|p| c_type(&substitute_type(p, &bindings)))
                    .collect()
            };
            self.line(&format!(
                "{} {}({});",
                c_type(&substitute_type(&sig.ret, &bindings)),
                inst.c_name,
                params.join(", ")
            ));
            emitted = true;
        }
        for methods in self.checked.methods.values() {
            let mut names: Vec<&String> = methods.keys().collect();
            names.sort();
            for name in names {
                let info = &methods[name];
                if !info.type_params.is_empty() {
                    continue;
                }
                let mut params = Vec::new();
                if let Some(receiver) = info.receiver {
                    params.push(self.method_receiver_c_type(&info.owner, &[], receiver));
                }
                params.extend(info.params.iter().map(c_type));
                self.line(&format!(
                    "{} {}({});",
                    c_type(&info.ret),
                    info.c_name,
                    params.join(", ")
                ));
                emitted = true;
            }
        }
        for inst in &self.checked.method_instantiations {
            let info = match self
                .checked
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
            let mut params = Vec::new();
            if let Some(receiver) = info.receiver {
                params.push(self.method_receiver_c_type(&info.owner, &inst.type_args, receiver));
            }
            params.extend(
                info.params
                    .iter()
                    .map(|p| c_type(&substitute_type(p, &bindings))),
            );
            self.line(&format!(
                "{} {}({});",
                c_type(&substitute_type(&info.ret, &bindings)),
                inst.c_name,
                params.join(", ")
            ));
            emitted = true;
        }
        if emitted {
            self.blank();
        }
    }

    fn method_receiver_c_type(&self, owner: &str, args: &[Type], receiver: ReceiverKind) -> String {
        let base = if self.checked.enums.contains_key(owner) {
            format!("bloom_enum_{}", named_mangle(owner, args))
        } else {
            format!("bloom_struct_{}", named_mangle(owner, args))
        };
        if receiver == ReceiverKind::Mutable {
            format!("{} *", base)
        } else {
            base
        }
    }

    fn emit_functions(&mut self) {
        for module in &self.checked.modules {
            self.file_lit = c_string_literal(module.path_display.as_bytes());
            for item in &module.program.items {
                let Item::Function(f) = item else { continue };
                let sig = match self.checked.functions.iter().find(|s| s.c_name == f.c_name) {
                    Some(s) => s.clone(),
                    None => continue,
                };
                if !sig.type_params.is_empty() {
                    continue;
                }
                let params: Vec<String> = f
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let ty = sig.params.get(i).cloned().unwrap_or(Type::Int);
                        format!("{} {}", c_type(&ty), self.var_c_name(p.id))
                    })
                    .collect();
                let param_str = if params.is_empty() {
                    "void".to_string()
                } else {
                    params.join(", ")
                };
                self.current_subst.clear();
                self.current_ret = sig.ret.clone();
                self.line(&format!(
                    "{} {}({}) {{",
                    c_type(&sig.ret),
                    sig.c_name,
                    param_str
                ));
                self.indent += 1;
                self.push_scope();
                for (i, p) in f.params.iter().enumerate() {
                    let ty = sig.params.get(i).cloned().unwrap_or(Type::Int);
                    let cname = self.var_c_name(p.id);
                    self.register_local(&cname, &ty);
                }
                for stmt in &f.body.stmts {
                    self.emit_stmt(stmt);
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
                self.blank();
            }
        }
        for inst in &self.checked.fn_instantiations.clone() {
            let sig = self.checked.functions[inst.index].clone();
            let bindings: HashMap<String, Type> = sig
                .type_params
                .iter()
                .cloned()
                .zip(inst.type_args.iter().cloned())
                .collect();
            let body = self.find_function_body(&sig.c_name);
            let f = match body {
                Some(f) => f,
                None => continue,
            };
            let params: Vec<String> = f
                .params
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let ty = sig.params.get(i).cloned().unwrap_or(Type::Int);
                    let ty = substitute_type(&ty, &bindings);
                    format!("{} {}", c_type(&ty), self.var_c_name(p.id))
                })
                .collect();
            let param_str = if params.is_empty() {
                "void".to_string()
            } else {
                params.join(", ")
            };
            self.current_subst = bindings.clone();
            self.current_ret = substitute_type(&sig.ret, &bindings);
            self.line(&format!(
                "{} {}({}) {{",
                c_type(&self.current_ret.clone()),
                inst.c_name,
                param_str
            ));
            self.indent += 1;
            self.push_scope();
            for (i, p) in f.params.iter().enumerate() {
                let ty = sig.params.get(i).cloned().unwrap_or(Type::Int);
                let ty = substitute_type(&ty, &bindings);
                let cname = self.var_c_name(p.id);
                self.register_local(&cname, &ty);
            }
            for stmt in &f.body.stmts {
                self.emit_stmt(stmt);
            }
            self.pop_scope();
            self.indent -= 1;
            self.line("}");
            self.blank();
            self.current_subst.clear();
        }
    }

    fn find_function_body(&self, c_name: &str) -> Option<Function> {
        for module in &self.checked.modules {
            for item in &module.program.items {
                if let Item::Function(f) = item {
                    if f.c_name == c_name {
                        return Some(f.clone());
                    }
                }
            }
        }
        None
    }

    fn emit_methods(&mut self) {
        for module in &self.checked.modules {
            self.file_lit = c_string_literal(module.path_display.as_bytes());
            for item in &module.program.items {
                let Item::Impl(b) = item else { continue };
                for m in &b.methods {
                    let info = match self
                        .checked
                        .methods
                        .get(&b.canonical_type)
                        .and_then(|ms| ms.get(&m.name))
                    {
                        Some(i) => i.clone(),
                        None => continue,
                    };
                    if !info.type_params.is_empty() {
                        continue;
                    }
                    let mut params = Vec::new();
                    if let Some(receiver) = info.receiver {
                        params.push(format!(
                            "{} {}",
                            self.method_receiver_c_type(&info.owner, &[], receiver),
                            self.var_c_name(m.self_id)
                        ));
                    }
                    for (i, p) in m.params.iter().enumerate() {
                        let ty = info.params.get(i).cloned().unwrap_or(Type::Int);
                        params.push(format!("{} {}", c_type(&ty), self.var_c_name(p.id)));
                    }
                    self.current_subst.clear();
                    self.current_ret = info.ret.clone();
                    self.line(&format!(
                        "{} {}({}) {{",
                        c_type(&info.ret),
                        info.c_name,
                        params.join(", ")
                    ));
                    self.indent += 1;
                    self.push_scope();
                    for (i, p) in m.params.iter().enumerate() {
                        let ty = info.params.get(i).cloned().unwrap_or(Type::Int);
                        let cname = self.var_c_name(p.id);
                        self.register_local(&cname, &ty);
                    }
                    for stmt in &m.body.stmts {
                        self.emit_stmt(stmt);
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    self.line("}");
                    self.blank();
                }
            }
        }
        for inst in &self.checked.method_instantiations.clone() {
            let info = match self
                .checked
                .methods
                .get(&inst.owner)
                .and_then(|ms| ms.get(&inst.method))
            {
                Some(info) => info.clone(),
                None => continue,
            };
            let m = match self.find_method_body(&info.owner, &info.name) {
                Some(m) => m,
                None => continue,
            };
            let bindings: HashMap<String, Type> = info
                .type_params
                .iter()
                .cloned()
                .zip(inst.type_args.iter().cloned())
                .collect();
            let mut params = Vec::new();
            if let Some(receiver) = info.receiver {
                params.push(format!(
                    "{} {}",
                    self.method_receiver_c_type(&info.owner, &inst.type_args, receiver),
                    self.var_c_name(m.self_id)
                ));
            }
            for (i, p) in m.params.iter().enumerate() {
                let ty = info.params.get(i).cloned().unwrap_or(Type::Int);
                let ty = substitute_type(&ty, &bindings);
                params.push(format!("{} {}", c_type(&ty), self.var_c_name(p.id)));
            }
            self.current_subst = bindings.clone();
            self.current_ret = substitute_type(&info.ret, &bindings);
            self.line(&format!(
                "{} {}({}) {{",
                c_type(&self.current_ret.clone()),
                inst.c_name,
                params.join(", ")
            ));
            self.indent += 1;
            self.push_scope();
            for (i, p) in m.params.iter().enumerate() {
                let ty = info.params.get(i).cloned().unwrap_or(Type::Int);
                let ty = substitute_type(&ty, &bindings);
                let cname = self.var_c_name(p.id);
                self.register_local(&cname, &ty);
            }
            for stmt in &m.body.stmts {
                self.emit_stmt(stmt);
            }
            self.pop_scope();
            self.indent -= 1;
            self.line("}");
            self.blank();
            self.current_subst.clear();
        }
    }

    fn find_method_body(&self, owner: &str, method: &str) -> Option<Method> {
        for module in &self.checked.modules {
            for item in &module.program.items {
                if let Item::Impl(b) = item {
                    if b.canonical_type == owner {
                        for m in &b.methods {
                            if m.name == method {
                                return Some(m.clone());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn emit_main_wrapper(&mut self) {
        let main_c = self
            .checked
            .functions
            .get(self.checked.main_index)
            .map(|s| s.c_name.clone())
            .unwrap_or_else(|| "bf_main".to_string());
        self.line("int main(void) {");
        self.indent += 1;
        self.line("bloom_init();");
        for g in &self.checked.globals {
            let value = const_c_value(&g.value, &g.ty);
            let line = format!("{} = {};", g.c_name, value);
            self.line(&line);
        }
        self.line(&format!("int bloom_main_result = (int){}();", main_c));
        for g in &self.checked.globals {
            if let Some(l) = self.drop_line(&g.ty, &g.c_name) {
                self.line(&l);
            }
        }
        self.line("bloom_mem_report();");
        self.line("return bloom_main_result;");
        self.indent -= 1;
        self.line("}");
    }

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(decl) => {
                let mark = self.begin_stmt();
                let ty = self.var_type(decl.id);
                let cname = self.var_c_name(decl.id);
                match &decl.init {
                    Some(init) => {
                        let text = self.expr_coerced(init, &ty);
                        let owned = self.take_owned(&text);
                        let init_text = if owned || !self.is_managed(&ty) {
                            text
                        } else {
                            self.clone_text(&ty, &text)
                        };
                        self.flush_prelude();
                        self.line(&format!("{} {} = {};", c_type(&ty), cname, init_text));
                    }
                    None => {
                        self.flush_prelude();
                        if self.is_managed(&ty) {
                            self.line(&format!("{} {} = {{0}};", c_type(&ty), cname));
                        } else {
                            self.line(&format!("{} {};", c_type(&ty), cname));
                        }
                    }
                }
                self.register_local(&cname, &ty);
                self.end_stmt(mark);
            }
            Stmt::Expr { expr, .. } => {
                let mark = self.begin_stmt();
                if let ExprKind::Match { scrutinee, arms } = &expr.kind {
                    let ty = self.expr_type(expr);
                    let (lines, _) =
                        self.emit_match_lines(expr.id, scrutinee, arms, ty != Type::Void, &ty);
                    for l in lines {
                        self.line(&l);
                    }
                    self.end_stmt(mark);
                    return;
                }
                let text = self.emit_expr(expr);
                self.flush_prelude();
                if matches!(expr.kind, ExprKind::Call { .. }) {
                    self.line(&format!("{};", text));
                } else {
                    self.line(&format!("(void)({});", text));
                }
                self.end_stmt(mark);
            }
            Stmt::If {
                cond,
                then_block,
                else_branch,
                ..
            } => {
                let mark = self.begin_stmt();
                let cond_text = self.emit_expr(cond);
                self.flush_prelude();
                self.line(&format!("if ({}) {{", cond_text));
                self.indent += 1;
                self.emit_block_scoped(then_block);
                self.indent -= 1;
                match else_branch {
                    None => self.line("}"),
                    Some(ElseBranch::Block(b)) => {
                        self.line("} else {");
                        self.indent += 1;
                        self.emit_block_scoped(b);
                        self.indent -= 1;
                        self.line("}");
                    }
                    Some(ElseBranch::If(s)) => {
                        self.line("} else {");
                        self.indent += 1;
                        self.emit_stmt(s);
                        self.indent -= 1;
                        self.line("}");
                    }
                }
                self.end_stmt(mark);
            }
            Stmt::While { cond, body, .. } => {
                let mark = self.begin_stmt();
                let cond_text = self.emit_expr(cond);
                let prelude = std::mem::take(&mut self.prelude);
                if prelude.is_empty() && self.stmt_temps.len() == mark {
                    self.line(&format!("while ({}) {{", cond_text));
                    self.indent += 1;
                    self.loop_marks.push(self.scopes.len());
                    self.emit_block_scoped(body);
                    self.loop_marks.pop();
                    self.indent -= 1;
                    self.line("}");
                } else {
                    self.line("while (true) {");
                    self.indent += 1;
                    for p in prelude {
                        self.line(&p);
                    }
                    self.line(&format!("if (!({})) {{ break; }}", cond_text));
                    let cond_temps = self.stmt_temps.split_off(mark);
                    for (name, ty) in cond_temps.into_iter().rev() {
                        if let Some(l) = self.drop_line(&ty, &name) {
                            self.line(&l);
                        }
                    }
                    self.loop_marks.push(self.scopes.len());
                    self.emit_block_scoped(body);
                    self.loop_marks.pop();
                    self.indent -= 1;
                    self.line("}");
                }
                self.end_stmt(mark);
            }
            Stmt::For {
                decl,
                range_start,
                range_end,
                body,
                ..
            } => {
                let mark = self.begin_stmt();
                let start_text = self.emit_expr(range_start);
                let start_prelude = std::mem::take(&mut self.prelude);
                let end_text = self.emit_expr(range_end);
                let end_prelude = std::mem::take(&mut self.prelude);
                for p in start_prelude {
                    self.line(&p);
                }
                for p in end_prelude {
                    self.line(&p);
                }
                let end_var = self.fresh_temp();
                let cname = self.var_c_name(decl.id);
                self.line(&format!("int64_t {} = {};", end_var, end_text));
                let range_temps = self.stmt_temps.split_off(mark);
                for (name, ty) in range_temps.into_iter().rev() {
                    if let Some(l) = self.drop_line(&ty, &name) {
                        self.line(&l);
                    }
                }
                self.line(&format!(
                    "for (int64_t {} = {}; {} < {}; {}++) {{",
                    cname, start_text, cname, end_var, cname
                ));
                self.indent += 1;
                self.loop_marks.push(self.scopes.len());
                self.emit_block_scoped(body);
                self.loop_marks.pop();
                self.indent -= 1;
                self.line("}");
                self.end_stmt(mark);
            }
            Stmt::Return { value, .. } => {
                let mark = self.begin_stmt();
                let ret = self.current_ret.clone();
                match value {
                    Some(expr) => {
                        let text = self.expr_coerced(expr, &ret);
                        let owned = self.take_owned(&text);
                        let managed_ret = self.is_managed(&ret);
                        let has_scope_drops = self.scopes.iter().any(|s| !s.is_empty());
                        let has_stmt_drops = self.stmt_temps.len() > mark;
                        self.flush_prelude();
                        if !has_scope_drops && !has_stmt_drops {
                            let final_text = if !owned && managed_ret {
                                self.clone_text(&ret, &text)
                            } else {
                                text
                            };
                            self.line(&format!("return {};", final_text));
                            self.stmt_temps.truncate(mark);
                            return;
                        }
                        let tmp = self.fresh_temp();
                        let init = if owned {
                            text
                        } else if managed_ret {
                            self.clone_text(&ret, &text)
                        } else {
                            text
                        };
                        self.line(&format!("{} {} = {};", c_type(&ret), tmp, init));
                        self.end_stmt(mark);
                        self.drop_all_scopes();
                        self.line(&format!("return {};", tmp));
                    }
                    None => {
                        self.flush_prelude();
                        self.end_stmt(mark);
                        self.drop_all_scopes();
                        self.line("return;");
                    }
                }
            }
            Stmt::Break { .. } => {
                if let Some(&mark) = self.loop_marks.last() {
                    self.drop_scopes_from(mark);
                }
                self.line("break;");
            }
            Stmt::Continue { .. } => {
                if let Some(&mark) = self.loop_marks.last() {
                    self.drop_scopes_from(mark);
                }
                self.line("continue;");
            }
            Stmt::Block(b) => {
                self.line("{");
                self.indent += 1;
                self.emit_block_scoped(b);
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Unsafe(b) => {
                self.line("{");
                self.indent += 1;
                self.emit_block_scoped(b);
                self.indent -= 1;
                self.line("}");
            }
        }
    }

    fn expr_coerced(&mut self, expr: &Expr, target: &Type) -> String {
        let text = self.emit_expr(expr);
        let ty = self.expr_type(expr);
        if ty == Type::Int && *target == Type::Float {
            format!("((double)({}))", text)
        } else {
            text
        }
    }

    fn emit_expr(&mut self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(v) => format!("{}LL", v),
            ExprKind::Float(v) => format_float_literal(*v),
            ExprKind::Str(s) => self.literal_name(s.as_bytes()),
            ExprKind::Char(c) => format!("(uint32_t){}u", *c as u32),
            ExprKind::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            ExprKind::Ident(_) => match self.checked.ident_refs.get(&expr.id) {
                Some(id) => {
                    let name = self.var_c_name(*id);
                    if self.pointer_vars.contains(id) {
                        format!("(*{})", name)
                    } else {
                        name
                    }
                }
                None => "0".to_string(),
            },
            ExprKind::Path(_, _) => {
                if let Some(&index) = self.checked.global_refs.get(&expr.id) {
                    if let Some(g) = self.checked.globals.get(index) {
                        return g.c_name.clone();
                    }
                }
                if let Some(vref) = self.checked.variant_refs.get(&expr.id).cloned() {
                    let tmp = self.fresh_temp();
                    let args: Vec<Type> = vref.enum_args.iter().map(|t| self.subst(t)).collect();
                    let enum_ty = Type::Enum(vref.enum_name.clone(), args.clone());
                    self.prelude
                        .push(format!("{} {};", enum_c_name(&vref.enum_name, &args), tmp));
                    self.prelude.push(format!("{}.tag = {};", tmp, vref.tag));
                    if self.is_managed(&enum_ty) {
                        self.stmt_temps.push((tmp.clone(), enum_ty));
                    }
                    tmp
                } else {
                    "0".to_string()
                }
            }
            ExprKind::Unary { op, operand } => {
                let text = self.emit_expr(operand);
                let ty = self.expr_type(operand);
                match op {
                    UnaryOp::Neg => {
                        if ty == Type::Float {
                            format!("(-({}))", text)
                        } else {
                            format!("bloom_ineg({})", text)
                        }
                    }
                    UnaryOp::Not => format!("(!({}))", text),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.expr_type(lhs);
                let rt = self.expr_type(rhs);
                let result = self.expr_type(expr);
                let l = self.emit_expr(lhs);
                let r = self.emit_expr(rhs);
                let text = self.emit_binary(*op, &lt, &rt, &result, l, r, expr);
                if self.is_managed(&result) {
                    self.owned_temp(&result, &text)
                } else {
                    text
                }
            }
            ExprKind::Assign { target, value } => {
                let target_ty = self.expr_type(target);
                let value_text = self.expr_coerced(value, &target_ty);
                match &target.kind {
                    ExprKind::Ident(_) => {
                        let cname = match self.checked.ident_refs.get(&target.id) {
                            Some(id) => self.var_c_name(*id),
                            None => "0".to_string(),
                        };
                        if self.is_managed(&target_ty) {
                            let owned = self.take_owned(&value_text);
                            let val = if owned {
                                value_text
                            } else {
                                self.clone_text(&target_ty, &value_text)
                            };
                            let tmp = self.fresh_temp();
                            self.prelude
                                .push(format!("{} {} = {};", c_type(&target_ty), tmp, val));
                            if let Some(l) = self.drop_line(&target_ty, &cname) {
                                self.prelude.push(l);
                            }
                            self.prelude.push(format!("{} = {};", cname, tmp));
                            tmp
                        } else {
                            format!("({} = {})", cname, value_text)
                        }
                    }
                    ExprKind::Index { base, index } => {
                        let base_text = self.emit_expr(base);
                        let index_text = self.emit_expr(index);
                        let base_ty = self.expr_type(base);
                        let (elem_ty, suffix, is_slice) = match &base_ty {
                            Type::Array(elem) => {
                                let e = (**elem).clone();
                                (e.clone(), arr_suffix(&e), false)
                            }
                            Type::Slice(elem) => {
                                let e = (**elem).clone();
                                (e.clone(), arr_suffix(&e), true)
                            }
                            _ => (Type::Int, "i64", false),
                        };
                        let tmp = self.fresh_temp();
                        self.prelude.push(format!(
                            "{} {} = {};",
                            c_type(&elem_ty),
                            tmp,
                            value_text
                        ));
                        if is_slice {
                            self.prelude.push(format!(
                                "bloom_slice_{}_set({}, {}, {}, {}, {});",
                                suffix,
                                base_text,
                                index_text,
                                tmp,
                                self.file_lit,
                                expr.span.start.line
                            ));
                        } else {
                            self.prelude.push(format!(
                                "bloom_arr_{}_set({}, {}, {}, {}, {});",
                                suffix,
                                base_text,
                                index_text,
                                tmp,
                                self.file_lit,
                                expr.span.start.line
                            ));
                        }
                        tmp
                    }
                    ExprKind::Field { base, name, .. } => {
                        let base_text = self.emit_expr(base);
                        if self.is_managed(&target_ty) {
                            let owned = self.take_owned(&value_text);
                            let val = if owned {
                                value_text
                            } else {
                                self.clone_text(&target_ty, &value_text)
                            };
                            let tmp = self.fresh_temp();
                            self.prelude
                                .push(format!("{} {} = {};", c_type(&target_ty), tmp, val));
                            let place = format!("({}).{}", base_text, field_c_name(name));
                            if let Some(l) = self.drop_line(&target_ty, &place) {
                                self.prelude.push(l);
                            }
                            self.prelude.push(format!("{} = {};", place, tmp));
                            tmp
                        } else {
                            format!("(({}).{} = {})", base_text, field_c_name(name), value_text)
                        }
                    }
                    _ => "0".to_string(),
                }
            }
            ExprKind::Call {
                callee,
                type_args: _,
                args,
            } => self.emit_call(expr.id, callee, args),
            ExprKind::Try(operand) => self.emit_try(expr.id, operand),
            ExprKind::Index { base, index } => {
                let base_text = self.emit_expr(base);
                let index_text = self.emit_expr(index);
                let base_ty = self.expr_type(base);
                match &base_ty {
                    Type::Array(elem) => format!(
                        "bloom_arr_{}_get({}, {}, {}, {})",
                        arr_suffix(elem),
                        base_text,
                        index_text,
                        self.file_lit,
                        expr.span.start.line
                    ),
                    Type::Slice(elem) => format!(
                        "bloom_slice_{}_get({}, {}, {}, {})",
                        arr_suffix(elem),
                        base_text,
                        index_text,
                        self.file_lit,
                        expr.span.start.line
                    ),
                    Type::String => format!(
                        "bloom_str_char_at({}, {}, {}, {})",
                        base_text, index_text, self.file_lit, expr.span.start.line
                    ),
                    _ => "0".to_string(),
                }
            }
            ExprKind::Field { base, name, .. } => {
                let base_text = self.emit_expr(base);
                format!("(({}).{})", base_text, field_c_name(name))
            }
            ExprKind::StructLit { name, fields, .. } => {
                if let Some(vref) = self.checked.variant_refs.get(&expr.id).cloned() {
                    let tmp = self.fresh_temp();
                    let args: Vec<Type> = vref.enum_args.iter().map(|t| self.subst(t)).collect();
                    let enum_ty = Type::Enum(vref.enum_name.clone(), args.clone());
                    self.prelude
                        .push(format!("{} {};", enum_c_name(&vref.enum_name, &args), tmp));
                    self.prelude.push(format!("{}.tag = {};", tmp, vref.tag));
                    let member = format!("m_{}", c_ident(&vref.variant_name));
                    for f in fields {
                        let idx = vref
                            .field_names
                            .as_ref()
                            .and_then(|names| names.iter().position(|n| *n == f.name));
                        let (field_ty, access) = match idx {
                            Some(i) => (
                                vref.field_types
                                    .get(i)
                                    .cloned()
                                    .map(|t| self.subst(&t))
                                    .unwrap_or(Type::Int),
                                format!("{}.data.{}.{}", tmp, member, field_c_name(&f.name)),
                            ),
                            None => (Type::Int, format!("{}.data.{}.x", tmp, member)),
                        };
                        let text = self.expr_coerced(&f.value, &field_ty);
                        let owned = self.take_owned(&text);
                        let val = if owned || !self.is_managed(&field_ty) {
                            text
                        } else {
                            self.clone_text(&field_ty, &text)
                        };
                        self.prelude.push(format!("{} = {};", access, val));
                    }
                    if self.is_managed(&enum_ty) {
                        self.stmt_temps.push((tmp.clone(), enum_ty));
                    }
                    return tmp;
                }
                let resolved = self.expr_type(expr);
                let (type_name, type_args) = match &resolved {
                    Type::Struct(canonical, args) => (canonical.clone(), args.clone()),
                    _ => (name.clone(), Vec::new()),
                };
                let def = self.checked.structs.get(&type_name).cloned();
                let bindings: HashMap<String, Type> = def
                    .as_ref()
                    .map(|d| {
                        d.params
                            .iter()
                            .cloned()
                            .zip(type_args.iter().cloned())
                            .collect()
                    })
                    .unwrap_or_default();
                let tmp = self.fresh_temp();
                self.prelude.push(format!(
                    "{} {};",
                    c_type(&Type::Struct(type_name.clone(), type_args.clone())),
                    tmp
                ));
                for f in fields {
                    let field_ty = def
                        .as_ref()
                        .and_then(|d| d.field(&f.name))
                        .map(|d| substitute_type(&d.ty, &bindings))
                        .map(|t| self.subst(&t))
                        .unwrap_or(Type::Int);
                    let text = self.expr_coerced(&f.value, &field_ty);
                    let owned = self.take_owned(&text);
                    let val = if owned || !self.is_managed(&field_ty) {
                        text
                    } else {
                        self.clone_text(&field_ty, &text)
                    };
                    self.prelude
                        .push(format!("{}.{} = {};", tmp, field_c_name(&f.name), val));
                }
                if self.is_managed(&resolved) {
                    self.stmt_temps.push((tmp.clone(), resolved));
                }
                tmp
            }
            ExprKind::Match { scrutinee, arms } => {
                let ty = self.expr_type(expr);
                let (lines, result) =
                    self.emit_match_lines(expr.id, scrutinee, arms, ty != Type::Void, &ty);
                self.prelude.extend(lines);
                result.unwrap_or_else(|| "0".to_string())
            }
            ExprKind::ArrayLit(elems) => {
                let arr_ty = self.expr_type(expr);
                let elem_ty = arr_ty.element().cloned().unwrap_or(Type::Int);
                let suffix = arr_suffix(&elem_ty);
                let tmp = self.fresh_temp();
                self.prelude.push(format!(
                    "{} {} = bloom_arr_{}_new();",
                    c_type(&arr_ty),
                    tmp,
                    suffix
                ));
                for elem in elems {
                    let text = self.expr_coerced(elem, &elem_ty);
                    self.prelude
                        .push(format!("bloom_arr_{}_push({}, {});", suffix, tmp, text));
                }
                if self.is_managed(&arr_ty) {
                    self.stmt_temps.push((tmp.clone(), arr_ty));
                }
                tmp
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_binary(
        &mut self,
        op: BinOp,
        lt: &Type,
        rt: &Type,
        result: &Type,
        l: String,
        r: String,
        expr: &Expr,
    ) -> String {
        use BinOp::*;
        let numeric_mixed = lt.is_numeric() && rt.is_numeric() && (lt != rt);
        let (lc, rc) = if numeric_mixed {
            if *lt == Type::Int {
                (format!("((double)({}))", l), r)
            } else {
                (l, format!("((double)({}))", r))
            }
        } else {
            (l, r)
        };
        match op {
            Add | Sub | Mul | Div | Mod => {
                if *result == Type::String {
                    return format!("bloom_str_concat({}, {})", lc, rc);
                }
                if *result == Type::Float {
                    let sym = op.symbol();
                    return format!("(({}) {} ({}))", lc, sym, rc);
                }
                match op {
                    Add => format!("bloom_iadd({}, {})", lc, rc),
                    Sub => format!("bloom_isub({}, {})", lc, rc),
                    Mul => format!("bloom_imul({}, {})", lc, rc),
                    Div => format!(
                        "bloom_idiv({}, {}, {}, {})",
                        lc, rc, self.file_lit, expr.span.start.line
                    ),
                    Mod => format!(
                        "bloom_imod({}, {}, {}, {})",
                        lc, rc, self.file_lit, expr.span.start.line
                    ),
                    _ => unreachable!(),
                }
            }
            Eq | Ne => {
                if *lt == Type::String && *rt == Type::String {
                    let base = format!("bloom_str_eq({}, {})", lc, rc);
                    if op == Eq {
                        base
                    } else {
                        format!("(!{})", base)
                    }
                } else {
                    let sym = if op == Eq { "==" } else { "!=" };
                    format!("(({}) {} ({}))", lc, sym, rc)
                }
            }
            Lt | Le | Gt | Ge => {
                let sym = op.symbol();
                format!("(({}) {} ({}))", lc, sym, rc)
            }
            And | Or => {
                let sym = op.symbol();
                format!("(({}) {} ({}))", lc, sym, rc)
            }
        }
    }

    fn emit_call(&mut self, id: ExprId, callee: &Expr, args: &[Expr]) -> String {
        if let ExprKind::Path(_, _) = &callee.kind {
            if let Some(vref) = self.checked.variant_refs.get(&id).cloned() {
                let tmp = self.fresh_temp();
                let enum_args: Vec<Type> = vref.enum_args.iter().map(|t| self.subst(t)).collect();
                let enum_ty = Type::Enum(vref.enum_name.clone(), enum_args.clone());
                self.prelude.push(format!(
                    "{} {};",
                    enum_c_name(&vref.enum_name, &enum_args),
                    tmp
                ));
                self.prelude.push(format!("{}.tag = {};", tmp, vref.tag));
                let member = format!("m_{}", c_ident(&vref.variant_name));
                for (i, arg) in args.iter().enumerate() {
                    let field_ty = vref
                        .field_types
                        .get(i)
                        .cloned()
                        .map(|t| self.subst(&t))
                        .unwrap_or(Type::Int);
                    let text = self.expr_coerced(arg, &field_ty);
                    let owned = self.take_owned(&text);
                    let val = if owned || !self.is_managed(&field_ty) {
                        text
                    } else {
                        self.clone_text(&field_ty, &text)
                    };
                    self.prelude
                        .push(format!("{}.data.{}._{} = {};", tmp, member, i, val));
                }
                if self.is_managed(&enum_ty) {
                    self.stmt_temps.push((tmp.clone(), enum_ty));
                }
                return tmp;
            }
        }
        match self.checked.callee_refs.get(&id).cloned() {
            Some(Callee::Builtin(kind)) => self.emit_builtin(kind, args),
            Some(Callee::User(info)) if info.base_c_name.starts_with("bloom_math_") => {
                self.emit_math_call(&info.base_c_name, args)
            }
            Some(Callee::User(info))
                if info.base_c_name.starts_with("bloom_fs_")
                    || info.base_c_name.starts_with("bloom_process_")
                    || info.base_c_name.starts_with("bloom_time_")
                    || info.base_c_name.starts_with("bloom_win32_")
                    || info.base_c_name.starts_with("bloom_net_")
                    || info.base_c_name.starts_with("bloom_tls_")
                    || info.base_c_name.starts_with("bloom_path_")
                    || info.base_c_name.starts_with("bloom_json_")
                    || info.base_c_name.starts_with("bloom_web_")
                    || info.base_c_name.starts_with("bloom_html_")
                    || info.base_c_name.starts_with("bloom_http_")
                    || info.base_c_name.starts_with("bloom_db_")
                    || info.base_c_name.starts_with("bloom_stmt_")
                    || info.base_c_name.starts_with("bloom_auth_")
                    || info.base_c_name.starts_with("bloom_ui_")
                    || info.base_c_name.starts_with("bloom_desktop_") =>
            {
                self.emit_stdlib_call(&info.base_c_name, &info.params, args, id)
            }
            Some(Callee::User(info)) => {
                let mut arg_texts = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let target = self.subst(&info.params.get(i).cloned().unwrap_or(Type::Int));
                    let text = self.expr_coerced(arg, &target);
                    let owned = self.take_owned(&text);
                    if owned || !self.is_managed(&target) {
                        arg_texts.push(text);
                    } else {
                        arg_texts.push(self.clone_text(&target, &text));
                    }
                }
                let type_args: Vec<Type> = info.type_args.iter().map(|t| self.subst(t)).collect();
                let c_name = specialized_c_name(&info.base_c_name, &type_args);
                let call = format!("{}({})", c_name, arg_texts.join(", "));
                let ret_ty = self.expr_type_by_id(id);
                if self.is_managed(&ret_ty) {
                    self.owned_temp(&ret_ty, &call)
                } else {
                    call
                }
            }
            Some(Callee::Method(info)) => {
                let mut arg_texts = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let target = self.subst(&info.params.get(i).cloned().unwrap_or(Type::Int));
                    let text = self.expr_coerced(arg, &target);
                    let owned = self.take_owned(&text);
                    if owned || !self.is_managed(&target) {
                        arg_texts.push(text);
                    } else {
                        arg_texts.push(self.clone_text(&target, &text));
                    }
                }
                let type_args: Vec<Type> = info.type_args.iter().map(|t| self.subst(t)).collect();
                let c_name = specialized_c_name(&info.base_c_name, &type_args);
                let call = match info.receiver {
                    Some(receiver) => {
                        let base_expr = match &callee.kind {
                            ExprKind::Field { base, .. } => base.as_ref(),
                            _ => {
                                return "0".to_string();
                            }
                        };
                        let base_text = self.emit_expr(base_expr);
                        let receiver_text = if receiver == ReceiverKind::Mutable {
                            format!("&({})", base_text)
                        } else {
                            base_text
                        };
                        if arg_texts.is_empty() {
                            format!("{}({})", c_name, receiver_text)
                        } else {
                            format!("{}({}, {})", c_name, receiver_text, arg_texts.join(", "))
                        }
                    }
                    None => format!("{}({})", c_name, arg_texts.join(", ")),
                };
                let ret_ty = self.expr_type_by_id(id);
                if self.is_managed(&ret_ty) {
                    self.owned_temp(&ret_ty, &call)
                } else {
                    call
                }
            }
            None => "0".to_string(),
        }
    }

    fn emit_try(&mut self, _id: ExprId, operand: &Expr) -> String {
        let operand_ty = self.expr_type(operand);
        let ret_ty = self.current_ret.clone();
        let is_result = matches!(
            &operand_ty,
            Type::Enum(name, args) if name == "Result" && args.len() == 2
        );
        let is_option = matches!(
            &operand_ty,
            Type::Enum(name, args) if name == "Option" && args.len() == 1
        );
        if !is_result && !is_option {
            return "0".to_string();
        }
        let operand_text = self.emit_expr(operand);
        let owned = self.take_owned(&operand_text);
        let tmp = self.fresh_temp();
        self.prelude.push(format!(
            "{} {} = {};",
            c_type(&operand_ty),
            tmp,
            operand_text
        ));
        if owned && self.is_managed(&operand_ty) {
            self.stmt_temps.push((tmp.clone(), operand_ty.clone()));
        }
        let ret_tmp = self.fresh_temp();
        let mut branch: Vec<String> = Vec::new();
        branch.push(format!("{} {};", c_type(&ret_ty), ret_tmp));
        branch.push(format!("{}.tag = 1;", ret_tmp));
        if is_result {
            let payload_ty = match &operand_ty {
                Type::Enum(_, a) => a.get(1).cloned().unwrap_or(Type::Int),
                _ => Type::Int,
            };
            let payload = format!("{}.data.m_Err._0", tmp);
            let init = self.clone_text(&payload_ty, &payload);
            branch.push(format!("{}.data.m_Err._0 = {};", ret_tmp, init));
        }
        for l in self.temp_drop_lines(0) {
            branch.push(l);
        }
        for l in self.scope_drop_lines() {
            branch.push(l);
        }
        branch.push(format!("return {};", ret_tmp));
        self.prelude.push(format!("if ({}.tag == 1) {{", tmp));
        for l in branch {
            self.prelude.push(format!("    {}", l));
        }
        self.prelude.push("}".to_string());
        if is_result {
            format!("{}.data.m_Ok._0", tmp)
        } else {
            format!("{}.data.m_Some._0", tmp)
        }
    }

    fn emit_match_lines(
        &mut self,
        id: ExprId,
        scrutinee: &Expr,
        arms: &[MatchArm],
        want_value: bool,
        result_ty: &Type,
    ) -> (Vec<String>, Option<String>) {
        let start_len = self.prelude.len();
        let scrut_text = self.emit_expr(scrutinee);
        let scrut_owned = self.take_owned(&scrut_text);
        let scrut_prelude: Vec<String> = self.prelude.split_off(start_len);
        let scrut_ty = self.expr_type(scrutinee);
        let mut lines: Vec<String> = scrut_prelude;
        let scrut_tmp = self.fresh_temp();
        lines.push(format!(
            "{} {} = {};",
            c_type(&scrut_ty),
            scrut_tmp,
            scrut_text
        ));
        if scrut_owned && self.is_managed(&scrut_ty) {
            self.stmt_temps.push((scrut_tmp.clone(), scrut_ty.clone()));
        }
        let matched_tmp = self.fresh_temp();
        lines.push(format!("bool {} = false;", matched_tmp));
        let result_tmp = if want_value {
            let t = self.fresh_temp();
            if self.is_managed(result_ty) {
                lines.push(format!("{} {} = {{0}};", c_type(result_ty), t));
                self.stmt_temps.push((t.clone(), result_ty.clone()));
            } else {
                lines.push(format!("{} {};", c_type(result_ty), t));
            }
            Some(t)
        } else {
            None
        };
        for (index, arm) in arms.iter().enumerate() {
            let binding_infos: Vec<BindingInfo> = self
                .checked
                .match_arm_bindings
                .get(&(id, index))
                .cloned()
                .unwrap_or_default();
            let mut cursor = 0usize;
            let plan = self.plan_pattern(
                &arm.pattern,
                &scrut_tmp,
                &scrut_ty,
                &binding_infos,
                &mut cursor,
            );
            let cond = if plan.conds.is_empty() {
                "1".to_string()
            } else {
                plan.conds.join(" && ")
            };
            let arm_mark = self.stmt_temps.len();
            lines.push(format!("if (!{} && ({})) {{", matched_tmp, cond));
            for d in plan.decls {
                lines.push(format!("    {}", d));
            }
            for a in plan.applies {
                lines.push(format!("    {}", a));
            }
            let has_guard = arm.guard.is_some();
            if let Some(guard) = &arm.guard {
                let guard_start = self.prelude.len();
                let guard_text = self.emit_expr(guard);
                let guard_prelude: Vec<String> = self.prelude.split_off(guard_start);
                for l in guard_prelude {
                    lines.push(format!("    {}", l));
                }
                lines.push(format!("    if ({}) {{", guard_text));
            }
            let guard_mark = self.stmt_temps.len();
            let body_start = self.prelude.len();
            let body_text = self.emit_expr(&arm.body);
            let body_owned = self.take_owned(&body_text);
            let body_prelude: Vec<String> = self.prelude.split_off(body_start);
            let indent = if has_guard { "        " } else { "    " };
            for l in body_prelude {
                lines.push(format!("{}{}", indent, l));
            }
            if let Some(rt) = &result_tmp {
                let val = if body_owned || !self.is_managed(result_ty) {
                    body_text
                } else {
                    self.clone_text(result_ty, &body_text)
                };
                lines.push(format!("{}{} = {};", indent, rt, val));
            } else if matches!(arm.body.kind, ExprKind::Call { .. }) {
                lines.push(format!("{}{};", indent, body_text));
            } else {
                lines.push(format!("{}(void)({});", indent, body_text));
            }
            lines.push(format!("{}{} = true;", indent, matched_tmp));
            let body_temps = self.stmt_temps.split_off(guard_mark);
            for (name, ty) in body_temps.into_iter().rev() {
                if let Some(l) = self.drop_line(&ty, &name) {
                    lines.push(format!("{}{}", indent, l));
                }
            }
            if has_guard {
                lines.push("    }".to_string());
            }
            let guard_temps = self.stmt_temps.split_off(arm_mark);
            for (name, ty) in guard_temps.into_iter().rev() {
                if let Some(l) = self.drop_line(&ty, &name) {
                    lines.push(format!("    {}", l));
                }
            }
            lines.push("}".to_string());
        }
        lines.push(format!("if (!{}) {{", matched_tmp));
        lines.push("    bloom_panicf(NULL, 0, \"match: no arm matched\");".to_string());
        lines.push("}".to_string());
        (lines, result_tmp)
    }

    /// Builds the condition, binding declarations and binding assignments for
    /// one pattern. Or-patterns produce an OR condition plus a dispatch chain
    /// that extracts the bindings from whichever alternative matched.
    fn plan_pattern(
        &mut self,
        pat: &Pattern,
        access: &str,
        ty: &Type,
        bindings: &[BindingInfo],
        cursor: &mut usize,
    ) -> PatternPlan {
        match pat {
            Pattern::Wild(_) => PatternPlan::default(),
            Pattern::Literal(lit, _) => {
                let cond = match lit {
                    LiteralPattern::Int(v) => format!("({} == {}LL)", access, v),
                    LiteralPattern::Bool(v) => format!("({} == {})", access, v),
                    LiteralPattern::Char(v) => format!("({} == (uint32_t){}u)", access, *v as u32),
                    LiteralPattern::Str(v) => {
                        let lit = self.literal_name(v.as_bytes());
                        format!("(bloom_str_eq({}, {}))", access, lit)
                    }
                };
                PatternPlan {
                    conds: vec![cond],
                    ..PatternPlan::default()
                }
            }
            Pattern::Binding(_, _, _) => {
                let b = bindings.get(*cursor).cloned();
                *cursor += 1;
                match b {
                    Some(b) => {
                        let bty = self.subst(&b.ty);
                        PatternPlan {
                            conds: Vec::new(),
                            decls: vec![format!("{} {};", c_type(&bty), b.c_name)],
                            applies: vec![format!("{} = {};", b.c_name, access)],
                        }
                    }
                    None => PatternPlan::default(),
                }
            }
            Pattern::Or(alternatives, _) => {
                let mut plans = Vec::new();
                for alt in alternatives {
                    let mut c = 0usize;
                    plans.push(self.plan_pattern(alt, access, ty, bindings, &mut c));
                }
                *cursor = bindings.len();
                let alt_conds: Vec<String> = plans
                    .iter()
                    .map(|p| {
                        if p.conds.is_empty() {
                            "1".to_string()
                        } else {
                            format!("({})", p.conds.join(" && "))
                        }
                    })
                    .collect();
                let cond = format!("({})", alt_conds.join(" || "));
                let decls = plans.first().map(|p| p.decls.clone()).unwrap_or_default();
                let any_binds = plans.iter().any(|p| !p.applies.is_empty());
                let mut applies = Vec::new();
                if any_binds {
                    for (i, (c, p)) in alt_conds.iter().zip(plans.iter()).enumerate() {
                        let kw = if i == 0 { "if" } else { "} else if" };
                        applies.push(format!("{} ({}) {{", kw, c));
                        for a in &p.applies {
                            applies.push(format!("    {}", a));
                        }
                    }
                    applies.push("}".to_string());
                }
                PatternPlan {
                    conds: vec![cond],
                    decls,
                    applies,
                }
            }
            Pattern::Variant {
                path, args, named, ..
            } => {
                let variant_name = path.last().cloned().unwrap_or_default();
                let info = match ty {
                    Type::Enum(name, _) => self.checked.enums.get(name).cloned(),
                    _ => None,
                };
                let Some(info) = info else {
                    return PatternPlan::default();
                };
                let Some((tag, var)) = info.variant(&variant_name) else {
                    return PatternPlan::default();
                };
                let enum_args = match ty {
                    Type::Enum(_, a) => a.clone(),
                    _ => Vec::new(),
                };
                let type_bindings: HashMap<String, Type> =
                    info.params.iter().cloned().zip(enum_args).collect();
                let field_types: Vec<Type> = var
                    .fields
                    .iter()
                    .map(|f| substitute_type(f, &type_bindings))
                    .collect();
                let member = format!("m_{}", c_ident(&variant_name));
                let mut plan = PatternPlan {
                    conds: vec![format!("{}.tag == {}", access, tag)],
                    ..PatternPlan::default()
                };
                if let Some(names) = &var.field_names {
                    for nf in named {
                        if let Some(idx) = names.iter().position(|n| *n == nf.name) {
                            let ft = field_types.get(idx).cloned().unwrap_or(Type::Int);
                            let child =
                                format!("{}.data.{}.{}", access, member, field_c_name(&nf.name));
                            let sub = self.plan_pattern(&nf.pattern, &child, &ft, bindings, cursor);
                            plan.extend(sub);
                        }
                    }
                } else {
                    for (i, sub_pat) in args.iter().enumerate() {
                        let ft = field_types.get(i).cloned().unwrap_or(Type::Int);
                        let child = format!("{}.data.{}._{}", access, member, i);
                        let sub = self.plan_pattern(sub_pat, &child, &ft, bindings, cursor);
                        plan.extend(sub);
                    }
                }
                plan
            }
        }
    }

    /// Calls a standard library function. Arguments are borrowed (the C
    /// helpers do not take ownership), and managed results become owned
    /// temporaries.
    fn emit_stdlib_call(
        &mut self,
        c_name: &str,
        params: &[Type],
        args: &[Expr],
        id: ExprId,
    ) -> String {
        let mut arg_texts = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            let target = self.subst(&params.get(i).cloned().unwrap_or(Type::Int));
            arg_texts.push(self.expr_coerced(arg, &target));
        }
        let call = match c_name {
            "bloom_time_format_utc" => format!(
                "{}({}, {}, {})",
                c_name,
                arg_texts.join(", "),
                self.file_lit,
                args[0].span.start.line
            ),
            _ => format!("{}({})", c_name, arg_texts.join(", ")),
        };
        let ret_ty = self.expr_type_by_id(id);
        if self.is_managed(&ret_ty) {
            self.owned_temp(&ret_ty, &call)
        } else {
            call
        }
    }

    fn emit_math_call(&mut self, c_name: &str, args: &[Expr]) -> String {
        let f = |me: &mut Self, i: usize| me.expr_coerced(&args[i], &Type::Float);
        let n = |me: &mut Self, i: usize| me.expr_coerced(&args[i], &Type::Int);
        match c_name {
            "bloom_math_sqrt" => format!("sqrt({})", f(self, 0)),
            "bloom_math_pow" => format!("pow({}, {})", f(self, 0), f(self, 1)),
            "bloom_math_floor" => format!("floor({})", f(self, 0)),
            "bloom_math_ceil" => format!("ceil({})", f(self, 0)),
            "bloom_math_round" => format!("bloom_math_round({})", f(self, 0)),
            "bloom_math_abs" => format!("fabs({})", f(self, 0)),
            "bloom_math_min" => format!("fmin({}, {})", f(self, 0), f(self, 1)),
            "bloom_math_max" => format!("fmax({}, {})", f(self, 0), f(self, 1)),
            "bloom_math_clamp" => format!(
                "bloom_f64_clamp({}, {}, {}, {}, {})",
                f(self, 0),
                f(self, 1),
                f(self, 2),
                self.file_lit,
                args[0].span.start.line
            ),
            "bloom_math_abs_int" => format!("bloom_iabs({})", n(self, 0)),
            "bloom_math_min_int" => format!("bloom_i64_min({}, {})", n(self, 0), n(self, 1)),
            "bloom_math_max_int" => format!("bloom_i64_max({}, {})", n(self, 0), n(self, 1)),
            "bloom_math_clamp_int" => format!(
                "bloom_i64_clamp({}, {}, {}, {}, {})",
                n(self, 0),
                n(self, 1),
                n(self, 2),
                self.file_lit,
                args[0].span.start.line
            ),
            _ => "0".to_string(),
        }
    }

    fn emit_builtin(&mut self, kind: crate::check::BuiltinKind, args: &[Expr]) -> String {
        use crate::check::BuiltinKind::*;
        let type_suffix = |ty: &Type| -> &'static str {
            match ty {
                Type::Int => "i64",
                Type::Float => "f64",
                Type::Bool => "bool",
                Type::String => "str",
                Type::Char => "char",
                _ => "str",
            }
        };
        match kind {
            Print | Println => {
                if args.is_empty() {
                    return "bloom_println_empty()".to_string();
                }
                let text = self.emit_expr(&args[0]);
                let ty = self.expr_type(&args[0]);
                let name = if kind == Print {
                    "bloom_print"
                } else {
                    "bloom_println"
                };
                format!("{}_{}({})", name, type_suffix(&ty), text)
            }
            Input => {
                let text = self.emit_expr(&args[0]);
                self.owned_temp(&Type::String, &format!("bloom_input({})", text))
            }
            Len => {
                let text = self.emit_expr(&args[0]);
                let ty = self.expr_type(&args[0]);
                match ty {
                    Type::String => format!("bloom_str_len({})", text),
                    Type::Array(_) => format!("(int64_t)(({})->len)", text),
                    Type::Slice(elem) => format!(
                        "bloom_slice_{}_len({}, {}, {})",
                        arr_suffix(&elem),
                        text,
                        self.file_lit,
                        args[0].span.start.line
                    ),
                    _ => "0".to_string(),
                }
            }
            Str => {
                let text = self.emit_expr(&args[0]);
                let ty = self.expr_type(&args[0]);
                match ty {
                    Type::String => text,
                    Type::Int => {
                        self.owned_temp(&Type::String, &format!("bloom_str_from_i64({})", text))
                    }
                    Type::Float => {
                        self.owned_temp(&Type::String, &format!("bloom_str_from_f64({})", text))
                    }
                    Type::Bool => {
                        self.owned_temp(&Type::String, &format!("bloom_str_from_bool({})", text))
                    }
                    Type::Char => {
                        self.owned_temp(&Type::String, &format!("bloom_str_from_char({})", text))
                    }
                    _ => self.owned_temp(&Type::String, "bloom_str_from_cstr(\"<value>\")"),
                }
            }
            Int => {
                let text = self.emit_expr(&args[0]);
                let ty = self.expr_type(&args[0]);
                match ty {
                    Type::Int => text,
                    Type::Float => format!(
                        "bloom_f64_to_i64({}, {}, {})",
                        text, self.file_lit, args[0].span.start.line
                    ),
                    Type::Bool | Type::Char => format!("(int64_t)({})", text),
                    Type::String => format!(
                        "bloom_str_to_i64({}, {}, {})",
                        text, self.file_lit, args[0].span.start.line
                    ),
                    Type::Ffi(_) => format!("(int64_t)({})", text),
                    _ => "0".to_string(),
                }
            }
            Float => {
                let text = self.emit_expr(&args[0]);
                let ty = self.expr_type(&args[0]);
                match ty {
                    Type::Int => format!("((double)({}))", text),
                    Type::Float => text,
                    Type::String => format!(
                        "bloom_str_to_f64({}, {}, {})",
                        text, self.file_lit, args[0].span.start.line
                    ),
                    Type::Ffi(_) => format!("((double)({}))", text),
                    _ => "0.0".to_string(),
                }
            }
            Char => {
                let text = self.emit_expr(&args[0]);
                format!(
                    "bloom_char_from_i64({}, {}, {})",
                    text, self.file_lit, args[0].span.start.line
                )
            }
            Exit => {
                let text = self.emit_expr(&args[0]);
                format!("bloom_exit({})", text)
            }
            RawPtrNull => "NULL".to_string(),
            CIntConv | CUIntConv | CLongConv | CULongConv | CLongLongConv | CULongLongConv
            | F32Conv => {
                let text = self.emit_expr(&args[0]);
                let c_name = match kind {
                    CIntConv => "int",
                    CUIntConv => "unsigned int",
                    CLongConv => "long",
                    CULongConv => "unsigned long",
                    CLongLongConv => "long long",
                    CULongLongConv => "unsigned long long",
                    _ => "float",
                };
                format!("(({})({}))", c_name, text)
            }
            Panic => {
                let text = self.emit_expr(&args[0]);
                format!(
                    "bloom_panic_msg({}, {}, {})",
                    self.file_lit, args[0].span.start.line, text
                )
            }
            Push => {
                let base = self.emit_expr(&args[0]);
                let base_ty = self.expr_type(&args[0]);
                let elem = base_ty.element().cloned().unwrap_or(Type::Int);
                let val = self.expr_coerced(&args[1], &elem);
                format!("bloom_arr_{}_push({}, {})", arr_suffix(&elem), base, val)
            }
            Pop => {
                let base = self.emit_expr(&args[0]);
                let base_ty = self.expr_type(&args[0]);
                let elem = base_ty.element().cloned().unwrap_or(Type::Int);
                let opt_ty = Type::Enum("Option".to_string(), vec![elem]);
                let tmp = self.fresh_temp();
                self.prelude.push(format!("{} {};", c_type(&opt_ty), tmp));
                self.prelude.push(format!(
                    "if (({base})->len == 0) {{ {tmp}.tag = 1; }} else {{ ({base})->len--; ({base})->gen++; {tmp}.tag = 0; {tmp}.data.m_Some._0 = ({base})->data[({base})->len]; }}",
                    base = base,
                    tmp = tmp
                ));
                if self.is_managed(&opt_ty) {
                    self.stmt_temps.push((tmp.clone(), opt_ty));
                }
                tmp
            }
            Insert => {
                let base = self.emit_expr(&args[0]);
                let base_ty = self.expr_type(&args[0]);
                let elem = base_ty.element().cloned().unwrap_or(Type::Int);
                let idx = self.expr_coerced(&args[1], &Type::Int);
                let val = self.expr_coerced(&args[2], &elem);
                format!(
                    "bloom_arr_{}_insert({}, {}, {}, {}, {})",
                    arr_suffix(&elem),
                    base,
                    idx,
                    val,
                    self.file_lit,
                    args[0].span.start.line
                )
            }
            Remove => {
                let base = self.emit_expr(&args[0]);
                let base_ty = self.expr_type(&args[0]);
                let elem = base_ty.element().cloned().unwrap_or(Type::Int);
                let idx_text = self.emit_expr(&args[1]);
                let opt_ty = Type::Enum("Option".to_string(), vec![elem.clone()]);
                let idx_tmp = self.fresh_temp();
                self.prelude
                    .push(format!("int64_t {} = {};", idx_tmp, idx_text));
                let tmp = self.fresh_temp();
                self.prelude.push(format!("{} {};", c_type(&opt_ty), tmp));
                self.prelude.push(format!(
                    "if ({i} < 0 || (uint64_t){i} >= (uint64_t)({b})->len) {{ {t}.tag = 1; }} else {{ {t}.tag = 0; {t}.data.m_Some._0 = bloom_arr_{s}_remove({b}, {i}, {f}, {l}); }}",
                    i = idx_tmp,
                    b = base,
                    t = tmp,
                    s = arr_suffix(&elem),
                    f = self.file_lit,
                    l = args[0].span.start.line
                ));
                if self.is_managed(&opt_ty) {
                    self.stmt_temps.push((tmp.clone(), opt_ty));
                }
                tmp
            }
            Slice => {
                let base = self.emit_expr(&args[0]);
                let base_ty = self.expr_type(&args[0]);
                let elem = base_ty.element().cloned().unwrap_or(Type::Int);
                let start = self.expr_coerced(&args[1], &Type::Int);
                let end = self.expr_coerced(&args[2], &Type::Int);
                let file_lit = self.file_lit.clone();
                let line = args[0].span.start.line;
                let call = format!(
                    "bloom_arr_{}_slice({}, {}, {}, {}, {})",
                    arr_suffix(&elem),
                    base,
                    start,
                    end,
                    file_lit,
                    line
                );
                self.owned_temp(&base_ty, &call)
            }
            SliceView => {
                let base = self.emit_expr(&args[0]);
                let base_ty = self.expr_type(&args[0]);
                let elem = base_ty.element().cloned().unwrap_or(Type::Int);
                let start = self.expr_coerced(&args[1], &Type::Int);
                let end = self.expr_coerced(&args[2], &Type::Int);
                let slice_ty = Type::Slice(Box::new(elem.clone()));
                let file_lit = self.file_lit.clone();
                let line = args[0].span.start.line;
                let call = format!(
                    "bloom_slice_{}_new({}, {}, {}, {}, {})",
                    arr_suffix(&elem),
                    base,
                    start,
                    end,
                    file_lit,
                    line
                );
                self.owned_temp(&slice_ty, &call)
            }
            Contains => {
                let a = self.emit_expr(&args[0]);
                let b = self.emit_expr(&args[1]);
                format!("bloom_str_contains({}, {})", a, b)
            }
            StartsWith => {
                let a = self.emit_expr(&args[0]);
                let b = self.emit_expr(&args[1]);
                format!("bloom_str_starts_with({}, {})", a, b)
            }
            EndsWith => {
                let a = self.emit_expr(&args[0]);
                let b = self.emit_expr(&args[1]);
                format!("bloom_str_ends_with({}, {})", a, b)
            }
            Trim => {
                let a = self.emit_expr(&args[0]);
                self.owned_temp(&Type::String, &format!("bloom_str_trim({})", a))
            }
            Replace => {
                let a = self.emit_expr(&args[0]);
                let b = self.emit_expr(&args[1]);
                let c = self.emit_expr(&args[2]);
                self.owned_temp(
                    &Type::String,
                    &format!("bloom_str_replace({}, {}, {})", a, b, c),
                )
            }
            ToUpper => {
                let a = self.emit_expr(&args[0]);
                self.owned_temp(&Type::String, &format!("bloom_str_to_upper({})", a))
            }
            ToLower => {
                let a = self.emit_expr(&args[0]);
                self.owned_temp(&Type::String, &format!("bloom_str_to_lower({})", a))
            }
            Split => {
                let a = self.emit_expr(&args[0]);
                let b = self.emit_expr(&args[1]);
                self.owned_temp(
                    &Type::Array(Box::new(Type::String)),
                    &format!(
                        "bloom_str_split({}, {}, {}, {})",
                        a, b, self.file_lit, args[0].span.start.line
                    ),
                )
            }
            Join => {
                let a = self.emit_expr(&args[0]);
                let b = self.emit_expr(&args[1]);
                self.owned_temp(&Type::String, &format!("bloom_str_join({}, {})", a, b))
            }
        }
    }
}

fn format_float_literal(v: f64) -> String {
    let s = format!("{:?}", v);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

fn const_c_value(value: &ConstValue, ty: &Type) -> String {
    match value {
        ConstValue::Int(v) => {
            if *ty == Type::Float {
                format_float_literal(*v as f64)
            } else {
                format!("{}LL", v)
            }
        }
        ConstValue::Float(v) => format_float_literal(*v),
        ConstValue::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        ConstValue::Str(s) => format!(
            "bloom_str_from_bytes({}, {})",
            c_string_literal(s.as_bytes()),
            s.len()
        ),
        ConstValue::Char(c) => format!("(uint32_t){}u", *c as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn generate(src: &str) -> String {
        let tokens = lex(src).tokens;
        let out = parse(tokens);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        let checked = check(out.program).expect("should typecheck");
        emit(&checked)
    }

    #[test]
    fn generates_hello_world() {
        let c = generate("fn main() -> int { println(\"Hello from Bloom!\"); return 0; }");
        assert!(c.contains("int main(void)"));
        assert!(c.contains("bloom_init();"));
        assert!(c.contains("bloom_println_str("));
        assert!(c.contains("bf_main()"));
    }

    #[test]
    fn generates_arithmetic_helpers() {
        let c = generate("fn main() -> int { let x = 1 + 2 * 3; return x; }");
        assert!(c.contains("bloom_iadd"));
        assert!(c.contains("bloom_imul"));
    }

    #[test]
    fn generates_bounds_checked_index() {
        let c = generate("fn main() -> int { let xs = [1, 2, 3]; return xs[0]; }");
        assert!(c.contains("bloom_arr_i64_new()"));
        assert!(c.contains("bloom_arr_i64_push("));
        assert!(c.contains("bloom_arr_i64_get("));
    }

    #[test]
    fn generates_string_concat() {
        let c = generate("fn main() -> int { let s = \"a\" + \"b\"; println(s); return 0; }");
        assert!(c.contains("bloom_str_concat("));
    }

    #[test]
    fn generates_float_promotion() {
        let c = generate("fn main() -> int { let x = 1 + 2.5; println(x); return 0; }");
        assert!(c.contains("((double)(1LL))"));
    }

    #[test]
    fn generates_global_const() {
        let c = generate("const MAX: int = 10;\nfn main() -> int { return MAX; }");
        assert!(c.contains("int64_t v_MAX_0;"));
        assert!(c.contains("v_MAX_0 = 10LL;"));
    }

    #[test]
    fn generates_input_and_conversion() {
        let c = generate(
            "fn main() -> int { let name = input(\"Name: \"); let n = int(input(\"N: \")); println(name); return n; }",
        );
        assert!(c.contains("bloom_input("));
        assert!(c.contains("bloom_str_to_i64("));
    }

    #[test]
    fn generates_for_loop() {
        let c = generate("fn main() -> int { for i in 0..10 { println(i); } return 0; }");
        assert!(c.contains("for (int64_t"));
        assert!(c.contains("< t_0;"));
    }

    #[test]
    fn escapes_string_literals() {
        let c = generate("fn main() -> int { println(\"a\\nb\"); return 0; }");
        assert!(c.contains("\\012"));
    }

    #[test]
    fn emits_struct_typedef() {
        let c = generate(
            "struct Point {\n    x: int,\n    y: float\n}\nfn main() -> int { let p = Point { x: 1, y: 2 }; return p.x; }",
        );
        assert!(c.contains("typedef struct {"));
        assert!(c.contains("int64_t f_x;"));
        assert!(c.contains("double f_y;"));
        assert!(c.contains("} bloom_struct_Point;"));
    }

    #[test]
    fn emits_struct_literal_prelude() {
        let c =
            generate("struct P { x: int }\nfn main() -> int { let p = P { x: 7 }; return p.x; }");
        assert!(c.contains("bloom_struct_P t_0;"));
        assert!(c.contains("t_0.f_x = 7LL;"));
    }

    #[test]
    fn emits_field_access_and_assignment() {
        let c = generate(
            "struct P { x: int }\nfn main() -> int { var p = P { x: 1 }; p.x = 2; return p.x; }",
        );
        assert!(c.contains(".f_x = 2LL)"));
        assert!(c.contains(").f_x"));
    }

    #[test]
    fn struct_typedefs_are_topologically_sorted() {
        let c = generate(
            "struct Outer { inner: Inner }\nstruct Inner { v: int }\nfn main() -> int { let o = Outer { inner: Inner { v: 1 } }; return o.inner.v; }",
        );
        let inner_pos = c.find("} bloom_struct_Inner;").expect("Inner typedef");
        let outer_pos = c.find("} bloom_struct_Outer;").expect("Outer typedef");
        assert!(inner_pos < outer_pos);
    }

    #[test]
    fn struct_params_in_c_signature() {
        let c = generate(
            "struct P { x: int }\nfn get(p: P) -> int { return p.x; }\nfn main() -> int { return get(P { x: 1 }); }",
        );
        assert!(c.contains("int64_t bf_get(bloom_struct_P "));
    }

    #[test]
    fn struct_return_type_in_c_signature() {
        let c = generate(
            "struct P { x: int }\nfn make() -> P { return P { x: 1 }; }\nfn main() -> int { return make().x; }",
        );
        assert!(c.contains("bloom_struct_P bf_make(void)"));
    }

    #[test]
    fn emits_forward_prototypes() {
        let c = generate("fn main() -> int { return helper(); }\nfn helper() -> int { return 1; }");
        let proto = c.find("int64_t bf_helper(void);").expect("prototype");
        let def = c.find("int64_t bf_helper(void) {").expect("definition");
        assert!(proto < def, "prototype must come before definition");
    }

    #[test]
    fn emits_method_prototype_and_definition() {
        let c = generate(
            "struct P { x: int }\nimpl P { fn get(self) -> int { return self.x; } }\nfn main() -> int { let p = P { x: 1 }; return p.get(); }",
        );
        assert!(c.contains("int64_t bf_P_get(bloom_struct_P"));
        assert!(c.contains("bf_P_get("));
    }

    #[test]
    fn mutable_receiver_uses_pointer() {
        let c = generate(
            "struct P { x: int }\nimpl P { fn set(var self, v: int) -> void { self.x = v; } }\nfn main() -> int { var p = P { x: 1 }; p.set(2); return 0; }",
        );
        assert!(c.contains("void bf_P_set(bloom_struct_P *"));
        assert!(c.contains("bf_P_set(&(v_p_"));
    }

    #[test]
    fn value_receiver_passes_by_value() {
        let c = generate(
            "struct P { x: int }\nimpl P { fn get(self) -> int { return self.x; } }\nfn main() -> int { var p = P { x: 1 }; return p.get(); }",
        );
        assert!(c.contains("bf_P_get(v_p_"));
        assert!(!c.contains("bf_P_get(&"));
    }

    #[test]
    fn method_on_self_field_uses_arrow() {
        let c = generate(
            "struct I { v: int }\nstruct O { inner: I }\nimpl I { fn set(var self, x: int) -> void { self.v = x; } }\nimpl O { fn touch(var self) -> void { self.inner.set(1); } }\nfn main() -> int { var o = O { inner: I { v: 0 } }; o.touch(); return 0; }",
        );
        assert!(c.contains("->"));
    }

    #[test]
    fn methods_do_not_collide_across_structs() {
        let c = generate(
            "struct A { x: int }\nstruct B { x: int }\nimpl A { fn get(self) -> int { return self.x; } }\nimpl B { fn get(self) -> int { return self.x + 1; } }\nfn main() -> int { let a = A { x: 1 }; let b = B { x: 2 }; return a.get() + b.get(); }",
        );
        assert!(c.contains("bf_A_get"));
        assert!(c.contains("bf_B_get"));
    }

    #[test]
    fn emits_enum_typedef_with_tag_and_union() {
        let c = generate(
            "enum Message {\n    Quit,\n    Move { x: int, y: int },\n    Write(string)\n}\nfn main() -> int {\n    let m = Message::Write(\"hi\");\n    return match m {\n        Message::Quit => 0,\n        Message::Move { x, y } => x + y,\n        Message::Write(t) => len(t)\n    };\n}",
        );
        assert!(c.contains("int32_t tag;"));
        assert!(c.contains("union {"));
        assert!(c.contains("m_Quit"));
        assert!(c.contains("m_Move"));
        assert!(c.contains("m_Write"));
        assert!(c.contains("bloom_enum_Message"));
    }

    #[test]
    fn emits_variant_construction_and_match_tags() {
        let c = generate(
            "enum Color { Red, Green }\nfn main() -> int {\n    let c = Color::Green;\n    return match c {\n        Color::Red => 0,\n        Color::Green => 1\n    };\n}",
        );
        assert!(c.contains(".tag = 1;"));
        assert!(c.contains(".tag == 0"));
    }

    #[test]
    fn emits_generic_enum_instantiations_with_distinct_names() {
        let c = generate(
            "fn main() -> int {\n    let a: Option<int> = Option::Some(1);\n    let b: Option<string> = Option::Some(\"x\");\n    let n = match a {\n        Option::Some(v) => v,\n        Option::None => 0\n    };\n    let s = match b {\n        Option::Some(v) => len(v),\n        Option::None => 0\n    };\n    return n + s;\n}",
        );
        assert!(c.contains("bloom_enum_Option_i64"));
        assert!(c.contains("bloom_enum_Option_str"));
    }

    #[test]
    fn emits_nested_enum_instantiation_name() {
        let c = generate(
            "fn main() -> int {\n    let o: Option<Option<int>> = Option::Some(Option::Some(1));\n    return match o {\n        Option::Some(inner) => match inner {\n            Option::Some(v) => v,\n            Option::None => 0\n        },\n        Option::None => -1\n    };\n}",
        );
        assert!(c.contains("bloom_enum_Option_Option_i64"));
    }

    #[test]
    fn emits_result_instantiation() {
        let c = generate(
            "fn f() -> Result<int, string> {\n    return Result::Ok(1);\n}\nfn main() -> int {\n    let r = f();\n    return match r {\n        Result::Ok(v) => v,\n        Result::Err(e) => len(e)\n    };\n}",
        );
        assert!(c.contains("bloom_enum_Result_i64_str"));
    }

    #[test]
    fn emits_pattern_bindings_for_named_variants() {
        let c = generate(
            "enum E { V { x: int, y: int } }\nfn main() -> int {\n    let e = E::V { x: 1, y: 2 };\n    return match e {\n        E::V { x, y } => x + y\n    };\n}",
        );
        assert!(c.contains("m_V"));
        assert!(c.contains(".f_x"));
    }

    #[test]
    fn enum_typedefs_come_before_struct_typedefs_that_use_them() {
        let c = generate(
            "struct S { e: E }\nenum E { A, B }\nfn main() -> int {\n    let s = S { e: E::A };\n    return 0;\n}",
        );
        let enum_pos = c.find("} bloom_enum_E;").expect("enum typedef");
        let struct_pos = c.find("} bloom_struct_S;").expect("struct typedef");
        assert!(enum_pos < struct_pos);
    }

    #[test]
    fn emits_generic_struct_instantiations() {
        let c = generate(
            "struct Box<T> { value: T }\nfn main() -> int {\n    let a = Box { value: 1 };\n    let b: Box<string> = Box { value: \"x\" };\n    return a.value;\n}",
        );
        assert!(c.contains("} bloom_struct_Box_i64;"));
        assert!(c.contains("} bloom_struct_Box_str;"));
        assert!(c.contains("int64_t f_value;"));
    }

    #[test]
    fn emits_specialized_generic_functions() {
        let c = generate(
            "fn identity<T>(value: T) -> T {\n    return value;\n}\nfn main() -> int {\n    println(identity(1));\n    println(identity(\"x\"));\n    return 0;\n}",
        );
        assert!(c.contains("bf_identity_i64(int64_t"));
        assert!(c.contains("bf_identity_str(bloom_str"));
        assert!(c.contains("bf_identity_i64(1LL)"));
    }

    #[test]
    fn emits_specialized_generic_methods() {
        let c = generate(
            "struct Box<T> { value: T }\nimpl<T> Box<T> {\n    fn get(self) -> T {\n        return self.value;\n    }\n}\nfn main() -> int {\n    let b = Box { value: 5 };\n    return b.get();\n}",
        );
        assert!(c.contains("bf_Box_get_i64(bloom_struct_Box_i64"));
        assert!(c.contains("bf_Box_get_i64(v_"));
    }

    #[test]
    fn emits_transitive_method_instantiations() {
        let c = generate(
            "struct Box<T> { value: T }\nimpl<T> Box<T> {\n    fn get(self) -> T {\n        return self.value;\n    }\n    fn twice(self) -> T {\n        return self.get();\n    }\n}\nfn main() -> int {\n    let b = Box { value: 3 };\n    return b.twice();\n}",
        );
        assert!(c.contains("bf_Box_twice_i64"));
        assert!(c.contains("bf_Box_get_i64"));
    }

    #[test]
    fn emits_try_operator_code() {
        let c = generate(
            "fn f(r: Result<int, string>) -> Result<int, string> {\n    let v = r?;\n    return Result::Ok(v);\n}\nfn main() -> int { return 0; }",
        );
        assert!(c.contains("if (") && c.contains(".tag == 1) {"));
        assert!(c.contains("return "));
        assert!(c.contains(".data.m_Ok._0"));
    }

    #[test]
    fn emits_associated_function() {
        let c = generate(
            "enum Shape { Circle(float) }\nimpl Shape {\n    fn circle(radius: float) -> Shape {\n        return Shape::Circle(radius);\n    }\n}\nfn main() -> int {\n    let s = Shape::circle(1.0);\n    return 0;\n}",
        );
        assert!(c.contains("bf_Shape_circle(double"));
        assert!(c.contains("bf_Shape_circle(1.0)"));
    }

    #[test]
    fn generic_enum_method_instantiations() {
        let c = generate(
            "impl<T> Option<T> {\n    fn is_some(self) -> bool {\n        return match self {\n            Option::Some(_) => true,\n            Option::None => false\n        };\n    }\n}\nfn main() -> int {\n    let a: Option<int> = Option::Some(1);\n    if a.is_some() {\n        return 1;\n    }\n    return 0;\n}",
        );
        assert!(c.contains("bf_Option_is_some_i64"));
    }
}
