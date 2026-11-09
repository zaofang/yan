//! `yan mcp package` (RFC 0061): each public function of the served module, other than `main`
//! and tests, is a tool. Its schema rides to the server as a `tool` attribute on the Core item.

use serde_json::json;
use yan_core::ast as y;
use yan_hir as j;

use crate::{Cx, Field, Globals, jsonwire};

pub(crate) fn attach(g: &Globals, m: &j::Module, items: &mut [y::Item], cx: &mut Cx) {
    for it in &m.items {
        let j::ItemKind::Func(f) = &it.kind else {
            continue;
        };
        let name = f.name.as_str();
        if !it.is_pub || name == "main" || name.starts_with("test_") {
            continue;
        }
        let Some(sig) = g.fns.get(name) else {
            continue;
        };
        let refuse = |cx: &mut Cx, why: String| {
            cx.err(
                "E201",
                it.name_span,
                name,
                format!(
                    "`{name}` cannot be served as a tool: {why}; make it private to keep it out"
                ),
            );
        };
        if !sig.tparams.is_empty() {
            refuse(cx, "it is generic".into());
            continue;
        }
        if sig.caps.iter().any(|c| c == "io") {
            refuse(cx, "it uses `io`, and stdio carries the protocol".into());
            continue;
        }
        if let Some(p) = sig.params.iter().find(|p| p.conv == y::Conv::Inout) {
            refuse(cx, format!("parameter `{}` is `inout`", p.name));
            continue;
        }
        let params: Vec<Field> = sig
            .params
            .iter()
            .map(|p| Field {
                name: p.name.clone(),
                ty: p.ty.clone(),
                ty_span: p.span,
                default: p.default.clone(),
                wire: None,
                attrs: Vec::new(),
            })
            .collect();
        let schema = match jsonwire::tool(g, &params, &sig.ret) {
            Ok(s) => s,
            Err(what) => {
                refuse(cx, format!("its {what} has no json form (derive `Wire`)"));
                continue;
            }
        };
        let doc = f.body.as_ref().and_then(|b| b.doc.clone());
        let tool = json!({
            "doc": doc,
            "uses": sig.caps,
            "params": sig.params.iter().map(|p| &p.name).collect::<Vec<_>>(),
            "schema": schema,
        });
        if let Some(item) = items
            .iter_mut()
            .find(|i| matches!(&i.kind, y::ItemKind::Fn(d) if d.path == [name]))
        {
            item.attrs.push(y::Attr {
                name: "tool".into(),
                args: Some(vec![y::AttrArg {
                    name: None,
                    value: y::ConstVal::Lit(y::Literal::Str(tool.to_string())),
                }]),
            });
        }
    }
}
