//! std.session (9.5): `session.protocol(spec)` parses a Scribble-style global protocol at
//! stage 0 and projects it onto each role (Honda, Yoshida and Carbone, POPL 2008), merging the
//! branches of a choice a role does not make (R633, E728). Each projection becomes a finite
//! state machine whose states are the endpoint types of R634.

use std::collections::HashMap;

use yan_diag::Span;
use yan_hir as j;

use crate::Cx;

/// Byte offsets into the protocol string.
type At = (usize, usize);

#[derive(Clone, Debug, PartialEq)]
enum G {
    Msg {
        from: usize,
        to: usize,
        label: String,
        payload: Option<j::Type>,
        next: Box<G>,
    },
    Choice {
        by: usize,
        at: At,
        branches: Vec<(At, G)>,
    },
    Rec(String, Box<G>),
    Var(String),
    End,
}

/// A step of a sequence, before the sequence is folded into a [`G`].
enum Step {
    Msg(usize, usize, String, Option<j::Type>),
    Choice(usize, At, Vec<(At, G)>),
    Rec(String, G),
    Var(String),
    End,
}

/// A projection onto one role.
#[derive(Clone, Debug, PartialEq)]
enum L {
    /// An internal choice, or one send: (to, label, payload, continuation).
    Send(Vec<(usize, String, Option<j::Type>, L)>),
    /// An external choice, or one receive, from one peer.
    Recv(usize, Vec<(String, Option<j::Type>, L)>),
    Rec(String, Box<L>),
    Var(String),
    End,
}

/// A state of a role's endpoint; state 0 is the initial one.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum State {
    /// The labels this state may send: (to, label, payload, next state).
    Send(Vec<(usize, String, Option<j::Type>, usize)>),
    /// The labels it may receive from one peer: (label, payload, next state).
    Recv(usize, Vec<(String, Option<j::Type>, usize)>),
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Protocol {
    /// The name its first alias gives it, which its generated types start with.
    pub name: String,
    pub roles: Vec<String>,
    /// Each role's state machine.
    pub states: Vec<Vec<State>>,
}

type Fail = (String, At, Vec<(At, String)>);

