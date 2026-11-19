//! Prints the Yan Core AST of each file named on the command line as S-expressions, in the format
//! that `selfhost/s0/core_parse.yn` prints, so the two parsers can be compared byte for byte (#1505).
//! Spans print as `line:col-end_line:end_col`; `None` prints `~`, `Some(x)` as `(some x)`.

use yan_core::ast::*;
use yan_core::parse;
use yan_diag::Span;

fn q(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn sp(s: &Span) -> String {
    format!("{}:{}-{}:{}", s.line, s.col, s.end_line, s.end_col)
}

fn b(v: bool) -> &'static str {
    if v { "T" } else { "F" }
}

fn list<T>(v: &[T], f: impl Fn(&T) -> String) -> String {
    format!("[{}]", v.iter().map(f).collect::<Vec<_>>().join(" "))
}

fn opt<T>(v: &Option<T>, f: impl Fn(&T) -> String) -> String {
    match v {
        None => "~".into(),
        Some(x) => format!("(some {})", f(x)),
    }
}

fn node(tag: &str, parts: &[String]) -> String {
    if parts.is_empty() {
        tag.into()
    } else {
        format!("({tag} {})", parts.join(" "))
    }
}

fn names(v: &[Name]) -> String {
    list(v, |n| n.clone())
}

fn path(p: &Path) -> String {
    p.join(".")
}

fn vis(v: Vis) -> String {
    match v {
        Vis::Private => "private",
        Vis::Pkg => "pkg",
        Vis::Pub => "pub",
    }
    .into()
}

fn conv(c: Conv) -> String {
    match c {
        Conv::Let => "let",
        Conv::Inout => "inout",
        Conv::Sink => "sink",
    }
    .into()
}

fn lit(l: &Literal) -> String {
    match l {
        Literal::Int(s) => node("int", std::slice::from_ref(s)),
        Literal::Float(s) => node("float", std::slice::from_ref(s)),
        Literal::Str(s) => node("str", &[q(s)]),
        Literal::Char(c) => node("char", &[q(&c.to_string())]),
        Literal::Bool(v) => node("bool", &[b(*v).into()]),
        Literal::Unit => "unit".into(),
    }
}

fn atom(a: &Atom) -> String {
    match a {
        Atom::Name(n, s) => node("name", &[n.clone(), sp(s)]),
        Atom::Lit(l, s) => node("lit", &[lit(l), sp(s)]),
    }
}

fn call(c: &Call) -> String {
    node(
        "call",
        &[
            path(&c.path),
            sp(&c.span),
            list(&c.targs, targ),
            list(&c.args, arg),
        ],
    )
}

fn expr(e: &Expr) -> String {
    match e {
        Expr::Atom(a) => atom(a),
        Expr::Call(c) => call(c),
    }
}

fn arg(a: &Arg) -> String {
    match a {
        Arg::Value(c, x, s) => node("arg", &[conv(*c), atom(x), sp(s)]),
        Arg::Cap(n, s) => node("cap", &[n.clone(), sp(s)]),
    }
}

fn targ(t: &TArg) -> String {
    match t {
        TArg::Type(t) => ty(t),
        TArg::Nat(n) => node("nat", std::slice::from_ref(n)),
    }
}

fn named(n: &Named) -> String {
    node("named", &[path(&n.path), list(&n.args, targ)])
}

fn capset(c: &CapSet) -> String {
    match c {
        CapSet::None => "none".into(),
        CapSet::Names(v) => node("names", &[names(v)]),
    }
}

fn fn_type(f: &FnType) -> String {
    node(
        "fntype",
        &[
            list(&f.params, |(c, t)| format!("({} {})", conv(*c), ty(t))),
            ty(&f.ret),
            opt(&f.uses, capset),
            opt(&f.fail, |t| ty(t)),
        ],
    )
}

fn ty(t: &Type) -> String {
    match t {
        Type::Named(n) => named(n),
        Type::Tuple(v) => node("tuple", &[list(v, ty)]),
        Type::Dyn(n) => node("dyn", &[named(n)]),
        Type::Fn(f) => fn_type(f),
        Type::Refined { name, ty: t, pred } => node("refined", &[name.clone(), ty(t), expr(pred)]),
        Type::Union(v) => node("union", &[list(v, ty)]),
    }
}

