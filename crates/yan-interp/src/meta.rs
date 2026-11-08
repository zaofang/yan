//! `std.meta` (10.4, RFC 0051 D1, D2): `Type` handles and `info`.

use std::collections::HashMap;
use std::rc::Rc;

use yan_core::ast::*;
use yan_std::registry::{
    Id,
    id::{STD_META_INFO, STD_META_NEWTYPE, STD_META_RECORD_TYPE, STD_META_SUM_TYPE},
};

use crate::bind::Bind;
use crate::{R, Value, panic_with, unsupported};

pub(super) static BINDS: &[(Id, Bind)] = &[
    (STD_META_INFO, Bind::Plain(|a| info(a))),
    (
        STD_META_RECORD_TYPE,
        Bind::Plain(|a| reify(a, Reifier::Record)),
    ),
    (STD_META_SUM_TYPE, Bind::Plain(|a| reify(a, Reifier::Sum))),
    (
        STD_META_NEWTYPE,
        Bind::Plain(|a| reify(a, Reifier::Newtype)),
    ),
];

/// The records, sums, newtypes and traits of a module, by path, with their annotations.
#[derive(Debug, Default)]
pub(crate) struct Types {
    decls: HashMap<Name, (Vec<Attr>, ItemKind)>,
    /// The declarations `record_type`, `sum_type` and `newtype` made, in dependency order.
    reified: Vec<Name>,
}

