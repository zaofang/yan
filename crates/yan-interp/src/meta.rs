//! `std.meta` (10.4, RFC 0051 D1, D2): `Type` handles and `info`.

use std::collections::HashMap;
use std::rc::Rc;

use yan_core::ast::*;
use yan_std::registry::{Id, id::STD_META_INFO};

use crate::bind::Bind;
use crate::{R, Value, unsupported};

pub(super) static BINDS: &[(Id, Bind)] = &[(STD_META_INFO, Bind::Plain(|a| info(a)))];

/// The records, sums and newtypes of a module, by path, with their annotations.
#[derive(Debug, Default)]
pub(crate) struct Types(HashMap<Name, (Vec<Attr>, ItemKind)>);

impl Types {
    pub(crate) fn of(m: &Module) -> Self {
        Types(
            m.items
                .iter()
                .filter_map(|it| {
                    let path = match &it.kind {
                        ItemKind::Record(r) => &r.path,
                        ItemKind::Sum(s) => &s.path,
                        ItemKind::Newtype(n) => &n.path,
                        _ => return None,
                    };
                    Some((path.join("."), (it.attrs.clone(), it.kind.clone())))
                })
                .collect(),
        )
    }
}

/// A resolved type and the declarations `info` reads it against.
#[derive(Debug)]
pub(crate) struct Handle {
    pub ty: Type,
    types: Rc<Types>,
}

/// Handles compare by type identity: the resolved type, not the table it came with.
impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        self.ty == other.ty
    }
}

pub(crate) fn handle(ty: Type, types: &Rc<Types>) -> Value {
    Value::Type(Rc::new(Handle {
        ty,
        types: types.clone(),
    }))
}

fn data(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Data {
        name: name.into(),
        fields: fields.into_iter().map(|(n, v)| (n.into(), v)).collect(),
    }
}

fn s(x: &str) -> Value {
    Value::Str(x.into())
}

/// `std.meta.info(t)`.
fn info(args: &[Value]) -> R<Value> {
    let [Value::Type(h)] = args else {
        return unsupported("`std.meta.info` without a type");
    };
    let t = |ty: &Type| handle(ty.clone(), &h.types);
    let ts = |tys: &[Type]| Value::List(tys.iter().map(t).collect());
    let one =
        |variant: &str, record: &str, fields| data(variant, vec![("info", data(record, fields))]);
    Ok(match &h.ty {
        Type::Tuple(xs) => data("Tuple", vec![("elems", ts(xs))]),
        Type::Fn(f) => one(
            "Fn",
            "FnInfo",
            vec![
                (
                    "params",
                    Value::List(f.params.iter().map(|(_, p)| t(p)).collect()),
                ),
                ("ret", t(&f.ret)),
                (
                    "fails",
                    match f.fail.as_deref() {
                        None => Value::List(Vec::new()),
                        Some(Type::Union(xs)) => ts(xs),
                        Some(x) => ts(std::slice::from_ref(x)),
                    },
                ),
                (
                    "uses",
                    Value::List(match &f.uses {
                        Some(CapSet::Names(ns)) => ns.iter().map(|n| s(n)).collect(),
                        _ => Vec::new(),
                    }),
                ),
            ],
        ),
        Type::Named(n) => {
            let name = n.path.join(".");
            let args: Vec<Type> = n
                .args
                .iter()
                .filter_map(|a| match a {
                    TArg::Type(x) => Some(x.clone()),
                    TArg::Nat(_) => None,
                })
                .collect();
            if let Some((attrs, kind)) = h.types.0.get(&name) {
                return Ok(declared(&name, &args, attrs, kind, &h.types));
            }
            if let Some((bits, signed, float)) = scalar(&name) {
                return Ok(one(
                    "Scalar",
                    "ScalarInfo",
                    vec![
                        ("name", s(&name)),
                        ("bits", Value::Int(bits)),
                        ("signed", Value::Bool(signed)),
                        ("float", Value::Bool(float)),
                    ],
                ));
            }
            match (name.as_str(), args.as_slice()) {
                ("Option", [x]) => data("Optional", vec![("inner", t(x))]),
                _ => one(
                    "Builtin",
                    "BuiltinInfo",
                    vec![("name", s(&name)), ("args", ts(&args))],
                ),
            }
        }
        _ => return unsupported("`std.meta.info` of this type"),
    })
}