fn const_val(c: &ConstVal) -> String {
    match c {
        ConstVal::Lit(l) => lit(l),
        ConstVal::Path { path: p, fields } => node(
            "cpath",
            &[
                path(p),
                opt(fields, |f| {
                    list(f, |(n, v)| format!("({n} {})", const_val(v)))
                }),
            ],
        ),
        ConstVal::List(v) => node("clist", &[list(v, const_val)]),
        ConstVal::Tuple(v) => node("ctuple", &[list(v, const_val)]),
        ConstVal::Call(p, a) => node("ccall", &[path(p), list(a, attr_arg)]),
    }
}

fn attr_arg(a: &AttrArg) -> String {
    node(
        "attrarg",
        &[opt(&a.name, |n| n.clone()), const_val(&a.value)],
    )
}

fn attr(a: &Attr) -> String {
    node(
        "attr",
        &[a.name.clone(), opt(&a.args, |v| list(v, attr_arg))],
    )
}

fn tparam(t: &TParam) -> String {
    node(
        "tparam",
        &[
            b(t.comptime).into(),
            t.name.clone(),
            format!("{:?}", t.kind).to_lowercase(),
            list(&t.bounds, path),
        ],
    )
}

fn param(p: &Param) -> String {
    match p {
        Param::Value {
            comptime,
            conv: c,
            name,
            ty: t,
        } => node(
            "param",
            &[b(*comptime).into(), conv(*c), name.clone(), ty(t)],
        ),
        Param::Cap { name, ty: t } => node("capparam", &[name.clone(), named(t)]),
    }
}

fn contract(c: &Contract) -> String {
    match c {
        Contract::Requires(e) => node("requires", &[expr(e)]),
        Contract::Ensures(e) => node("ensures", &[expr(e)]),
        Contract::Decreases(v) => node("decreases", &[list(v, expr)]),
        Contract::Example(e) => node("example", &[expr(e)]),
        Contract::Cost { steps, alloc } => node("cost", &[opt(steps, expr), opt(alloc, expr)]),
    }
}

fn fn_def(f: &FnDef) -> String {
    node(
        "fndef",
        &[
            vis(f.vis),
            b(f.is_unsafe).into(),
            path(&f.path),
            sp(&f.name_span),
            list(&f.tparams, tparam),
            list(&f.params, param),
            ty(&f.ret),
            names(&f.from),
            opt(&f.fail, ty),
            list(&f.contracts, contract),
            opt(&f.body, block),
        ],
    )
}

fn field(f: &Field) -> String {
    node(
        "field",
        &[
            list(&f.attrs, attr),
            vis(f.vis),
            f.name.clone(),
            ty(&f.ty),
            opt(&f.default, const_val),
        ],
    )
}

fn xparam(p: &XParam) -> String {
    match p {
        XParam::Value { name, ty: t } => node("xvalue", &[name.clone(), ty(t)]),
        XParam::Callback {
            name,
            escaping,
            ty: t,
        } => node(
            "xcallback",
            &[name.clone(), b(*escaping).into(), fn_type(t)],
        ),
    }
}

fn xitem(x: &XItem) -> String {
    match x {
        XItem::Fn {
            attrs,
            safe,
            name,
            params,
            variadic,
            ret,
            from,
            uses_none,
            nonblocking,
        } => node(
            "xfn",
            &[
                list(attrs, attr),
                b(*safe).into(),
                name.clone(),
                list(params, xparam),
                b(*variadic).into(),
                ty(ret),
                names(from),
                b(*uses_none).into(),
                b(*nonblocking).into(),
            ],
        ),
        XItem::Type { name, drop } => node("xtype", &[name.clone(), path(drop)]),
        XItem::Static { safe, name, ty: t } => {
            node("xstatic", &[b(*safe).into(), name.clone(), ty(t)])
        }
    }
}

