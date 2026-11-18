//! `Inst::Move` markers (RFC 0087): which places a statement moves out of, from the ownership
//! facts of `yan_check::own`.

use super::*;
use yan_check::own;

/// Aggregates of these are copy; everything else is an owned value.
fn is_copy(s: &Shape) -> bool {
    match s {
        Shape::Scalar(_) | Shape::Char | Shape::Ref(_) => true,
        Shape::Tuple(ts) => ts.iter().all(is_copy),
        _ => false,
    }
}

fn place(root: &str) -> y::Place {
    y::Place {
        root: root.to_owned(),
        root_span: Default::default(),
        projs: Vec::new(),
    }
}

impl<'a, 'm> Fn<'a, 'm> {
    /// Marks what `k` consumes, after the statement lowered. Call-argument and name uses of an
    /// owned local mark the whole local; a field moved through a view marks that field.
    pub(super) fn mark_moves(&mut self, k: &'m y::StmtKind) {
        let c = own::consumption(k, &self.views);
        let mut places = Vec::new();
        // `rest` aliases its root's locals: the remainder is still readable.
        let rest = match k {
            y::StmtKind::Let {
                rhs: y::Rhs::Rest { root, .. },
                ..
            } => Some(root),
            _ => None,
        };
        for root in c.whole.iter().filter(|r| Some(*r) != rest) {
            self.locs(&place(root), &mut places);
        }
        for (root, path) in &c.partial {
            let mut p = place(root);
            p.projs = path
                .iter()
                .map(|f| y::Proj::Field(f.clone(), p.root_span))
                .collect();
            self.locs(&p, &mut places);
        }
        if !places.is_empty() {
            self.emit(Inst::Move { places });
        }
    }

    /// Marks `width` words at `ptr` moved: `Ptr.take` leaves its pointee moved-from.
    pub(super) fn mark_moved_at(&mut self, ptr: &Operand, shape: &Shape) {
        if is_copy(shape) {
            return;
        }
        let places = (0..shape.width() as u32)
            .map(|word| Loc::Word {
                ptr: ptr.clone(),
                word,
            })
            .collect();
        self.emit(Inst::Move { places });
    }

    /// Marks the whole local `n` moved, after its drop glue ran.
    pub(super) fn mark_name(&mut self, n: &str) {
        let mut places = Vec::new();
        self.locs(&place(n), &mut places);
        if !places.is_empty() {
            self.emit(Inst::Move { places });
        }
    }

    fn locs(&mut self, p: &y::Place, out: &mut Vec<Loc>) {
        let Ok((at, shape)) = self.place(p) else {
            return;
        };
        if is_copy(&shape) {
            return;
        }
        match at {
            At::Reg(ls) => out.extend(ls.into_iter().map(Loc::Local)),
            At::Mem(ptr, word) => out.extend((0..shape.width() as u32).map(|i| Loc::Word {
                ptr: ptr.clone(),
                word: word + i,
            })),
        }
    }

    /// The view a `let x = rhs` binds, for the moves that go through `x`.
    pub(super) fn bind_view(&mut self, n: &str, rhs: &y::Rhs, shape: &Shape) {
        match rhs {
            y::Rhs::Proj(p) if !is_copy(shape) => {
                let v = own::proj_view(&self.views, p);
                self.views.insert(n.to_owned(), v);
            }
            _ => drop(self.views.remove(n)),
        }
    }
}