struct Parser<'a> {
    s: &'a str,
    i: usize,
    roles: Vec<String>,
    recs: Vec<String>,
    yn: bool,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s[self.i..].starts_with(char::is_whitespace) {
            self.i += self.s[self.i..].chars().next().map_or(1, char::len_utf8);
        }
    }

    fn here(&self) -> At {
        (self.i, (self.i + 1).min(self.s.len()))
    }

    fn fail<T>(&self, what: impl Into<String>) -> Result<T, Fail> {
        Err((what.into(), self.here(), Vec::new()))
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        let word = tok.starts_with(char::is_alphabetic);
        let rest = &self.s[self.i..];
        let ok = rest.starts_with(tok)
            && !(word && rest[tok.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_'));
        if ok {
            self.i += tok.len();
        }
        ok
    }

    fn expect(&mut self, tok: &str) -> Result<(), Fail> {
        if self.eat(tok) {
            Ok(())
        } else {
            self.fail(format!("expected `{tok}`"))
        }
    }

    fn ident(&mut self) -> Result<(String, At), Fail> {
        self.ws();
        let start = self.i;
        let rest = &self.s[start..];
        if !rest.starts_with(|c: char| c.is_alphabetic() || c == '_') {
            return self.fail("expected a name");
        }
        let n = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        self.i += n;
        Ok((rest[..n].to_owned(), (start, start + n)))
    }

    fn role(&mut self, name: String) -> usize {
        match self.roles.iter().position(|r| *r == name) {
            Some(k) => k,
            None => {
                self.roles.push(name);
                self.roles.len() - 1
            }
        }
    }

    /// Steps separated by `;`, up to `|`, `}` or the end of the string.
    fn seq(&mut self) -> Result<G, Fail> {
        let mut steps = Vec::new();
        loop {
            self.ws();
            if self.i == self.s.len() || self.s[self.i..].starts_with(['|', '}']) {
                break;
            }
            if matches!(steps.last(), Some((Step::End | Step::Var(_), _))) {
                return self.fail("nothing can follow `end` or a recursion");
            }
            let at = self.here();
            steps.push((self.step()?, at));
            if !self.eat(";") {
                break;
            }
        }
        let mut g = G::End;
        for (step, _) in steps.into_iter().rev() {
            g = match step {
                Step::Msg(from, to, label, payload) => G::Msg {
                    from,
                    to,
                    label,
                    payload,
                    next: Box::new(g),
                },
                Step::Choice(by, at, branches) => G::Choice {
                    by,
                    at,
                    branches: branches
                        .into_iter()
                        .map(|(at, b)| (at, then(b, &g)))
                        .collect(),
                },
                Step::Rec(x, body) => G::Rec(x, Box::new(then(body, &g))),
                Step::Var(x) => G::Var(x),
                Step::End => G::End,
            };
        }
        Ok(g)
    }

    fn step(&mut self) -> Result<Step, Fail> {
        if self.eat("end") {
            return Ok(Step::End);
        }
        let start = self.i;
        if self.eat("choice") {
            let (by, _) = self.ident()?;
            let by = self.role(by);
            self.expect("{")?;
            let mut branches = Vec::new();
            loop {
                self.ws();
                let from = self.i;
                let b = self.seq()?;
                branches.push(((from, self.i), b));
                if !self.eat("|") {
                    break;
                }
            }
            self.expect("}")?;
            if branches.len() < 2 {
                return self.fail("a choice has two or more branches, separated by `|`");
            }
            return Ok(Step::Choice(by, (start, self.i), branches));
        }
        if self.eat("rec") {
            let (x, _) = self.ident()?;
            self.expect("{")?;
            self.recs.push(x.clone());
            let body = self.seq()?;
            self.recs.pop();
            self.expect("}")?;
            if !guarded(&body) {
                return Err((
                    format!("`rec {x}` repeats without a message"),
                    (start, self.i),
                    Vec::new(),
                ));
            }
            return Ok(Step::Rec(x, body));
        }
        let keyword = self.eat("continue");
        let (name, at) = self.ident()?;
        if keyword || self.recs.contains(&name) && !self.s[self.i..].trim_start().starts_with("->")
        {
            if !self.recs.contains(&name) {
                return Err((format!("no `rec {name}` encloses this"), at, Vec::new()));
            }
            return Ok(Step::Var(name));
        }
        let from = self.role(name);
        self.expect("->")?;
        let (to, to_at) = self.ident()?;
        let to = self.role(to);
        if to == from {
            return Err((
                format!("`{}` sends to itself", self.roles[from]),
                to_at,
                Vec::new(),
            ));
        }
        self.expect(":")?;
        let (label, _) = self.ident()?;
        self.expect("(")?;
        self.ws();
        let payload = if self.eat(")") {
            None
        } else {
            let t = self.ty()?;
            self.expect(")")?;
            Some(t)
        };
        Ok(Step::Msg(from, to, label, payload))
    }

    /// A payload type, `int` or `list[tuple[str, int]]`, as the module's surface spells it.
    fn ty(&mut self) -> Result<j::Type, Fail> {
        let (first, _) = self.ident()?;
        let mut path = vec![first];
        while self.eat(".") {
            path.push(self.ident()?.0);
        }
        let mut args = Vec::new();
        if self.eat("[") {
            loop {
                args.push(j::TArg::Type(self.ty()?));
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("]")?;
        }
        if let [n] = path.as_mut_slice() {
            let surface = if self.yn {
                yan_names::Surface::Yn
            } else {
                yan_names::Surface::Yag
            };
            if let Some(core) = yan_names::type_name(n, surface, yan_names::Surface::Yir) {
                *n = core.to_owned();
            }
        }
        Ok(j::Type::Named { path, args })
    }
}

/// `g` with `k` in place of each `end`.
fn then(g: G, k: &G) -> G {
    match g {
        G::End => k.clone(),
        G::Msg {
            from,
            to,
            label,
            payload,
            next,
        } => G::Msg {
            from,
            to,
            label,
            payload,
            next: Box::new(then(*next, k)),
        },
        G::Choice { by, at, branches } => G::Choice {
            by,
            at,
            branches: branches
                .into_iter()
                .map(|(at, b)| (at, then(b, k)))
                .collect(),
        },
        G::Rec(x, body) => G::Rec(x, Box::new(then(*body, k))),
        G::Var(x) => G::Var(x),
    }
}

/// Every path through `g` sends a message before it repeats.
fn guarded(g: &G) -> bool {
    match g {
        G::Msg { .. } | G::End => true,
        G::Choice { branches, .. } => branches.iter().all(|(_, b)| guarded(b)),
        G::Rec(_, body) => guarded(body),
        G::Var(_) => false,
    }
}

fn takes_part(g: &G, r: usize) -> bool {
    match g {
        G::Msg { from, to, next, .. } => *from == r || *to == r || takes_part(next, r),
        G::Choice { by, branches, .. } => {
            *by == r || branches.iter().any(|(_, b)| takes_part(b, r))
        }
        G::Rec(_, body) => takes_part(body, r),
        G::Var(_) | G::End => false,
    }
}

/// The first messages of `g` that `by` sends, or none when `g` starts otherwise.
fn first_sends(g: &G, by: usize, out: &mut Vec<String>) -> bool {
    match g {
        G::Msg { from, label, .. } if *from == by => {
            out.push(label.clone());
            true
        }
        G::Choice {
            by: b, branches, ..
        } if *b == by => branches.iter().all(|(_, g)| first_sends(g, by, out)),
        G::Rec(_, body) => first_sends(body, by, out),
        _ => false,
    }
}

/// The choices in `g` are each made by their role: every branch starts with a message the
/// role sends, and no two branches start with the same label.
fn check_choices(g: &G, roles: &[String]) -> Result<(), Fail> {
    match g {
        G::Msg { next, .. } => check_choices(next, roles),
        G::Choice { by, at, branches } => {
            let mut seen: Vec<(String, At)> = Vec::new();
            for (bat, b) in branches {
                let mut labels = Vec::new();
                if !first_sends(b, *by, &mut labels) {
                    return Err((
                        format!(
                            "this branch of `choice {r}` does not start with a message `{r}` sends, so `{r}` cannot make the choice",
                            r = roles[*by]
                        ),
                        *bat,
                        vec![(*at, "the choice".into())],
                    ));
                }
                for l in labels {
                    if let Some((_, first)) = seen.iter().find(|(s, _)| *s == l) {
                        return Err((
                            format!(
                                "two branches of `choice {}` start with `{l}`, so its receiver cannot tell them apart",
                                roles[*by]
                            ),
                            *bat,
                            vec![(*first, "the other branch".into())],
                        ));
                    }
                    seen.push((l, *bat));
                }
                check_choices(b, roles)?;
            }
            Ok(())
        }
        G::Rec(_, body) => check_choices(body, roles),
        G::Var(_) | G::End => Ok(()),
    }
}

/// The projection of `g` onto role `r`.
fn project(g: &G, r: usize, roles: &[String]) -> Result<L, Fail> {
    Ok(match g {
        G::Msg {
            from,
            to,
            label,
            payload,
            next,
        } => {
            let k = project(next, r, roles)?;
            if *from == r {
                L::Send(vec![(*to, label.clone(), payload.clone(), k)])
            } else if *to == r {
                L::Recv(*from, vec![(label.clone(), payload.clone(), k)])
            } else {
                k
            }
        }
        G::Choice { by, at, branches } => {
            let ls = branches
                .iter()
                .map(|(_, b)| project(b, r, roles))
                .collect::<Result<Vec<_>, _>>()?;
            if *by == r {
                let mut out = Vec::new();
                for l in ls {
                    let L::Send(bs) = l else {
                        unreachable!("check_choices: each branch starts with a send by `by`")
                    };
                    out.extend(bs);
                }
                return Ok(L::Send(out));
            }
            let mut acc = ls[0].clone();
            for (k, l) in ls.iter().enumerate().skip(1) {
                acc = match merge(&acc, l) {
                    Some(m) => m,
                    None => {
                        let i = (0..k).find(|&i| merge(&ls[i], l).is_none()).unwrap_or(0);
                        return Err((
                            format!(
                                "the protocol cannot be projected onto `{r}`: `{r}` cannot see which branch of `choice {by}` was taken, and its part differs between them",
                                r = roles[r],
                                by = roles[*by]
                            ),
                            *at,
                            vec![
                                (branches[i].0, format!("branch {}", i + 1)),
                                (branches[k].0, format!("branch {}", k + 1)),
                            ],
                        ));
                    }
                };
            }
            acc
        }
        G::Rec(x, body) if takes_part(body, r) => {
            L::Rec(x.clone(), Box::new(project(body, r, roles)?))
        }
        G::Rec(..) | G::End => L::End,
        G::Var(x) => L::Var(x.clone()),
    })
}

/// The merge of two projections of the branches of a choice the role does not make: equal
/// ones, or receives from one peer whose shared labels merge (full merge).
fn merge(a: &L, b: &L) -> Option<L> {
    if a == b {
        return Some(a.clone());
    }
    match (a, b) {
        (L::Recv(p, xs), L::Recv(q, ys)) if p == q => {
            let mut out = xs.clone();
            for (l, t, k) in ys {
                match out.iter_mut().find(|(m, _, _)| m == l) {
                    Some((_, u, j)) if u == t => *j = merge(j, k)?,
                    Some(_) => return None,
                    None => out.push((l.clone(), t.clone(), k.clone())),
                }
            }
            Some(L::Recv(*p, out))
        }
        (L::Rec(x, a), L::Rec(y, b)) if x == y => Some(L::Rec(x.clone(), Box::new(merge(a, b)?))),
        _ => None,
    }
}

enum Raw {
    Send(Vec<(usize, String, Option<j::Type>, usize)>),
    Recv(usize, Vec<(String, Option<j::Type>, usize)>),
    /// The state is the one at the index, through a `rec`.
    Same(usize),
    End,
}

fn build(l: &L, env: &mut HashMap<String, usize>, out: &mut Vec<Raw>) -> usize {
    let id = out.len();
    match l {
        L::Var(x) => return env[x],
        L::End => out.push(Raw::End),
        L::Rec(x, body) => {
            out.push(Raw::Same(id));
            let prev = env.insert(x.clone(), id);
            let to = build(body, env, out);
            // A role whose part of the body is only the repeat has nothing left to do.
            out[id] = if to == id { Raw::End } else { Raw::Same(to) };
            match prev {
                Some(p) => env.insert(x.clone(), p),
                None => env.remove(x),
            };
        }
        L::Send(bs) => {
            out.push(Raw::End);
            let bs = bs
                .iter()
                .map(|(to, l, t, k)| (*to, l.clone(), t.clone(), build(k, env, out)))
                .collect();
            out[id] = Raw::Send(bs);
        }
        L::Recv(p, bs) => {
            out.push(Raw::End);
            let bs = bs
                .iter()
                .map(|(l, t, k)| (l.clone(), t.clone(), build(k, env, out)))
                .collect();
            out[id] = Raw::Recv(*p, bs);
        }
    }
    id
}

/// The state machine of `l`: reachable states numbered from the initial one, one `end`.
fn machine(l: &L) -> Vec<State> {
    let mut raw = Vec::new();
    let start = build(l, &mut HashMap::new(), &mut raw);
    let end = raw.iter().position(|r| matches!(r, Raw::End));
    let resolve = |mut s: usize| {
        while let Raw::Same(t) = raw[s] {
            s = t;
        }
        match raw[s] {
            Raw::End => end.expect("an end state exists"),
            _ => s,
        }
    };
    let mut order = vec![resolve(start)];
    let mut index = HashMap::from([(order[0], 0)]);
    let mut k = 0;
    while k < order.len() {
        let nexts: Vec<usize> = match &raw[order[k]] {
            Raw::Send(bs) => bs.iter().map(|b| b.3).collect(),
            Raw::Recv(_, bs) => bs.iter().map(|b| b.2).collect(),
            Raw::Same(_) | Raw::End => Vec::new(),
        };
        for n in nexts {
            let n = resolve(n);
            if let std::collections::hash_map::Entry::Vacant(e) = index.entry(n) {
                e.insert(order.len());
                order.push(n);
            }
        }
        k += 1;
    }
    order
        .iter()
        .map(|s| match &raw[*s] {
            Raw::Send(bs) => State::Send(
                bs.iter()
                    .map(|(to, l, t, n)| (*to, l.clone(), t.clone(), index[&resolve(*n)]))
                    .collect(),
            ),
            Raw::Recv(p, bs) => State::Recv(
                *p,
                bs.iter()
                    .map(|(l, t, n)| (l.clone(), t.clone(), index[&resolve(*n)]))
                    .collect(),
            ),
            Raw::Same(_) | Raw::End => State::End,
        })
        .collect()
}

/// The span of bytes `at` of the string literal `lit` at `span`, when its source text is its
/// value; else the whole literal.
fn inside(cx: &Cx, span: Span, value: &str, at: At) -> Span {
    let src = cx.text(span);
    let exact = span.line == span.end_line
        && src.len() == value.len() + 2
        && src.get(1..src.len() - 1) == Some(value);
    if !exact {
        return span;
    }
    let (a, b) = (at.0.min(value.len()), at.1.min(value.len()).max(at.0 + 1));
    Span {
        line: span.line,
        col: span.col + 1 + a as u32,
        end_line: span.line,
        end_col: span.col + 1 + b as u32,
    }
}

/// `session.protocol(spec)`: the protocol, or none after reporting why there is none (E270 for
/// a malformed protocol, E728 for one that cannot be projected).
pub(crate) fn protocol(name: &str, spec: &str, span: Span, cx: &mut Cx) -> Option<Protocol> {
    let mut p = Parser {
        s: spec,
        i: 0,
        roles: Vec::new(),
        recs: Vec::new(),
        yn: cx.yn,
    };
    let parsed = p.seq().and_then(|g| {
        p.ws();
        if p.i < spec.len() {
            return p.fail("expected `;` or the end of the protocol");
        }
        Ok(g)
    });
    let report = |code: &str, (msg, at, labels): Fail, cx: &mut Cx| {
        let s = inside(cx, span, spec, at);
        cx.err(code, s, name, msg);
        for (at, text) in labels {
            let s = inside(cx, span, spec, at);
            cx.label(s, &text);
        }
    };
    let global = match parsed {
        Ok(g) => g,
        Err(f) => {
            report("E270", f, cx);
            return None;
        }
    };
    let roles = p.roles;
    if roles.len() < 2 {
        report(
            "E270",
            (
                "a protocol has two or more roles".into(),
                (0, spec.len()),
                Vec::new(),
            ),
            cx,
        );
        return None;
    }
    if let Err(f) = check_choices(&global, &roles) {
        report("E728", f, cx);
        return None;
    }
    let mut states = Vec::new();
    for r in 0..roles.len() {
        match project(&global, r, &roles) {
            Ok(l) => states.push(machine(&l)),
            Err(f) => {
                report("E728", f, cx);
                return None;
            }
        }
    }
    Some(Protocol {
        name: name.to_owned(),
        roles,
        states,
    })
}

/// What one endpoint type is: its protocol, role and state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Endpoint {
    pub protocol: String,
    pub role: usize,
    pub state: usize,
}

impl Protocol {
    /// The type of `role`'s endpoint in `state`.
    pub(crate) fn state_type(&self, role: usize, state: usize) -> String {
        match self.states[role][state] {
            State::End => format!("{}_{}_end", self.name, self.roles[role]),
            _ => format!("{}_{}_{state}", self.name, self.roles[role]),
        }
    }

    pub(crate) fn roles_type(&self) -> String {
        format!("{}_roles", self.name)
    }

    pub(crate) fn connect_fn(&self) -> String {
        format!("__{}_connect", self.name)
    }

    pub(crate) fn op_fn(&self, role: usize, state: usize, op: &str) -> String {
        format!("__{}_{op}", self.state_type(role, state))
    }

    /// What a choice state's `recv` returns: a sum with a variant per label.
    pub(crate) fn got_type(&self, role: usize, state: usize) -> String {
        format!("{}_got", self.state_type(role, state))
    }

    /// The endpoint types, by name.
    pub(crate) fn endpoints(&self) -> Vec<(String, Endpoint)> {
        let mut out = Vec::new();
        for (r, states) in self.states.iter().enumerate() {
            for k in 0..states.len() {
                out.push((
                    self.state_type(r, k),
                    Endpoint {
                        protocol: self.name.clone(),
                        role: r,
                        state: k,
                    },
                ));
            }
        }
        out
    }

    /// The peers `r` sends to and receives from.
    fn peers(&self, r: usize) -> (Vec<usize>, Vec<usize>) {
        let (mut to, mut from) = (Vec::new(), Vec::new());
        for s in &self.states[r] {
            match s {
                State::Send(bs) => to.extend(bs.iter().map(|b| b.0)),
                State::Recv(p, _) => from.push(*p),
                State::End => {}
            }
        }
        for v in [&mut to, &mut from] {
            v.sort_unstable();
            v.dedup();
        }
        (to, from)
    }

    /// The labels and payloads that travel from `a` to `b`, each with its variant name in the
    /// direction's message sum.
    fn messages(&self, a: usize, b: usize) -> Vec<(String, Option<j::Type>, String)> {
        let mut out: Vec<(String, Option<j::Type>, String)> = Vec::new();
        for s in &self.states[a] {
            let State::Send(bs) = s else { continue };
            for (to, l, t, _) in bs {
                if *to != b || out.iter().any(|(m, u, _)| m == l && u == t) {
                    continue;
                }
                let same = out.iter().filter(|(m, _, _)| m == l).count();
                let mut v = format!("{}_{l}", self.msg_type(a, b));
                if same > 0 {
                    v.push_str(&format!("_{}", same + 1));
                }
                out.push((l.clone(), t.clone(), v));
            }
        }
        out
    }

    fn msg_type(&self, a: usize, b: usize) -> String {
        format!("{}_{}_to_{}", self.name, self.roles[a], self.roles[b])
    }
}

fn ex(kind: j::ExprKind, at: Span) -> j::Expr {
    j::Expr {
        kind,
        span: at,
        paren: false,
    }
}

fn nm(n: &str, at: Span) -> j::Expr {
    ex(j::ExprKind::Name(n.to_owned()), at)
}

fn arg(name: Option<&str>, e: j::Expr) -> j::Arg {
    j::Arg {
        name: name.map(str::to_owned),
        span: e.span,
        kind: j::ArgKind::Expr(e),
    }
}

fn call(f: j::Expr, args: Vec<j::Arg>) -> j::Expr {
    let at = f.span;
    ex(j::ExprKind::Call(Box::new(f), args), at)
}

fn named(n: &str) -> j::Type {
    j::Type::Named {
        path: vec![n.to_owned()],
        args: Vec::new(),
    }
}

fn st(kind: j::StmtKind, at: Span) -> j::Stmt {
    j::Stmt { kind, span: at }
}

fn field(name: &str, ty: j::Type, at: Span) -> j::Field {
    j::Field {
        decorators: Vec::new(),
        name: name.to_owned(),
        ty,
        ty_span: at,
        default: None,
    }
}

/// `e else fail SessionClosed()`.
fn or_closed(e: j::Expr, at: Span) -> j::Expr {
    let closed = call(nm("SessionClosed", at), Vec::new());
    ex(
        j::ExprKind::Fallback(Box::new(e), Box::new(j::Fallback::Fail(closed))),
        at,
    )
}

/// `case T(f1=b1, ...)`, binding each field to `ep_<field>`, or to `_` when `bind` is false.
fn destructure(t: &str, fields: &[String], bind: bool) -> j::Pat {
    j::Pat::Path {
        path: vec![t.to_owned()],
        args: Some(
            fields
                .iter()
                .map(|f| {
                    let p = if bind {
                        j::Pat::Path {
                            path: vec![format!("ep_{f}")],
                            args: None,
                        }
                    } else {
                        j::Pat::Wild
                    };
                    (Some(f.clone()), p)
                })
                .collect(),
        ),
    }
}

/// The function's single statement: `match ep: case T(...): body`.
fn on_endpoint(
    t: &str,
    fields: &[String],
    bind: bool,
    body: Vec<j::Stmt>,
    at: Span,
) -> Vec<j::Stmt> {
    vec![st(
        j::StmtKind::Match {
            scrutinee: nm("ep", at),
            cases: vec![j::Case {
                pat: destructure(t, fields, bind),
                span: at,
                guard: None,
                body: j::Block {
                    inline: false,
                    stmts: body,
                },
            }],
        },
        at,
    )]
}

/// `T(f1=ep_f1, ...)`, with `ep_<swap.0>` replaced by `swap.1`.
fn rebuild(t: &str, fields: &[String], swap: (&str, &str), at: Span) -> j::Expr {
    call(
        nm(t, at),
        fields
            .iter()
            .map(|f| {
                let v = if f == swap.0 {
                    swap.1.to_owned()
                } else {
                    format!("ep_{f}")
                };
                arg(Some(f), nm(&v, at))
            })
            .collect(),
    )
}

fn func(
    name: String,
    params: Vec<(bool, &str, j::Type)>,
    ret: Option<j::Type>,
    fails: bool,
    stmts: Vec<j::Stmt>,
    at: Span,
) -> j::FuncDef {
    j::FuncDef {
        name,
        name_span: at,
        tparams: Vec::new(),
        params: params
            .into_iter()
            .map(|(take, n, ty)| j::Param::Value {
                conv: take.then_some(j::Conv::Take),
                name: n.to_owned(),
                span: at,
                ty,
                default: None,
            })
            .collect(),
        ret: Some(j::RetType {
            ty: ret,
            fail: if fails {
                vec![vec!["SessionClosed".into()]]
            } else {
                Vec::new()
            },
            from: Vec::new(),
        }),
        uses: None,
        body: Some(j::Body {
            doc: None,
            contracts: Vec::new(),
            stmts,
        }),
    }
}

/// The items that implement `p` (R634), all at `at`: a message sum per direction, a `@linear`
/// class per endpoint state holding the role's channel ends, a sum per choice that `recv` returns,
/// the class `connect` returns, and a function per operation each state allows.
pub(crate) fn items(p: &Protocol, is_pub: bool, at: Span) -> Vec<j::Item> {
    let mut out = Vec::new();
    let item = |kind, linear: bool| j::Item {
        decorators: if linear {
            vec![j::Decorator {
                path: vec!["linear".into()],
                args: None,
            }]
        } else {
            Vec::new()
        },
        is_pub,
        kind,
        span: at,
        name_span: at,
    };
    let n = p.roles.len();
    for a in 0..n {
        for b in 0..n {
            let ms = p.messages(a, b);
            if ms.is_empty() {
                continue;
            }
            let variants = ms
                .into_iter()
                .map(|(_, t, v)| j::Variant {
                    decorators: Vec::new(),
                    name: v,
                    fields: t.map(|t| vec![field("value", t, at)]).unwrap_or_default(),
                })
                .collect();
            out.push(item(
                j::ItemKind::Type(j::TypeDef {
                    name: p.msg_type(a, b),
                    tparams: Vec::new(),
                    rhs: j::TypeRhs::Sum(variants),
                    where_: None,
                }),
                false,
            ));
        }
    }
    let sender = |t: String| j::Type::Named {
        path: vec!["Sender".into()],
        args: vec![j::TArg::Type(named(&t))],
    };
    let receiver = |t: String| j::Type::Named {
        path: vec!["Receiver".into()],
        args: vec![j::TArg::Type(named(&t))],
    };
    let mut connect = Vec::new();
    let mut roles = Vec::new();
    for (r, states) in p.states.iter().enumerate() {
        let (to, from) = p.peers(r);
        let mut fields: Vec<(String, j::Type)> = Vec::new();
        for &b in &to {
            fields.push((format!("to_{}", p.roles[b]), sender(p.msg_type(r, b))));
        }
        for &b in &from {
            fields.push((format!("from_{}", p.roles[b]), receiver(p.msg_type(b, r))));
        }
        let names: Vec<String> = fields.iter().map(|f| f.0.clone()).collect();
        for k in 0..states.len() {
            out.push(item(
                j::ItemKind::Record(j::Record {
                    name: p.state_type(r, k),
                    tparams: Vec::new(),
                    doc: None,
                    fields: fields
                        .iter()
                        .map(|(f, t)| field(f, t.clone(), at))
                        .collect(),
                }),
                true,
            ));
        }
        for (k, s) in states.iter().enumerate() {
            let me = p.state_type(r, k);
            let ep = (true, "ep", named(&me));
            match s {
                State::Send(bs) => {
                    for (to, l, t, next) in bs {
                        let chan = format!("to_{}", p.roles[*to]);
                        let variant = p
                            .messages(r, *to)
                            .into_iter()
                            .find(|(m, u, _)| m == l && u == t)
                            .expect("a message of the direction")
                            .2;
                        let msg = call(
                            nm(&variant, at),
                            t.iter()
                                .map(|_| arg(Some("value"), nm("value", at)))
                                .collect(),
                        );
                        let send = j::ExprKind::Call(
                            Box::new(ex(
                                j::ExprKind::Attr(Box::new(nm("chan", at)), "send".into(), at),
                                at,
                            )),
                            vec![arg(None, msg)],
                        );
                        let body = vec![
                            st(
                                j::StmtKind::Assign {
                                    target: nm("chan", at),
                                    ty: None,
                                    value: nm(&format!("ep_{chan}"), at),
                                },
                                at,
                            ),
                            st(j::StmtKind::Expr(or_closed(ex(send, at), at)), at),
                            st(
                                j::StmtKind::Return(Some(rebuild(
                                    &p.state_type(r, *next),
                                    &names,
                                    (&chan, "chan"),
                                    at,
                                ))),
                                at,
                            ),
                        ];
                        let mut params = vec![ep.clone()];
                        if let Some(t) = t {
                            params.push((false, "value", t.clone()));
                        }
                        out.push(item(
                            j::ItemKind::Func(func(
                                p.op_fn(r, k, &format!("send_{l}")),
                                params,
                                Some(named(&p.state_type(r, *next))),
                                true,
                                on_endpoint(&me, &names, true, body, at),
                                at,
                            )),
                            false,
                        ));
                    }
                }
                State::Recv(from, bs) => {
                    let chan = format!("from_{}", p.roles[*from]);
                    let all = p.messages(*from, r);
                    let choice = bs.len() > 1;
                    let ret = if choice {
                        let variants = bs
                            .iter()
                            .map(|(l, t, next)| {
                                let mut fs: Vec<j::Field> =
                                    t.iter().map(|t| field("value", t.clone(), at)).collect();
                                fs.push(field("next", named(&p.state_type(r, *next)), at));
                                j::Variant {
                                    decorators: Vec::new(),
                                    name: l.clone(),
                                    fields: fs,
                                }
                            })
                            .collect();
                        out.push(item(
                            j::ItemKind::Type(j::TypeDef {
                                name: p.got_type(r, k),
                                tparams: Vec::new(),
                                rhs: j::TypeRhs::Sum(variants),
                                where_: None,
                            }),
                            false,
                        ));
                        named(&p.got_type(r, k))
                    } else {
                        let next = named(&p.state_type(r, bs[0].2));
                        match &bs[0].1 {
                            Some(t) => j::Type::Named {
                                path: vec!["tuple".into()],
                                args: vec![j::TArg::Type(t.clone()), j::TArg::Type(next)],
                            },
                            None => next,
                        }
                    };
                    let mut cases = Vec::new();
                    for (l, t, next) in bs {
                        let variant = &all
                            .iter()
                            .find(|(m, u, _)| m == l && u == t)
                            .expect("a message of the direction")
                            .2;
                        let ep = rebuild(&p.state_type(r, *next), &names, (&chan, "chan"), at);
                        let value = t.is_some().then(|| nm("value", at));
                        let result = if choice {
                            let mut args: Vec<j::Arg> =
                                value.into_iter().map(|v| arg(Some("value"), v)).collect();
                            args.push(arg(Some("next"), ep));
                            call(nm(l, at), args)
                        } else {
                            match value {
                                Some(v) => ex(j::ExprKind::Tuple(vec![v, ep]), at),
                                None => ep,
                            }
                        };
                        cases.push(j::Case {
                            pat: j::Pat::Path {
                                path: vec![variant.clone()],
                                args: Some(
                                    t.iter()
                                        .map(|_| {
                                            (
                                                Some("value".to_owned()),
                                                j::Pat::Path {
                                                    path: vec!["value".into()],
                                                    args: None,
                                                },
                                            )
                                        })
                                        .collect(),
                                ),
                            },
                            span: at,
                            guard: None,
                            body: j::Block {
                                inline: false,
                                stmts: vec![st(j::StmtKind::Return(Some(result)), at)],
                            },
                        });
                    }
                    if all.len() > bs.len() {
                        // The protocol never sends another label in this state.
                        cases.push(j::Case {
                            pat: j::Pat::Wild,
                            span: at,
                            guard: None,
                            body: j::Block {
                                inline: false,
                                stmts: vec![st(j::StmtKind::Unreachable, at)],
                            },
                        });
                    }
                    let recv = ex(
                        j::ExprKind::Call(
                            Box::new(ex(
                                j::ExprKind::Attr(Box::new(nm("chan", at)), "recv".into(), at),
                                at,
                            )),
                            Vec::new(),
                        ),
                        at,
                    );
                    let body = vec![
                        st(
                            j::StmtKind::Assign {
                                target: nm("chan", at),
                                ty: None,
                                value: nm(&format!("ep_{chan}"), at),
                            },
                            at,
                        ),
                        st(
                            j::StmtKind::Assign {
                                target: nm("msg", at),
                                ty: None,
                                value: or_closed(recv, at),
                            },
                            at,
                        ),
                        st(
                            j::StmtKind::Match {
                                scrutinee: nm("msg", at),
                                cases,
                            },
                            at,
                        ),
                    ];
                    out.push(item(
                        j::ItemKind::Func(func(
                            p.op_fn(r, k, "recv"),
                            vec![ep],
                            Some(ret),
                            true,
                            on_endpoint(&me, &names, true, body, at),
                            at,
                        )),
                        false,
                    ));
                }
                State::End => {
                    let body = vec![st(j::StmtKind::Pass, at)];
                    out.push(item(
                        j::ItemKind::Func(func(
                            p.op_fn(r, k, "close"),
                            vec![ep],
                            None,
                            false,
                            on_endpoint(&me, &names, false, body, at),
                            at,
                        )),
                        false,
                    ));
                }
            }
        }
        let init: Vec<j::Arg> = names
            .iter()
            .map(|f| {
                let (dir, peer) = f.split_once('_').expect("to_ or from_");
                let b = p.roles.iter().position(|x| x == peer).expect("a role");
                let v = if dir == "to" {
                    format!("c_{r}_{b}_tx")
                } else {
                    format!("c_{b}_{r}_rx")
                };
                arg(Some(f), nm(&v, at))
            })
            .collect();
        roles.push(field(&p.roles[r], named(&p.state_type(r, 0)), at));
        connect.push((p.roles[r].clone(), call(nm(&p.state_type(r, 0), at), init)));
    }
    out.push(item(
        j::ItemKind::Record(j::Record {
            name: p.roles_type(),
            tparams: Vec::new(),
            doc: None,
            fields: roles,
        }),
        false,
    ));
    let mut body = Vec::new();
    for a in 0..n {
        for b in 0..n {
            if p.messages(a, b).is_empty() {
                continue;
            }
            let chan = ex(
                j::ExprKind::Index(
                    Box::new(nm("Channel", at)),
                    Box::new(nm(&p.msg_type(a, b), at)),
                ),
                at,
            );
            let one = ex(j::ExprKind::Lit(j::Lit::Int("1".into())), at);
            body.push(st(
                j::StmtKind::Assign {
                    target: ex(
                        j::ExprKind::Tuple(vec![
                            nm(&format!("c_{a}_{b}_tx"), at),
                            nm(&format!("c_{a}_{b}_rx"), at),
                        ]),
                        at,
                    ),
                    ty: None,
                    value: call(chan, vec![arg(None, one)]),
                },
                at,
            ));
        }
    }
    let all = connect.into_iter().map(|(r, e)| arg(Some(&r), e)).collect();
    body.push(st(
        j::StmtKind::Return(Some(call(nm(&p.roles_type(), at), all))),
        at,
    ));
    out.push(item(
        j::ItemKind::Func(func(
            p.connect_fn(),
            Vec::new(),
            Some(named(&p.roles_type())),
            false,
            body,
            at,
        )),
        false,
    ));
    out
}
