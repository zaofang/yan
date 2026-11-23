use super::*;

/// The `sched` handler in effect while lowering a body (R1003).
#[derive(Clone, Copy)]
pub(super) struct Ctx {
    /// The handler does not implement `Concurrent` (E708).
    pub not_concurrent: bool,
    /// The handler is `pool`, which has no quiescence point for `virtual` time (E710).
    pub pool: bool,
}

impl Ctx {
    /// The hosted default handler is `pool`, which is not `Concurrent`; only `main` has no caller to inherit from.
    pub fn of_fn(name: &str) -> Self {
        Ctx {
            not_concurrent: name == "main",
            pool: false,
        }
    }
}

impl Lower<'_, '_, '_> {
    /// The context a `with sched = h` body runs under, `h` of type `ht`: `Concurrent` is a std
    /// handler constructor marked `@concurrent`, or a type with `impl Concurrent`. The opaque
    /// `Sched` type of any other expression does not.
    pub(super) fn sched_of(&self, h: &j::Expr, ht: &Ty) -> Ctx {
        let concurrent = if let j::ExprKind::Call(f, _) = &h.kind
            && let j::ExprKind::Name(n) = &f.kind
            && let Some(d) = ["sched", "sim", "evented"]
                .iter()
                .find_map(|m| crate::stdlib::decl(&format!("std.{m}.{n}")))
        {
            d.concurrent
        } else {
            self.implements(ht, "Concurrent")
        };
        Ctx {
            not_concurrent: !concurrent,
            pool: is_call(h, "pool"),
        }
    }
}

pub(super) fn is_call(e: &j::Expr, name: &str) -> bool {
    matches!(&e.kind, j::ExprKind::Call(f, _)
        if matches!(&f.kind, j::ExprKind::Name(n) if n == name))
}
