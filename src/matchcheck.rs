use std::collections::HashMap;

use crate::ast::Pattern;
use crate::check::EnumInfo;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq)]
pub enum LitValue {
    Int(i64),
    Bool(bool),
    Char(char),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NormPat {
    Wild,
    Ctor(usize, Vec<NormPat>),
    Lit(LitValue),
}

pub struct MatchCtx<'a> {
    pub enums: &'a HashMap<String, EnumInfo>,
}

impl<'a> MatchCtx<'a> {
    pub fn enum_info(&self, ty: &Type) -> Option<&EnumInfo> {
        match ty {
            Type::Enum(name, _) => self.enums.get(name),
            _ => None,
        }
    }

    pub fn variant_field_types(&self, ty: &Type, variant: usize) -> Vec<Type> {
        let Some(info) = self.enum_info(ty) else {
            return Vec::new();
        };
        let args = match ty {
            Type::Enum(_, args) => args.clone(),
            _ => Vec::new(),
        };
        let bindings: HashMap<String, Type> = info.params.iter().cloned().zip(args).collect();
        let Some(var) = info.variants.get(variant) else {
            return Vec::new();
        };
        var.fields
            .iter()
            .map(|f| crate::check::substitute_type(f, &bindings))
            .collect()
    }
}

/// Expands a pattern into normalized rows: an or-pattern produces one row
/// per alternative (the standard matrix expansion).
pub fn normalize_pattern_rows(pat: &Pattern, ty: &Type, ctx: &MatchCtx) -> Vec<NormPat> {
    match pat {
        Pattern::Or(alternatives, _) => {
            let mut rows = Vec::new();
            for alt in alternatives {
                rows.extend(normalize_pattern_rows(alt, ty, ctx));
            }
            rows
        }
        Pattern::Variant {
            path, args, named, ..
        } => {
            let variant_name = path.last().cloned().unwrap_or_default();
            let Some(info) = ctx.enum_info(ty) else {
                return vec![NormPat::Wild];
            };
            let Some(vi) = info.variants.iter().position(|v| v.name == variant_name) else {
                return vec![NormPat::Wild];
            };
            let field_types = ctx.variant_field_types(ty, vi);
            let var = &info.variants[vi];
            let mut sub_options: Vec<Vec<NormPat>> = Vec::new();
            if let Some(field_names) = &var.field_names {
                for (i, field_name) in field_names.iter().enumerate() {
                    let field_ty = field_types.get(i).cloned().unwrap_or(Type::Int);
                    let entry = named.iter().find(|n| n.name == *field_name);
                    let p = match entry {
                        Some(entry) => normalize_pattern_rows(&entry.pattern, &field_ty, ctx),
                        None => vec![NormPat::Wild],
                    };
                    sub_options.push(p);
                }
            } else {
                for (i, arg) in args.iter().enumerate() {
                    let field_ty = field_types.get(i).cloned().unwrap_or(Type::Int);
                    sub_options.push(normalize_pattern_rows(arg, &field_ty, ctx));
                }
            }
            let mut combos: Vec<Vec<NormPat>> = vec![Vec::new()];
            for options in sub_options {
                let mut next = Vec::new();
                for combo in &combos {
                    for opt in &options {
                        let mut c = combo.clone();
                        c.push(opt.clone());
                        next.push(c);
                    }
                }
                combos = next;
            }
            combos
                .into_iter()
                .map(|sub| NormPat::Ctor(vi, sub))
                .collect()
        }
        other => vec![normalize_pattern(other, ty, ctx)],
    }
}

pub fn normalize_pattern(pat: &Pattern, ty: &Type, ctx: &MatchCtx) -> NormPat {
    match pat {
        Pattern::Wild(_) | Pattern::Binding(_, _, _) => NormPat::Wild,
        Pattern::Or(alternatives, _) => alternatives
            .first()
            .map(|p| normalize_pattern(p, ty, ctx))
            .unwrap_or(NormPat::Wild),
        Pattern::Literal(lit, _) => match lit {
            crate::ast::LiteralPattern::Int(v) => NormPat::Lit(LitValue::Int(*v)),
            crate::ast::LiteralPattern::Bool(v) => NormPat::Lit(LitValue::Bool(*v)),
            crate::ast::LiteralPattern::Char(v) => NormPat::Lit(LitValue::Char(*v)),
            crate::ast::LiteralPattern::Str(v) => NormPat::Lit(LitValue::Str(v.clone())),
        },
        Pattern::Variant {
            path, args, named, ..
        } => {
            let variant_name = path.last().cloned().unwrap_or_default();
            let Some(info) = ctx.enum_info(ty) else {
                return NormPat::Wild;
            };
            let Some(vi) = info.variants.iter().position(|v| v.name == variant_name) else {
                return NormPat::Wild;
            };
            let field_types = ctx.variant_field_types(ty, vi);
            let var = &info.variants[vi];
            let mut sub = Vec::new();
            if let Some(field_names) = &var.field_names {
                for (i, field_name) in field_names.iter().enumerate() {
                    let field_ty = field_types.get(i).cloned().unwrap_or(Type::Int);
                    let entry = named.iter().find(|n| n.name == *field_name);
                    let p = match entry {
                        Some(entry) => normalize_pattern(&entry.pattern, &field_ty, ctx),
                        None => NormPat::Wild,
                    };
                    sub.push(p);
                }
            } else {
                for (i, arg) in args.iter().enumerate() {
                    let field_ty = field_types.get(i).cloned().unwrap_or(Type::Int);
                    sub.push(normalize_pattern(arg, &field_ty, ctx));
                }
            }
            NormPat::Ctor(vi, sub)
        }
    }
}