impl Types {
    pub(crate) fn of(m: &Module) -> Self {
        let decls = m
            .items
            .iter()
            .filter_map(|it| {
                let path = match &it.kind {
                    ItemKind::Record(r) => &r.path,
                    ItemKind::Sum(s) => &s.path,
                    ItemKind::Newtype(n) => &n.path,
                    ItemKind::Trait(t) => &t.path,
                    _ => return None,
                };
                Some((path.join("."), (it.attrs.clone(), it.kind.clone())))
            })
            .collect();
        Types {
            decls,
            reified: Vec::new(),
        }
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
        Type::Fn(f) => data(
            "Fn",
            vec![(
                "info",
                fn_info(
                    f.params.iter().map(|(_, p)| p),
                    &f.ret,
                    f.fail.as_deref(),
                    match &f.uses {
                        Some(CapSet::Names(ns)) => ns.clone(),
                        _ => Vec::new(),
                    },
                    &h.types,
                ),
            )],
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
            match h.types.decls.get(&name) {
                Some((_, ItemKind::Trait(tr))) => {
                    let methods = tr
                        .sigs
                        .iter()
                        .map(|f| {
                            data(
                                "MethodInfo",
                                vec![("name", s(&f.path.join("."))), ("sig", sig(f, &h.types))],
                            )
                        })
                        .collect();
                    return Ok(one(
                        "Trait",
                        "TraitInfo",
                        vec![("name", s(&name)), ("methods", Value::List(methods))],
                    ));
                }
                Some((attrs, kind)) => return declared(&name, &args, attrs, kind, &h.types),
                None => {}
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
                ("View" | "MutView", [x]) => one(
                    "View",
                    "ViewInfo",
                    vec![("elem", t(x)), ("mutable", Value::Bool(name == "MutView"))],
                ),
                ("Array", [.., elem]) => {
                    let axes = n.args[..n.args.len() - 1]
                        .iter()
                        .map(|a| match a {
                            TArg::Nat(k) => match k.parse() {
                                Ok(k) => data("Positional", vec![("size", Value::Int(k))]),
                                Err(_) => data("Variable", vec![("name", s(k))]),
                            },
                            TArg::Type(l) => data("Labelled", vec![("label", t(l))]),
                        })
                        .collect();
                    one(
                        "Array",
                        "ArrayInfo",
                        vec![("elem", t(elem)), ("axes", Value::List(axes))],
                    )
                }
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

/// An `FnInfo`: parameter types, return, failures and capabilities.
fn fn_info<'a>(
    params: impl Iterator<Item = &'a Type>,
    ret: &Type,
    fail: Option<&Type>,
    uses: Vec<Name>,
    types: &Rc<Types>,
) -> Value {
    let t = |ty: &Type| handle(ty.clone(), types);
    let fails = match fail {
        None => Vec::new(),
        Some(Type::Union(xs)) => xs.iter().map(t).collect(),
        Some(x) => vec![t(x)],
    };
    data(
        "FnInfo",
        vec![
            ("params", Value::List(params.map(t).collect())),
            ("ret", t(ret)),
            ("fails", Value::List(fails)),
            ("uses", Value::List(uses.iter().map(|n| s(n)).collect())),
        ],
    )
}

/// The `FnInfo` of a trait method's signature; capability parameters are its `uses`.
fn sig(f: &FnDef, types: &Rc<Types>) -> Value {
    let mut uses = Vec::new();
    let mut params = Vec::new();
    for p in &f.params {
        match p {
            Param::Value { ty, .. } => params.push(ty),
            Param::Cap { ty, .. } => uses.push(ty.path.join(".")),
        }
    }
    fn_info(params.into_iter(), &f.ret, f.fail.as_ref(), uses, types)
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
) -> R<Value> {
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
                            ("annotations", annotations(&f.attrs)),
                        ],
                    )
                })
                .collect(),
        )
    };
    let wrap = |variant: &str, record: &str, fs| data(variant, vec![("info", data(record, fs))]);
    let marked = attrs.iter().any(|a| a.name == "newtype");
    let attrs: Vec<Attr> = attrs
        .iter()
        .filter(|a| a.name != "newtype")
        .cloned()
        .collect();
    let attrs = attrs.as_slice();
    Ok(match kind {
        ItemKind::Record(r)
            if marked && matches!(r.fields.as_slice(), [f] if f.name == "value") =>
        {
            wrap(
                "Newtype",
                "NewtypeInfo",
                vec![
                    ("name", s(name)),
                    (
                        "inner",
                        handle(subst(&r.fields[0].ty, &r.tparams, args), types),
                    ),
                    ("annotations", annotations(attrs)),
                ],
            )
        }
        ItemKind::Record(r) => wrap(
            "Record",
            "RecordInfo",
            vec![
                ("name", s(name)),
                ("params", params()),
                ("fields", fields(&r.tparams, &r.fields)),
                ("annotations", annotations(attrs)),
                ("layout", layout(name, args, kind, types)?),
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
                ("layout", layout(name, args, kind, types)?),
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
        _ => unreachable!("traits are described by `info`"),
    })
}

/// `Layout(size, align)` of a record or sum applied to `args`, under the default target layout.
fn layout(name: &str, args: &[Type], kind: &ItemKind, types: &Types) -> R<Value> {
    let applied = match kind {
        ItemKind::Record(r) => ItemKind::Record(Record {
            tparams: Vec::new(),
            fields: r
                .fields
                .iter()
                .map(|f| Field {
                    ty: subst(&f.ty, &r.tparams, args),
                    ..f.clone()
                })
                .collect(),
            ..r.clone()
        }),
        ItemKind::Sum(su) => ItemKind::Sum(Sum {
            tparams: Vec::new(),
            variants: su
                .variants
                .iter()
                .map(|v| Variant {
                    fields: v.fields.as_ref().map(|fs| {
                        fs.iter()
                            .map(|f| Field {
                                ty: subst(&f.ty, &su.tparams, args),
                                ..f.clone()
                            })
                            .collect()
                    }),
                    ..v.clone()
                })
                .collect(),
            ..su.clone()
        }),
        _ => unreachable!("only records and sums have a layout"),
    };
    let ty = Type::Named(Named {
        path: vec![name.into()],
        args: Vec::new(),
    });
    let decls = types.decls.values().map(|(_, k)| k).chain([&applied]);
    match yan_mir::layout_of(decls, &ty) {
        Ok(l) => Ok(data(
            "Layout",
            vec![
                ("size", Value::Int(l.size.into())),
                ("align", Value::Int(l.align.into())),
            ],
        )),
        Err(e) => unsupported(format!("the layout of `{name}`: {e}")),
    }
}

/// What a stage-0 `Type` result stands for: the type and the declarations it was reified from.
pub(crate) fn reified(v: &Value) -> Option<crate::Reified> {
    let Value::Type(h) = v else { return None };
    Some(crate::Reified {
        ty: h.ty.clone(),
        decls: h
            .types
            .reified
            .iter()
            .map(|n| h.types.decls[n].clone())
            .map(|(attrs, kind)| Item {
                attrs,
                kind,
                span: yan_diag::Span::default(),
            })
            .collect(),
    })
}

#[derive(Clone, Copy)]
enum Reifier {
    Record,
    Sum,
    Newtype,
}

fn field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    let Value::Data { fields, .. } = v else {
        return None;
    };
    fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Str(x)) => Some(x.to_string()),
        _ => None,
    }
}

