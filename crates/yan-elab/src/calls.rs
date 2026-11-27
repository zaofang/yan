//! The names a function body calls, the edges std-body discovery follows.

use std::collections::HashSet;

use yan_hir as j;

use crate::lower::{lenient_py::is_formatted_template, stmt_exprs, sub_blocks, walk};

pub(crate) fn callees(stmts: &[j::Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        for e in stmt_exprs(s) {
            expr_callees(e, out);
        }
        for b in sub_blocks(s) {
            callees(b, out);
        }
    }
}

/// The callees of a body, contracts included: a `post all(...)` calls `all` too.
pub(crate) fn body_callees(b: &j::Body, out: &mut HashSet<String>) {
    callees(&b.stmts, out);
    for c in &b.contracts {
        match c {
            j::Contract::Pre(e) | j::Contract::Post(e) | j::Contract::Decreases(e) => {
                expr_callees(e, out);
            }
            j::Contract::Cost { steps, alloc } => {
                for e in steps.iter().chain(alloc) {
                    expr_callees(e, out);
                }
            }
            j::Contract::Ex { expr, with } => {
                expr_callees(expr, out);
                for (_, e) in with {
                    expr_callees(e, out);
                }
            }
        }
    }
}

fn expr_callees(e: &j::Expr, out: &mut HashSet<String>) {
    walk(e, &mut |e| {
        if matches!(e.kind, j::ExprKind::Binary("**", ..))
            || matches!(&e.kind, j::ExprKind::Call(f, _) if matches!(&f.kind, j::ExprKind::Name(n) if n == "pow"))
        {
            out.insert("int_pow".into());
            out.insert("bigint_pow".into());
        }
        if is_formatted_template(e)
            || matches!(&e.kind, j::ExprKind::FStr { pieces, .. }
                if pieces.iter().any(|p| matches!(p, j::FPiece::Expr(_, Some(_)))))
        {
            out.insert("fmt_pad".into());
            out.insert("fmt_sign".into());
            out.insert("fmt_group".into());
        }
        if let j::ExprKind::Call(f, args) = &e.kind {
            match &f.kind {
                j::ExprKind::Name(n) | j::ExprKind::Attr(_, n, _) => {
                    // N34 reads `xs.pop(i)` as `remove_at` and `d.pop(k)` as `remove`.
                    if n == "pop" && args.len() == 1 {
                        out.insert("remove_at".into());
                        out.insert("remove".into());
                    }
                    // `max(xs)` lowers to the iterator's `max_nonempty` (R658).
                    if n == "min" || n == "max" {
                        out.insert(format!("{n}_nonempty"));
                    }
                    // N37 may collect an `iter` from a source.
                    if matches!(n.as_str(), "chars" | "iter" | "graphemes" | "list") {
                        out.insert("collect".into());
                    }
                    out.insert(n.clone());
                }
                _ => {}
            }
        }
    });
}