fn item(i: &Item) -> String {
    let kind = match &i.kind {
        ItemKind::Fn(f) => node("fn", &[fn_def(f)]),
        ItemKind::Gen(f) => node("gen", &[fn_def(f)]),
        ItemKind::Inst(x) => node("inst", &[list(&x.args, targ), fn_def(&x.def)]),
        ItemKind::Record(r) => node(
            "record",
            &[
                vis(r.vis),
                path(&r.path),
                list(&r.tparams, tparam),
                list(&r.fields, field),
            ],
        ),
        ItemKind::Sum(s) => node(
            "sum",
            &[
                vis(s.vis),
                path(&s.path),
                list(&s.tparams, tparam),
                list(&s.variants, |v| {
                    node(
                        "variant",
                        &[v.name.clone(), opt(&v.fields, |f| list(f, field))],
                    )
                }),
            ],
        ),
        ItemKind::Newtype(n) => node(
            "newtype",
            &[
                vis(n.vis),
                path(&n.path),
                list(&n.tparams, tparam),
                ty(&n.ty),
                opt(&n.pred, expr),
            ],
        ),
        ItemKind::Trait(t) => node(
            "trait",
            &[
                vis(t.vis),
                path(&t.path),
                list(&t.tparams, tparam),
                names(&t.assoc),
                list(&t.sigs, fn_def),
                opt(&t.derived, block),
            ],
        ),
        ItemKind::Impl(i) => node(
            "impl",
            &[
                b(i.is_unsafe).into(),
                named(&i.trait_),
                ty(&i.for_ty),
                list(&i.assoc, |(n, t)| format!("({n} {})", ty(t))),
                list(&i.fns, fn_def),
            ],
        ),
        ItemKind::Const(c) => node(
            "const",
            &[vis(c.vis), path(&c.path), ty(&c.ty), const_val(&c.value)],
        ),
        ItemKind::Test(t) => node("test", &[path(&t.path), block(&t.body)]),
        ItemKind::Extern(e) => node(
            "extern",
            &[
                q(&e.abi),
                e.lib.clone(),
                opt(&e.link, |s| q(s)),
                list(&e.items, xitem),
            ],
        ),
    };
    node("item", &[list(&i.attrs, attr), kind, sp(&i.span)])
}

fn block(b: &Block) -> String {
    list(&b.stmts, stmt)
}

fn place(p: &Place) -> String {
    node(
        "place",
        &[
            p.root.clone(),
            sp(&p.root_span),
            list(&p.projs, |x| match x {
                Proj::Field(n, s) => node("field", &[n.clone(), sp(s)]),
                Proj::Index(a) => node("index", &[atom(a)]),
            }),
        ],
    )
}

fn ctor(c: &Ctor) -> String {
    match c {
        Ctor::Named {
            path: p,
            targs,
            fields,
        } => node(
            "ctor",
            &[
                path(p),
                list(targs, targ),
                list(fields, |(n, a)| format!("({n} {})", atom(a))),
            ],
        ),
        Ctor::Tuple(v) => node("tupctor", &[list(v, atom)]),
    }
}

fn rhs(r: &Rhs) -> String {
    match r {
        Rhs::Atom(a) => atom(a),
        Rhs::Call(c) => call(c),
        Rhs::Propagate {
            call: c,
            via,
            drops,
        } => node("propagate", &[call(c), opt(via, path), names(drops)]),
        Rhs::Try(c) => node("try", &[call(c)]),
        Rhs::Rest { root, moved } => node("rest", &[root.clone(), list(moved, |m| names(m))]),
        Rhs::Ctor(c) => ctor(c),
        Rhs::Proj(p) => node("proj", &[place(p)]),
        Rhs::Copy(p) => node("copy", &[place(p)]),
        Rhs::Move(p) => node("move", &[place(p)]),
        Rhs::Closure(c) => node(
            "closure",
            &[
                list(&c.captures, |(cv, n, s)| {
                    format!("({} {n} {})", conv(*cv), sp(s))
                }),
                list(&c.params, param),
                ty(&c.ret),
                opt(&c.uses, capset),
                opt(&c.fail, ty),
                block(&c.body),
            ],
        ),
    }
}

fn pat(p: &Pat) -> String {
    match p {
        Pat::Wild => "wild".into(),
        Pat::Lit(l) => node("plit", &[lit(l)]),
        Pat::Bind(n) => node("bind", std::slice::from_ref(n)),
        Pat::Ctor { path: p, fields } => node(
            "pctor",
            &[
                path(p),
                opt(fields, |(f, rest)| {
                    format!(
                        "({} {})",
                        list(f, |(n, x)| format!("({n} {})", pat(x))),
                        b(*rest)
                    )
                }),
            ],
        ),
        Pat::Tuple(v) => node("ptuple", &[list(v, pat)]),
        Pat::Or(v) => node("por", &[list(v, pat)]),
        Pat::Range(a, c) => node("prange", &[lit(a), lit(c)]),
        Pat::Slice { elems, rest } => node(
            "pslice",
            &[list(elems, pat), opt(rest, |r| opt(r, |n| n.clone()))],
        ),
    }
}