fn items(v: Option<&Value>) -> Option<&[Value]> {
    match v {
        Some(Value::List(xs)) => Some(xs),
        _ => None,
    }
}

/// The handles inside `v`'s declarations are merged into `into`, so a reified type keeps the
/// declarations of the reified types it mentions.
fn merge(into: &mut Types, from: &Types) {
    for n in &from.reified {
        if !into.decls.contains_key(n) {
            into.decls.insert(n.clone(), from.decls[n].clone());
            into.reified.push(n.clone());
        }
    }
    for (n, d) in &from.decls {
        into.decls.entry(n.clone()).or_insert_with(|| d.clone());
    }
}

fn fields_of(
    list: &[Value],
    into: &mut Types,
    owner: &str,
    what: &str,
) -> R<Vec<yan_core::ast::Field>> {
    let mut out: Vec<yan_core::ast::Field> = Vec::new();
    for f in list {
        let (Some(name), Some(Value::Type(h)), Some(Value::Bool(public))) =
            (text(field(f, "name")), field(f, "ty"), field(f, "public"))
        else {
            return unsupported("a `FieldInfo` that is not name, ty, public and annotations");
        };
        if out.iter().any(|g| g.name == name) {
            return panic_with(
                "reify",
                format!("{what} `{owner}` has two fields named `{name}`"),
            );
        }
        merge(into, &h.types);
        out.push(yan_core::ast::Field {
            attrs: attrs_of(field(f, "annotations")),
            is_pub: *public,
            name,
            ty: h.ty.clone(),
            default: None,
        });
    }
    Ok(out)
}

fn attrs_of(v: Option<&Value>) -> Vec<Attr> {
    items(v)
        .into_iter()
        .flatten()
        .filter_map(|a| text(field(a, "name")))
        .map(|name| Attr { name, args: None })
        .collect()
}

/// `record_type`, `sum_type` and `newtype` (RFC 0051 D3): a nominal type named after the
/// reflection record. Its checking is the elaborator's, which declares it like a written type.
fn reify(args: &[Value], how: Reifier) -> R<Value> {
    let [info] = args else {
        return unsupported("a reifier without one reflection record");
    };
    let Some(name) = text(field(info, "name")) else {
        return unsupported("a reflection record without a `name`");
    };
    let mut types = Types::default();
    let tparams = Vec::new();
    let kind = match how {
        Reifier::Record => {
            let Some(fs) = items(field(info, "fields")) else {
                return unsupported("a `RecordInfo` without `fields`");
            };
            ItemKind::Record(Record {
                is_pub: true,
                path: vec![name.clone()],
                tparams,
                fields: fields_of(fs, &mut types, &name, "record")?,
            })
        }
        Reifier::Sum => {
            let Some(vs) = items(field(info, "variants")) else {
                return unsupported("a `SumInfo` without `variants`");
            };
            let mut variants: Vec<Variant> = Vec::new();
            for v in vs {
                let Some(label) = text(field(v, "name")) else {
                    return unsupported("a `VariantInfo` without a `name`");
                };
                if variants.iter().any(|w| w.name == label) {
                    return panic_with(
                        "reify",
                        format!("sum `{name}` has two variants named `{label}`"),
                    );
                }
                let fs = items(field(v, "fields")).unwrap_or(&[]);
                let fs = fields_of(fs, &mut types, &name, "variant")?;
                variants.push(Variant {
                    name: label,
                    fields: (!fs.is_empty()).then_some(fs),
                });
            }
            ItemKind::Sum(Sum {
                is_pub: true,
                path: vec![name.clone()],
                tparams,
                variants,
            })
        }
        Reifier::Newtype => {
            let Some(Value::Type(inner)) = field(info, "inner") else {
                return unsupported("a `NewtypeInfo` without `inner`");
            };
            merge(&mut types, &inner.types);
            ItemKind::Newtype(Newtype {
                is_pub: true,
                path: vec![name.clone()],
                tparams,
                ty: inner.ty.clone(),
                pred: None,
            })
        }
    };
    let attrs = attrs_of(field(info, "annotations"));
    let ty = Type::Named(Named {
        path: vec![name.clone()],
        args: Vec::new(),
    });
    types.decls.insert(name.clone(), (attrs, kind));
    types.reified.push(name);
    Ok(Value::Type(Rc::new(Handle {
        ty,
        types: Rc::new(types),
    })))
}
