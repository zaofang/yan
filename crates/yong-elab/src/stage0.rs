//! Stage-0 constants (10.4): `let x: T = #e` is evaluated on the interpreter and its value
//! materialized as the constant.

use std::path::{Component, Path, PathBuf};

use yong_core::ast as y;
use yong_diag::Span;
use yong_hir as j;
use yong_interp::{Limits, Stage0, Stop};

use crate::{Cx, Globals, R, lower, unsupported};

/// The value of the stage-0 constant `name = #e`, or none after reporting why it has none.
pub(crate) fn constant(
    g: &Globals,
    it: &j::Item,
    name: &str,
    e: &j::Expr,
    at: Span,
    items: &[y::Item],
    cx: &mut Cx,
) -> R<Option<y::ConstVal>> {
    if let j::ExprKind::Call(f, args) = &e.kind
        && matches!(&f.kind, j::ExprKind::Name(n) if n == "embed")
    {
        return embed(name, args, cx);
    }
    let j::ItemKind::Const { ty, .. } = &it.kind else {
        unreachable!("a constant")
    };
    let fname = format!("__stage0_{name}");
    let f = j::FuncDef {
        name: fname.clone(),
        name_span: at,
        tparams: Vec::new(),
        params: Vec::new(),
        ret: Some(j::RetType {
            ty: Some(ty.clone()),
            fail: Vec::new(),
            from: Vec::new(),
        }),
        uses: None,
        body: Some(j::Body {
            doc: None,
            contracts: Vec::new(),
            stmts: vec![j::Stmt {
                kind: j::StmtKind::Return(Some(e.clone())),
                span: at,
            }],
        }),
    };
    let errors = |cx: &Cx| {
        cx.diags
            .iter()
            .filter(|d| d.severity == yong_diag::Severity::Error)
            .count()
    };
    let before = errors(cx);
    let sig = crate::sig(g, &f, cx)?;
    let fd = lower::function(g, &f, &sig, false, cx)?;
    if errors(cx) > before {
        return Ok(None);
    }
    let mut module = y::Module {
        header: Vec::new(),
        path: vec!["stage0".into()],
        items: items.to_vec(),
    };
    module.items.push(y::Item {
        attrs: Vec::new(),
        kind: y::ItemKind::Fn(fd),
        span: at,
    });
    let (code, msg) = match yong_interp::stage0(&module, &fname, limits(it)?) {
        Ok(Stage0::Value(v)) => return Ok(Some(v)),
        Ok(Stage0::NotMaterializable(what)) => (
            "E262",
            format!("{what} cannot cross into run time; only data is materialized"),
        ),
        Err(Stop::Panic(kind, _)) => ("E260", format!("stage-0 evaluation panicked: {kind}")),
        Err(Stop::Limit(what)) => (
            "E261",
            format!("stage-0 evaluation exceeded its {what} limit; raise it with `@limit`"),
        ),
        Err(Stop::Unsupported(what)) => return unsupported(format!("at stage 0: {what}")),
    };
    cx.err(code, at, name, msg);
    Ok(None)
}

/// The budgets of 10.4, raised by `@limit(stage0_depth: n, stage0_steps: n)`.
fn limits(it: &j::Item) -> R<Limits> {
    let mut l = Limits::default();
    for d in it.decorators.iter().filter(|d| d.path == ["limit"]) {
        for a in d.args.iter().flatten() {
            let j::ArgKind::Expr(j::Expr {
                kind: j::ExprKind::Lit(j::Lit::Int(n)),
                ..
            }) = &a.kind
            else {
                return unsupported("a `@limit` value that is not an integer literal");
            };
            let n: u64 = n
                .replace('_', "")
                .parse()
                .map_err(|_| crate::Unsupported(format!("`@limit` value {n}")))?;
            match a.name.as_deref() {
                Some("stage0_depth") => l.depth = n as usize,
                Some("stage0_steps") => l.steps = n,
                Some("stage0_heap") => {}
                _ => return unsupported("this `@limit`"),
            }
        }
    }
    Ok(l)
}

/// `#embed(path)`: the file's text, from inside the package root (E267). The root is the
/// nearest directory above the module with a `yong.toml`, else the module's own directory.
fn embed(name: &str, args: &[j::Arg], cx: &mut Cx) -> R<Option<y::ConstVal>> {
    let [
        j::Arg {
            kind:
                j::ArgKind::Expr(j::Expr {
                    kind: j::ExprKind::Lit(j::Lit::Str { value, .. }),
                    span,
                    ..
                }),
            ..
        },
    ] = args
    else {
        return unsupported("`embed` with other than one string literal");
    };
    let dir = Path::new(cx.file)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let root = dir
        .ancestors()
        .find(|d| d.join("yong.toml").is_file())
        .unwrap_or(dir);
    let path = normalize(&dir.join(value));
    let root = normalize(root);
    if path.starts_with("..") || !path.starts_with(&root) {
        cx.err(
            "E267",
            *span,
            name,
            format!("`{value}` is outside the package root"),
        );
        return Ok(None);
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(y::ConstVal::Lit(y::Literal::Str(text)))),
        Err(e) => unsupported(format!("embedding {}: {e}", path.display())),
    }
}

/// `p` with `.` and `..` resolved lexically.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out
}