fn specialize(matrix: &[Vec<NormPat>], variant: usize, arity: usize) -> Vec<Vec<NormPat>> {
    let mut result = Vec::new();
    for row in matrix {
        match row.first() {
            Some(NormPat::Ctor(tag, args)) => {
                if *tag == variant {
                    let mut new_row = args.clone();
                    new_row.resize(arity, NormPat::Wild);
                    new_row.extend_from_slice(&row[1..]);
                    result.push(new_row);
                }
            }
            Some(NormPat::Wild) => {
                let mut new_row = vec![NormPat::Wild; arity];
                new_row.extend_from_slice(&row[1..]);
                result.push(new_row);
            }
            _ => {}
        }
    }
    result
}

fn specialize_lit(matrix: &[Vec<NormPat>], lit: &LitValue) -> Vec<Vec<NormPat>> {
    let mut result = Vec::new();
    for row in matrix {
        match row.first() {
            Some(NormPat::Lit(v)) if v == lit => {
                result.push(row[1..].to_vec());
            }
            Some(NormPat::Wild) => {
                result.push(row[1..].to_vec());
            }
            _ => {}
        }
    }
    result
}

fn default_matrix(matrix: &[Vec<NormPat>]) -> Vec<Vec<NormPat>> {
    matrix
        .iter()
        .filter(|row| matches!(row.first(), Some(NormPat::Wild)))
        .map(|row| row[1..].to_vec())
        .collect()
}

pub fn is_exhaustive(
    matrix: &[Vec<NormPat>],
    types: &[Type],
    ctx: &MatchCtx,
) -> Option<Vec<NormPat>> {
    if types.is_empty() {
        if matrix.iter().any(|r| r.is_empty()) {
            return None;
        }
        return Some(Vec::new());
    }
    let t0 = &types[0];
    if let Some(info) = ctx.enum_info(t0) {
        for vi in 0..info.variants.len() {
            let fields = ctx.variant_field_types(t0, vi);
            let spec = specialize(matrix, vi, fields.len());
            let mut new_types = fields.clone();
            new_types.extend_from_slice(&types[1..]);
            if let Some(sub) = is_exhaustive(&spec, &new_types, ctx) {
                let mut witness = vec![NormPat::Ctor(
                    vi,
                    sub[..fields.len().min(sub.len())].to_vec(),
                )];
                witness.extend_from_slice(&sub[fields.len().min(sub.len())..]);
                return Some(witness);
            }
        }
        None
    } else if *t0 == Type::Bool {
        for value in [true, false] {
            let spec = specialize_lit(matrix, &LitValue::Bool(value));
            if let Some(sub) = is_exhaustive(&spec, &types[1..], ctx) {
                let mut witness = vec![NormPat::Lit(LitValue::Bool(value))];
                witness.extend_from_slice(&sub);
                return Some(witness);
            }
        }
        None
    } else {
        let default = default_matrix(matrix);
        if let Some(sub) = is_exhaustive(&default, &types[1..], ctx) {
            let mut witness = vec![NormPat::Wild];
            witness.extend_from_slice(&sub);
            Some(witness)
        } else {
            None
        }
    }
}

pub fn is_useful(matrix: &[Vec<NormPat>], q: &[NormPat], types: &[Type], ctx: &MatchCtx) -> bool {
    if q.is_empty() {
        return matrix.is_empty();
    }
    let q0 = &q[0];
    match q0 {
        NormPat::Ctor(tag, args) => {
            let Some(t0) = types.first() else {
                return false;
            };
            let fields = ctx.variant_field_types(t0, *tag);
            let spec = specialize(matrix, *tag, fields.len());
            let mut new_q = args.clone();
            new_q.resize(fields.len(), NormPat::Wild);
            new_q.extend_from_slice(&q[1..]);
            let mut new_types = fields;
            new_types.extend_from_slice(&types[1..]);
            is_useful(&spec, &new_q, &new_types, ctx)
        }
        NormPat::Lit(lit) => {
            let Some(t0) = types.first() else {
                return false;
            };
            if *t0 == Type::Bool {
                let spec = specialize_lit(matrix, lit);
                let new_q = q[1..].to_vec();
                return is_useful(&spec, &new_q, &types[1..], ctx);
            }
            let spec = specialize_lit(matrix, lit);
            is_useful(&spec, &q[1..], &types[1..], ctx)
        }
        NormPat::Wild => {
            let Some(t0) = types.first() else {
                return false;
            };
            if let Some(info) = ctx.enum_info(t0) {
                for vi in 0..info.variants.len() {
                    let fields = ctx.variant_field_types(t0, vi);
                    let spec = specialize(matrix, vi, fields.len());
                    let mut new_q = vec![NormPat::Wild; fields.len()];
                    new_q.extend_from_slice(&q[1..]);
                    let mut new_types = fields;
                    new_types.extend_from_slice(&types[1..]);
                    if is_useful(&spec, &new_q, &new_types, ctx) {
                        return true;
                    }
                }
                false
            } else if *t0 == Type::Bool {
                for value in [true, false] {
                    let spec = specialize_lit(matrix, &LitValue::Bool(value));
                    if is_useful(&spec, &q[1..], &types[1..], ctx) {
                        return true;
                    }
                }
                false
            } else {
                let default = default_matrix(matrix);
                is_useful(&default, &q[1..], &types[1..], ctx)
            }
        }
    }
}

