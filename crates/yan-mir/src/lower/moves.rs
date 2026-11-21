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
        for root in &c.whole {
            self.locs(&place(root), &mut places);
        }
        for (root, path) in &c.partial {
            let Ok((at, shape)) = self.view_at(root, path) else {
                continue;
            };
            self.shape_locs(at, &shape, &mut places);
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

    /// The words of the field at `path` of the local `root`; through a sum, the path names a
    /// variant and then its field (`own::view_paths`).
    fn view_at(&mut self, root: &str, path: &[String]) -> R<(At, Shape)> {
        let (mut at, mut shape) = self.place(&place(root))?;
        let mut path = path.iter();
        while let Some(f) = path.next() {
            if matches!(shape, Shape::Sum(..)) {
                let Some((_, first, fs)) = shape.variant(f) else {
                    return unsupported(format!("variant `{f}`"));
                };
                let Some(f) = path.next() else {
                    return unsupported(format!("a move out of variant `{f}` as a whole"));
                };
                let mut start = first;
                let mut found = None;
                for (n, s) in fs {
                    if n == f {
                        found = Some(s.clone());
                        break;
                    }
                    start += s.width();
                }
                let Some(s) = found else {
                    return unsupported(format!("field `{f}`"));
                };
                (at, shape) = self.sub_at(at, start, s);
            } else {
                (at, shape) = self.field_at(at, &shape, f)?;
            }
        }
        Ok((at, shape))
    }

    fn locs(&mut self, p: &y::Place, out: &mut Vec<Loc>) {
        let Ok((at, shape)) = self.place(p) else {
            return;
        };
        self.shape_locs(at, &shape, out);
    }

    fn shape_locs(&mut self, at: At, shape: &Shape, out: &mut Vec<Loc>) {
        if is_copy(shape) {
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

    /// Makes the names `p` binds view the scrutinee, for the length of an arm.
    pub(super) fn bind_arm_views(&mut self, scrutinee: &y::Atom, p: &y::Pat) -> Vec<String> {
        let y::Atom::Name(x, _) = scrutinee else {
            return Vec::new();
        };
        let base = own::scrutinee_view(&self.views, x);
        let bound = own::pat_views(p, &self.types.ctors, None, &base);
        let names = bound.iter().map(|(n, _)| n.clone()).collect();
        self.views.extend(bound);
        names
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