fn asm_operand(o: &AsmOperand) -> String {
    match o {
        AsmOperand::Reg { dir, reg, value } => node(
            "reg",
            &[
                format!("{dir:?}").to_lowercase(),
                match reg {
                    AsmReg::Class(n) => node("class", std::slice::from_ref(n)),
                    AsmReg::Explicit(s) => node("explicit", &[q(s)]),
                },
                atom(value),
            ],
        ),
        AsmOperand::ClobberAbi(s) => node("clobber_abi", &[q(s)]),
        AsmOperand::Options(v) => node("options", &[names(v)]),
    }
}

fn stmt(s: &Stmt) -> String {
    let k = match &s.kind {
        StmtKind::Let {
            mutable,
            pat: p,
            pat_span,
            ty: t,
            rhs: r,
            rhs_span,
        } => node(
            "let",
            &[
                b(*mutable).into(),
                pat(p),
                sp(pat_span),
                ty(t),
                rhs(r),
                sp(rhs_span),
            ],
        ),
        StmtKind::Assign {
            place: p,
            rhs: r,
            rhs_span,
        } => node("assign", &[place(p), rhs(r), sp(rhs_span)]),
        StmtKind::If { cond, then, els } => node("if", &[atom(cond), block(then), opt(els, block)]),
        StmtKind::Match { scrutinee, arms } => node(
            "match",
            &[
                atom(scrutinee),
                list(arms, |(p, bl)| format!("({} {})", pat(p), block(bl))),
            ],
        ),
        StmtKind::Loop {
            label,
            invariants,
            decreases,
            body,
        } => node(
            "loop",
            &[
                label.clone(),
                list(invariants, expr),
                list(decreases, expr),
                block(body),
            ],
        ),
        StmtKind::Break { label, value } => node("break", &[label.clone(), opt(value, atom)]),
        StmtKind::Continue { label } => node("continue", std::slice::from_ref(label)),
        StmtKind::Jp { name, params, body } => {
            node("jp", &[name.clone(), list(params, param), block(body)])
        }
        StmtKind::Jump { name, args } => node("jump", &[name.clone(), list(args, atom)]),
        StmtKind::Ret(a) => node("ret", &[atom(a)]),
        StmtKind::Fail(a) => node("fail", &[atom(a)]),
        StmtKind::Yield(a) => node("yield", &[atom(a)]),
        StmtKind::Become(c) => node("become", &[call(c)]),
        StmtKind::With {
            name,
            dyn_,
            handler,
            body,
        } => node(
            "with",
            &[name.clone(), b(*dyn_).into(), atom(handler), block(body)],
        ),
        StmtKind::Deadline { at, body } => node("deadline", &[atom(at), block(body)]),
        StmtKind::Par { concurrent, blocks } => {
            node("par", &[b(*concurrent).into(), list(blocks, block)])
        }
        StmtKind::ComptimeFor {
            name,
            ty: t,
            iter,
            body,
        } => node(
            "comptime_for",
            &[name.clone(), ty(t), atom(iter), block(body)],
        ),
        StmtKind::ComptimeIf { cond, then, els } => {
            node("comptime_if", &[atom(cond), block(then), opt(els, block)])
        }
        StmtKind::Assert(e) => node("assert", &[expr(e)]),
        StmtKind::Unreachable => "unreachable".into(),
        StmtKind::Unsafe(bl) => node("unsafe", &[block(bl)]),
        StmtKind::Asm { template, operands } => {
            node("asm", &[q(template), list(operands, asm_operand)])
        }
        StmtKind::Drop(n) => node("drop", std::slice::from_ref(n)),
        StmtKind::Reuse { name, ctor: c } => node("reuse", &[name.clone(), ctor(c)]),
        StmtKind::Rhs(r) => node("rhs", &[rhs(r)]),
    };
    node("stmt", &[k, sp(&s.span)])
}

fn main() {
    for p in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&p).expect("readable file");
        println!("file {p}");
        let (module, diags) = parse(&p, &src);
        for d in &diags {
            println!(
                "diag {} {} {} {} {}",
                d.code,
                sp(&d.span),
                q(&d.item),
                q(&d.quote),
                d.message
            );
        }
        if let Some(m) = module {
            for h in &m.header {
                println!("header {}", q(h));
            }
            println!("module {}", path(&m.path));
            for i in &m.items {
                println!("{}", item(i));
            }
        }
    }
}
