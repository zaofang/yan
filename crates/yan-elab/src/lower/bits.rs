//! `@layout(bits(uN))` records (5.2): the value is one `uN`, fields packed LSB first.
//! Construction packs, a read shifts and narrows, an assignment reads-modifies-writes.

use super::*;

const FIELD: &str = "_bits";

fn container(n: u32) -> Option<Ty> {
    crate::ty::fixed(&format!("u{n}")).map(Ty::Fixed)
}

/// The one field a packed record has in Yan Core.
pub(crate) fn core_field(n: u32) -> y::Field {
    y::Field {
        attrs: Vec::new(),
        is_pub: true,
        name: FIELD.into(),
        ty: container(n).expect("a container of at most 64 bits").core(),
        default: None,
    }
}

fn width(t: &Ty) -> u32 {
    match t {
        Ty::Fixed(n) => n[1..].parse().expect("an integer width"),
        t => unreachable!("E908 left a field of type {t}"),
    }
}

impl Lower<'_, '_, '_> {
    /// The record name when `t` is a packed record.
    pub(super) fn bits_record(&self, t: &Ty) -> Option<String> {
        match t {
            Ty::Data(r) if self.g.layout_bits.contains_key(r) => Some(r.clone()),
            _ => None,
        }
    }

    /// The container type, and the offset and type of field `f`.
    fn bits_slot(&mut self, r: &str, f: &str, at: Span) -> R<(Ty, u32, Ty)> {
        let n = self.g.layout_bits[r];
        let Some(c) = container(n) else {
            return unsupported(format!("`@layout(bits(u{n}))`"));
        };
        let mut off = 0;
        for fd in &self.g.records[r] {
            if fd.name == f {
                return Ok((c, off, fd.ty.clone()));
            }
            off += width(&fd.ty);
        }
        let names = self.g.records[r].iter().map(|fd| fd.name.as_str());
        self.err_near("E203", at, format!("`{r}` has no field `{f}`"), f, names);
        Ok((c, 0, Ty::Err))
    }

    fn bits_op(&mut self, op: &str, c: &Ty, a: Atom, b: Atom, at: Span) -> Atom {
        let p = format!("{op}_{}", c.scalar().expect("an integer"));
        let rhs = Rhs::Call(call(&p, vec![], vec![let_arg(a), let_arg(b)], at));
        self.tmp(c, rhs, at)
    }

    fn bits_lit(v: u128, at: Span) -> Atom {
        Atom::Lit(Literal::Int(v.to_string()), at)
    }

    fn bits_conv(&mut self, x: Atom, from: &Ty, to: &Ty, at: Span) -> Atom {
        let (f, t) = (from.scalar().expect("int"), to.scalar().expect("int"));
        if f == t {
            return x;
        }
        let rhs = Rhs::Call(call(
            &format!("wrap_as_{f}_{t}"),
            vec![],
            vec![let_arg(x)],
            at,
        ));
        self.tmp(to, rhs, at)
    }

    fn bits_place(mut p: Place, at: Span) -> Place {
        p.projs.push(Proj::Field(FIELD.into(), at));
        p
    }

    /// `p.f`: the container shifted down, narrowed to the field type.
    pub(super) fn bits_read(
        &mut self,
        p: Place,
        r: &str,
        f: &str,
        fsp: Span,
        at: Span,
    ) -> R<(Rhs, Ty)> {
        let (c, off, ft) = self.bits_slot(r, f, fsp)?;
        if ft == Ty::Err {
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
        }
        let whole = self.tmp(&c, Rhs::Proj(Self::bits_place(p, at)), at);
        let down = self.bits_op("shr", &c, whole, Self::bits_lit(off.into(), at), at);
        let narrowed = self.bits_conv(down, &c, &ft, at);
        Ok((Rhs::Atom(narrowed), ft))
    }

    /// `field` as a container-wide value at its offset, zero elsewhere.
    fn bits_place_field(&mut self, v: Atom, ft: &Ty, c: &Ty, off: u32, at: Span) -> Atom {
        let wide = self.bits_conv(v, ft, c, at);
        let w = width(ft);
        let masked = if w == width(c) {
            wide
        } else {
            self.bits_op("and", c, wide, Self::bits_lit((1u128 << w) - 1, at), at)
        };
        self.bits_op(
            "wrapping_shl",
            c,
            masked,
            Self::bits_lit(off.into(), at),
            at,
        )
    }

    /// `Record(f=v, ...)`: every field packed and or-ed, `vals` in declaration order.
    pub(super) fn bits_ctor(&mut self, r: &str, vals: Vec<Atom>, at: Span) -> R<Rhs> {
        let n = self.g.layout_bits[r];
        let Some(c) = container(n) else {
            return unsupported(format!("`@layout(bits(u{n}))`"));
        };
        let tys: Vec<Ty> = self.g.records[r].iter().map(|f| f.ty.clone()).collect();
        let mut acc = Self::bits_lit(0, at);
        let mut off = 0;
        for (v, ft) in vals.into_iter().zip(tys) {
            let w = width(&ft);
            let shifted = self.bits_place_field(v, &ft, &c, off, at);
            acc = self.bits_op("or", &c, acc, shifted, at);
            off += w;
        }
        Ok(Rhs::Ctor(y::Ctor::Named {
            path: vec![r.to_owned()],
            targs: Vec::new(),
            fields: vec![(FIELD.into(), acc)],
        }))
    }

    /// `p.f = rhs`: clear the field's bits, or in the new value.
    pub(super) fn bits_write(
        &mut self,
        (p, r): (Place, &str),
        (f, fsp): (&str, Span),
        (rhs, t): (Rhs, &Ty),
        at: Span,
    ) -> R<()> {
        let (c, off, ft) = self.bits_slot(r, f, fsp)?;
        if ft == Ty::Err {
            return Ok(());
        }
        self.expect(t, &ft, at);
        let v = self.tmp(t, rhs, at);
        let whole = self.tmp(&c, Rhs::Proj(Self::bits_place(p.clone(), at)), at);
        let keep = !(((1u128 << width(&ft)) - 1) << off) & ((1u128 << width(&c)) - 1);
        let kept = self.bits_op("and", &c, whole, Self::bits_lit(keep, at), at);
        let put = self.bits_place_field(v, &ft, &c, off, at);
        let new = self.bits_op("or", &c, kept, put, at);
        self.emit(
            StmtKind::Assign {
                place: Self::bits_place(p, at),
                rhs: Rhs::Atom(new),
                rhs_span: at,
            },
            at,
        );
        Ok(())
    }
}