/// Bits, signedness and floatness of a scalar by its Yan Core name.
fn scalar(n: &str) -> Option<(i128, bool, bool)> {
    Some(match n {
        "bool" => (1, false, false),
        "char" => (32, false, false),
        "Addr" => (64, false, false),
        "f32" => (32, true, true),
        "f64" => (64, true, true),
        _ => {
            let (signed, bits) = n.split_at(1);
            let bits: i128 = bits.parse().ok()?;
            match signed {
                "i" => (bits, true, false),
                "u" => (bits, false, false),
                _ => return None,
            }
        }
    })
}

/// `ty` with the type parameters `ps` replaced by `args`.
fn subst(ty: &Type, ps: &[TParam], args: &[Type]) -> Type {
    let go = |t: &Type| subst(t, ps, args);
    match ty {
        Type::Named(n) if n.args.is_empty() && n.path.len() == 1 => ps
            .iter()
            .zip(args)
            .find(|(p, _)| p.name == n.path[0])
            .map_or_else(|| ty.clone(), |(_, a)| a.clone()),
        Type::Named(n) => Type::Named(Named {
            path: n.path.clone(),
            args: n
                .args
                .iter()
                .map(|a| match a {
                    TArg::Type(t) => TArg::Type(go(t)),
                    a => a.clone(),
                })
                .collect(),
        }),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(go).collect()),
        Type::Union(xs) => Type::Union(xs.iter().map(go).collect()),
        Type::Dyn(_) | Type::Fn(_) | Type::Refined { .. } => ty.clone(),
    }
}

fn annotations(attrs: &[Attr]) -> Value {
    Value::List(
        attrs
            .iter()
            .map(|a| data("Annotation", vec![("name", s(&a.name))]))
            .collect(),
    )
}

/// A record, sum or newtype of the module, applied to `args`.
fn declared(
    name: &str,
    args: &[Type],
    attrs: &[Attr],
    kind: &ItemKind,
    types: &Rc<Types>,
) -> Value {
    let params = || Value::List(args.iter().map(|a| handle(a.clone(), types)).collect());
    let fields = |ps: &[TParam], fs: &[Field]| {
        Value::List(
            fs.iter()
                .map(|f| {
                    data(
                        "FieldInfo",
                        vec![
                            ("name", s(&f.name)),
                            ("ty", handle(subst(&f.ty, ps, args), types)),
                            ("public", Value::Bool(f.is_pub)),
                            ("annotations", Value::List(Vec::new())),
                        ],
                    )
                })
                .collect(),
        )
    };
    let wrap = |variant: &str, record: &str, fs| data(variant, vec![("info", data(record, fs))]);
    match kind {
        ItemKind::Record(r) => wrap(
            "Record",
            "RecordInfo",
            vec![
                ("name", s(name)),
                ("params", params()),
                ("fields", fields(&r.tparams, &r.fields)),
                ("annotations", annotations(attrs)),
            ],
        ),
        ItemKind::Sum(su) => wrap(
            "Sum",
            "SumInfo",
            vec![
                ("name", s(name)),
                ("params", params()),
                (
                    "variants",
                    Value::List(
                        su.variants
                            .iter()
                            .map(|v| {
                                data(
                                    "VariantInfo",
                                    vec![
                                        ("name", s(&v.name)),
                                        (
                                            "fields",
                                            fields(&su.tparams, v.fields.as_deref().unwrap_or(&[])),
                                        ),
                                    ],
                                )
                            })
                            .collect(),
                    ),
                ),
                ("annotations", annotations(attrs)),
            ],
        ),
        ItemKind::Newtype(n) => wrap(
            "Newtype",
            "NewtypeInfo",
            vec![
                ("name", s(name)),
                ("inner", handle(subst(&n.ty, &n.tparams, args), types)),
                ("annotations", annotations(attrs)),
            ],
        ),
        _ => unreachable!("Types holds only records, sums and newtypes"),
    }
}
