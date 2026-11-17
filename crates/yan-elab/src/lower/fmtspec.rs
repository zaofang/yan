use super::*;

/// A format spec `[[fill]align][sign][0][width][,][.precision][type]` (SQ20).
pub(super) struct Spec {
    pub fill: char,
    /// 0 left, 1 right, 2 center, 3 after the sign.
    pub align: u8,
    /// 0 none, 1 `+`, 2 space.
    pub sign: u8,
    pub width: usize,
    pub group: bool,
    pub precision: Option<usize>,
    pub fixed: bool,
}

impl Spec {
    pub fn parse(spec: &str, t: &Ty) -> R<Spec> {
        let bad = || unsupported(format!("format specification `{spec}`"));
        let cs: Vec<char> = spec.chars().collect();
        let align_of = |c: char| match c {
            '<' => Some(0),
            '>' => Some(1),
            '^' => Some(2),
            '=' => Some(3),
            _ => None,
        };
        let (mut fill, mut align, mut rest) = match cs.as_slice() {
            [f, a, rest @ ..] if align_of(*a).is_some() => (*f, align_of(*a), rest),
            [a, rest @ ..] if align_of(*a).is_some() => (' ', align_of(*a), rest),
            rest => (' ', None, rest),
        };
        let numeric = matches!(t, Ty::Int | Ty::Float | Ty::BigInt | Ty::Fixed(_));
        let mut sign = 0;
        if let [c @ ('+' | ' ' | '-'), tail @ ..] = rest {
            if !numeric {
                return bad();
            }
            sign = match c {
                '+' => 1,
                ' ' => 2,
                _ => 0,
            };
            rest = tail;
        }
        if let ['0', tail @ ..] = rest {
            if !numeric {
                return bad();
            }
            if align.is_none() {
                fill = '0';
                align = Some(3);
            }
            rest = tail;
        }
        let number = |rest: &mut &[char]| {
            let n = rest.iter().take_while(|c| c.is_ascii_digit()).count();
            let v: String = rest[..n].iter().collect();
            *rest = &rest[n..];
            v.parse::<usize>().ok()
        };
        let width = number(&mut rest).unwrap_or(0);
        let group = matches!(rest, [',', ..]);
        if group {
            rest = &rest[1..];
        }
        let precision = match rest {
            ['.', tail @ ..] => {
                rest = tail;
                match number(&mut rest) {
                    Some(p) => Some(p),
                    None => return bad(),
                }
            }
            _ => None,
        };
        let fixed = match rest {
            [] => false,
            ['f' | 'F'] => true,
            ['d'] if *t == Ty::Int => false,
            ['s'] if *t == Ty::Str => false,
            _ => return bad(),
        };
        // Python's `g` is what a precision without a type means; a precision on an `int` or `str`
        // is an error there. Zero fill with grouping also groups the zeros.
        if (precision.is_some() && !fixed) || (fixed && *t != Ty::Float) || (group && fill == '0') {
            return bad();
        }
        if group && !numeric {
            return bad();
        }
        Ok(Spec {
            fill,
            align: align.unwrap_or(u8::from(numeric)),
            sign,
            width,
            group,
            precision,
            fixed,
        })
    }
}

impl Lower<'_, '_, '_> {
    /// The text of `a` under `spec`: its digits, then the sign, grouping and padding options.
    pub(super) fn spec_text(&mut self, a: Atom, t: &Ty, spec: &Spec, at: Span) -> Atom {
        let mut shown = if spec.fixed {
            let prec = spec.precision.unwrap_or(6);
            let args = vec![
                let_arg(a),
                let_arg(Atom::Lit(Literal::Int(prec.to_string()), at)),
            ];
            let c = call("std.fmt.show_fixed", vec![], args, at);
            self.tmp(&Ty::Str, Rhs::Call(c), at)
        } else {
            self.show(a, t, at)
        };
        if spec.group {
            let c = call("fmt_group", vec![], vec![let_arg(shown)], at);
            shown = self.tmp(&Ty::Str, Rhs::Call(c), at);
        }
        if spec.sign != 0 {
            let mode = Atom::Lit(Literal::Int(spec.sign.to_string()), at);
            let c = call("fmt_sign", vec![], vec![let_arg(shown), let_arg(mode)], at);
            shown = self.tmp(&Ty::Str, Rhs::Call(c), at);
        }
        let args = vec![
            let_arg(shown),
            let_arg(Atom::Lit(Literal::Int(spec.width.to_string()), at)),
            let_arg(Atom::Lit(Literal::Int(spec.align.to_string()), at)),
            let_arg(Atom::Lit(Literal::Str(spec.fill.to_string()), at)),
        ];
        let c = call("fmt_pad", vec![], args, at);
        self.tmp(&Ty::Str, Rhs::Call(c), at)
    }
}