pub fn witness_to_string(witness: &[NormPat], types: &[Type], ctx: &MatchCtx) -> String {
    if witness.is_empty() {
        return "_".to_string();
    }
    format_pattern(&witness[0], &types[0], ctx)
}

fn format_pattern(p: &NormPat, ty: &Type, ctx: &MatchCtx) -> String {
    match p {
        NormPat::Wild => "_".to_string(),
        NormPat::Ctor(tag, args) => {
            let Some(info) = ctx.enum_info(ty) else {
                return "_".to_string();
            };
            let Some(var) = info.variants.get(*tag) else {
                return "_".to_string();
            };
            let ename = match ty {
                Type::Enum(name, _) => name.clone(),
                _ => "?".to_string(),
            };
            if args.is_empty() {
                return format!("{}::{}", ename, var.name);
            }
            let fields = ctx.variant_field_types(ty, *tag);
            let inner: Vec<String> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let ft = fields.get(i).cloned().unwrap_or(Type::Int);
                    format_pattern(a, &ft, ctx)
                })
                .collect();
            format!("{}::{}({})", ename, var.name, inner.join(", "))
        }
        NormPat::Lit(lit) => match lit {
            LitValue::Int(v) => v.to_string(),
            LitValue::Bool(v) => v.to_string(),
            LitValue::Char(v) => format!("'{}'", v),
            LitValue::Str(v) => format!("\"{}\"", v),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::EnumVariantInfo;

    fn option_ctx() -> HashMap<String, EnumInfo> {
        let mut enums = HashMap::new();
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
                        fields: vec![],
                        field_names: None,
                    },
                ],
                span: crate::token::Span::point(1, 1),
                is_pub: true,
                module: String::new(),
            },
        );
        enums
    }

    fn opt_int() -> Type {
        Type::Enum("Option".to_string(), vec![Type::Int])
    }

    #[test]
    fn single_variant_not_exhaustive() {
        let enums = option_ctx();
        let ctx = MatchCtx { enums: &enums };
        let matrix = vec![vec![NormPat::Ctor(0, vec![NormPat::Wild])]];
        let w = is_exhaustive(&matrix, &[opt_int()], &ctx).expect("witness");
        assert_eq!(witness_to_string(&w, &[opt_int()], &ctx), "Option::None");
    }

    #[test]
    fn full_coverage_is_exhaustive() {
        let enums = option_ctx();
        let ctx = MatchCtx { enums: &enums };
        let matrix = vec![
            vec![NormPat::Ctor(0, vec![NormPat::Wild])],
            vec![NormPat::Ctor(1, vec![])],
        ];
        assert!(is_exhaustive(&matrix, &[opt_int()], &ctx).is_none());
    }

    #[test]
    fn wildcard_is_exhaustive() {
        let enums = option_ctx();
        let ctx = MatchCtx { enums: &enums };
        let matrix = vec![vec![NormPat::Wild]];
        assert!(is_exhaustive(&matrix, &[opt_int()], &ctx).is_none());
    }

    #[test]
    fn useful_and_unreachable() {
        let enums = option_ctx();
        let ctx = MatchCtx { enums: &enums };
        let matrix = vec![vec![NormPat::Wild]];
        assert!(!is_useful(
            &matrix,
            &[NormPat::Ctor(0, vec![NormPat::Wild])],
            &[opt_int()],
            &ctx
        ));
        let empty: Vec<Vec<NormPat>> = Vec::new();
        assert!(is_useful(
            &empty,
            &[NormPat::Ctor(0, vec![NormPat::Wild])],
            &[opt_int()],
            &ctx
        ));
    }

    #[test]
    fn nested_witness_for_nested_option() {
        let enums = option_ctx();
        let ctx = MatchCtx { enums: &enums };
        let nested = Type::Enum("Option".to_string(), vec![opt_int()]);
        let matrix = vec![
            vec![NormPat::Ctor(
                0,
                vec![NormPat::Ctor(0, vec![NormPat::Wild])],
            )],
            vec![NormPat::Ctor(1, vec![])],
        ];
        let w = is_exhaustive(&matrix, std::slice::from_ref(&nested), &ctx).expect("witness");
        assert_eq!(
            witness_to_string(&w, &[nested], &ctx),
            "Option::Some(Option::None)"
        );
    }
}
