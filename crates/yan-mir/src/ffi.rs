//! Extern functions (12.1) that both backends can call: integer and `f64` parameters and
//! results, bound to the symbol of the declaring block's library.

use std::collections::HashMap;

use yan_core::ast as y;
use yan_rt::ffi::{FLOATS, INTS};

use crate::IntTy;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scalar {
    Int(IntTy),
    F64,
}

#[derive(Clone, Debug)]
pub struct ExternFn {
    pub link: String,
    pub params: Vec<Scalar>,
    /// `None` for a function with no result.
    pub ret: Option<Scalar>,
}

impl ExternFn {
    /// The result type code of `yan_rt::ffi::call`; 0 for no result.
    pub fn kind(&self) -> u8 {
        match self.ret {
            Some(Scalar::Int(t)) => yan_rt::ffi::int_kind(t.bits, t.signed),
            _ => 0,
        }
    }
}

fn scalar(t: &y::Type) -> Result<Scalar, String> {
    if let y::Type::Named(n) = t
        && let [name] = n.path.as_slice()
        && n.args.is_empty()
    {
        let int = |bits, signed| Ok(Scalar::Int(IntTy { bits, signed }));
        match name.as_str() {
            "i8" => return int(8, true),
            "i16" => return int(16, true),
            "i32" => return int(32, true),
            "i64" | "int" => return int(64, true),
            "u8" => return int(8, false),
            "u16" => return int(16, false),
            "u32" => return int(32, false),
            "u64" => return int(64, false),
            "f64" | "float" => return Ok(Scalar::F64),
            _ => {}
        }
    }
    Err(format!("extern type `{}`", yan_core::type_text(t)))
}

/// The callable externs of `m` by name, or why one cannot be called yet.
pub fn collect(m: &y::Module) -> HashMap<String, Result<ExternFn, String>> {
    let mut out = HashMap::new();
    for it in &m.items {
        let y::ItemKind::Extern(x) = &it.kind else {
            continue;
        };
        for xi in &x.items {
            let y::XItem::Fn {
                name,
                params,
                variadic,
                ret,
                ..
            } = xi
            else {
                continue;
            };
            out.insert(
                name.clone(),
                signature(x, params, *variadic, ret).map_err(|m| format!("{m} in native code")),
            );
        }
    }
    out
}

fn signature(
    x: &y::Extern,
    params: &[y::XParam],
    variadic: bool,
    ret: &y::Type,
) -> Result<ExternFn, String> {
    if variadic {
        return Err("a variadic extern call".into());
    }
    let ps = params
        .iter()
        .map(|p| match p {
            y::XParam::Value { ty, .. } => scalar(ty),
            y::XParam::Callback { .. } => Err("an extern callback".into()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if ps.iter().filter(|p| matches!(p, Scalar::Int(_))).count() > INTS
        || ps.iter().filter(|p| matches!(p, Scalar::F64)).count() > FLOATS
    {
        return Err("an extern call with this many arguments".into());
    }
    let ret = match ret {
        y::Type::Named(n) if n.path == ["Unit"] => None,
        t => Some(scalar(t)?),
    };
    Ok(ExternFn {
        link: x.link.clone().unwrap_or_else(|| x.lib.clone()),
        params: ps,
        ret,
    })
}
