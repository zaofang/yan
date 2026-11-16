//! Yan Core types as native slot shapes.

use super::*;

/// A Yan Core type as scalar slots.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Scalar(Ty),
    Str,
    /// A `bytes` value: a block holding a `Vec<u8>` (`yan_rt::bytes`).
    Bytes,
    /// A `bytearray`: the same block, changed in place.
    ByteArray,
    /// A `bigint`: a block holding its two's-complement bytes, little endian (`yan_rt::bigint`).
    BigInt,
    /// A `Path`: a `Str` block of normalized text, ordered by its parts.
    Path,
    /// A `WindowsPath`: a `Str` block of normalized text, equal, ordered and hashed case-folded.
    WinPath,
    /// A `char`: its code point in one word.
    Char,
    List(Box<Shape>),
    /// A `dict`: a block of entries in insertion order (`yan_rt::map`).
    Map(Box<Shape>, Box<Shape>),
    /// A `heap`: a list block kept as a min-heap (`yan_rt::pq`).
    Heap(Box<Shape>),
    /// A `deque`: a list block that also holds its `maxlen` (`yan_rt::dq`).
    Deque(Box<Shape>),
    Rec(String, Vec<(String, Shape)>),
    Tuple(Vec<Shape>),
    /// A tag slot, then each variant's fields in order.
    Sum(String, Vec<(String, Vec<(String, Shape)>)>),
    /// A closure: its code and its captures' block. `ret` is a `Result` for `fail E`.
    Fn(Vec<Shape>, Box<Shape>),
    /// A `Sender[T]` (true) or `Receiver[T]`: the channel both ends share.
    Chan(Box<Shape>, bool),
    /// An `Actor[S]`: the actor's index in `yan_rt::actor`, which the handle does not own.
    Actor(Box<Shape>),
    /// A `Ptr[T]` (6.6): its address, then its provenance (0 for none).
    Ptr,
    /// An `Arena`: a `yan_rt::arena` block that owns every slot `alloc` handed out.
    Arena,
    /// A `Ref[T]`: the address of its slot in an arena; the arena owns it.
    Ref(Box<Shape>),
    /// An `Atomic[T]` of an integer or `bool`: a block of one word, `T` wide at its start.
    Atomic(Ty),
    /// A `set`: a `yan_rt::map` block whose entries are keys with no value slots.
    Set(Box<Shape>),
    /// A recursive type where it recurs: always behind a block, so its slots are its type's.
    Named(Knot),
    /// A field whose type holds its record or sum again in its own slots, boxed (5.1): the
    /// address of an `Alloc` block of the slots of the knot's type.
    Boxed(Knot),
}

/// A back-reference to a recursive record or sum, tied once its type is built. Equal, hashed
/// and printed by name, so a cycle never recurses.
#[derive(Clone)]
pub struct Knot(pub String, Rc<OnceCell<Shape>>);

impl Knot {
    pub fn get(&self) -> &Shape {
        self.1.get().expect("a knot is tied before use")
    }
}

impl PartialEq for Knot {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}

impl Eq for Knot {}

impl std::hash::Hash for Knot {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.0.hash(h)
    }
}

impl std::fmt::Debug for Knot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Named({})", self.0)
    }
}

impl Shape {
    pub(super) fn slots(&self, out: &mut Vec<Ty>) {
        match self {
            Shape::Scalar(t) => out.push(*t),
            Shape::Char => out.push(I64),
            Shape::Str
            | Shape::Bytes
            | Shape::ByteArray
            | Shape::BigInt
            | Shape::Path
            | Shape::WinPath
            | Shape::List(_)
            | Shape::Map(..)
            | Shape::Heap(_)
            | Shape::Deque(_)
            | Shape::Chan(..)
            | Shape::Actor(_)
            | Shape::Arena
            | Shape::Ref(_)
            | Shape::Atomic(_)
            | Shape::Set(_)
            | Shape::Boxed(_) => out.push(Ty::Ptr),
            Shape::Fn(..) => out.extend([Ty::Ptr, Ty::Ptr]),
            Shape::Ptr => out.extend([I64, I64]),
            Shape::Rec(_, fs) => fs.iter().for_each(|(_, s)| s.slots(out)),
            Shape::Tuple(ss) => ss.iter().for_each(|s| s.slots(out)),
            Shape::Sum(_, vs) => {
                out.push(I64);
                vs.iter()
                    .flat_map(|(_, fs)| fs)
                    .for_each(|(_, s)| s.slots(out));
            }
            Shape::Named(k) => k.get().slots(out),
        }
    }

    /// Whether `name` recurs in these slots rather than behind a block.
    fn recurs_inline(&self, name: &str) -> bool {
        match self {
            Shape::Named(k) => k.0 == name,
            Shape::Rec(_, fs) => fs.iter().any(|(_, s)| s.recurs_inline(name)),
            Shape::Tuple(ss) => ss.iter().any(|s| s.recurs_inline(name)),
            Shape::Sum(_, vs) => vs
                .iter()
                .flat_map(|(_, fs)| fs)
                .any(|(_, s)| s.recurs_inline(name)),
            _ => false,
        }
    }

    pub(super) fn tys(&self) -> Vec<Ty> {
        let mut v = Vec::new();
        self.slots(&mut v);
        v
    }

    pub(super) fn width(&self) -> usize {
        self.tys().len()
    }

    /// The fields of a record or tuple, by name, with their first slot.
    pub(super) fn fields(&self) -> Option<Vec<(String, usize, &Shape)>> {
        let named: Vec<(String, &Shape)> = match self {
            Shape::Rec(_, fs) => fs.iter().map(|(n, s)| (n.clone(), s)).collect(),
            Shape::Tuple(ss) => ss
                .iter()
                .enumerate()
                .map(|(i, s)| (format!("_{i}"), s))
                .collect(),
            _ => return None,
        };
        let mut at = 0;
        Some(
            named
                .into_iter()
                .map(|(n, s)| {
                    let r = (n, at, s);
                    at += s.width();
                    r
                })
                .collect(),
        )
    }

    /// A sum's variant `name`: its tag, first slot and fields.
    pub(super) fn variant(&self, name: &str) -> Option<(i128, usize, &Fields)> {
        let Shape::Sum(_, vs) = self else {
            return None;
        };
        let mut at = 1;
        for (tag, (n, fs)) in vs.iter().enumerate() {
            if n == name {
                return Some((tag as i128, at, fs));
            }
            at += fs.iter().map(|(_, s)| s.width()).sum::<usize>();
        }
        None
    }
}

pub(super) struct Types<'m> {
    pub(super) recs: HashMap<&'m str, &'m y::Record>,
    pub(super) sums: HashMap<&'m str, &'m y::Sum>,
    pub(super) externs: HashMap<String, R<crate::ffi::ExternFn>>,
}

/// The records and sums being built, outermost first, each with the knot its recurrences share.
pub(super) type Stack = Vec<(String, Rc<OnceCell<Shape>>)>;

/// The back-reference to `name` if it is being built.
fn knot(stack: &Stack, name: &str) -> Option<Shape> {
    let (n, cell) = stack.iter().find(|(n, _)| n == name)?;
    Some(Shape::Named(Knot(n.clone(), cell.clone())))
}

/// Ties the knot of the type `built` on top of `stack`. Its fields that recur in its own slots
/// were boxed, so a recurrence left there is one boxing missed.
fn tie(stack: &mut Stack, built: R<Shape>) -> R<Shape> {
    let (name, cell) = stack.pop().expect("a type being built");
    let s = built?;
    if Rc::strong_count(&cell) > 1 {
        if s.recurs_inline(&name) {
            return unsupported(format!(
                "recursive type `{name}` with an unboxed recurrence"
            ));
        }
        cell.set(s.clone()).expect("tied once");
    }
    Ok(s)
}

impl Types<'_> {
    pub(super) fn shape(&self, t: &y::Type) -> R<Shape> {
        self.shape_in(t, &mut Vec::new())
    }

    /// The shape of the field type `t` (`decl` as written) of the record or sum `owner`: boxed
    /// when `decl` holds `owner` again in its own slots, so that every type has one shape.
    fn field_shape(&self, owner: &str, decl: &y::Type, t: &y::Type, stack: &mut Stack) -> R<Shape> {
        let s = self.shape_in(t, stack)?;
        if !self.reaches(decl, owner) {
            return Ok(s);
        }
        Ok(Shape::Boxed(match s {
            Shape::Named(k) => k,
            s => Knot(yan_core::type_text(t), Rc::new(OnceCell::from(s))),
        }))
    }

    /// Whether a value of type `t` holds the record or sum `owner` in its own slots, directly or
    /// through the fields of other records and sums. A type argument counts as held, since an
    /// instance may hold it.
    fn reaches(&self, t: &y::Type, owner: &str) -> bool {
        let mut seen = std::collections::HashSet::new();
        let mut todo = Vec::new();
        inline_heads(t, &mut todo);
        while let Some(n) = todo.pop() {
            if n == owner {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            if let Some(r) = self.recs.get(n) {
                r.fields.iter().for_each(|f| inline_heads(&f.ty, &mut todo));
            } else if let Some(s) = self.sums.get(n) {
                s.variants
                    .iter()
                    .flat_map(|v| v.fields.iter().flatten())
                    .for_each(|f| inline_heads(&f.ty, &mut todo));
            }
        }
        false
    }

    /// The shape of a nominal record or sum at type arguments: its fields with the arguments
    /// substituted, named by the canonical instance text (D3, D7).
    fn generic_shape(&self, n: &y::Named, stack: &mut Stack) -> Option<R<Shape>> {
        let name = n.path[0].as_str();
        let (tparams, fields): (&[y::TParam], Vec<(&str, &[y::Field])>) =
            if let Some(r) = self.recs.get(name).filter(|r| !r.tparams.is_empty()) {
                (&r.tparams, vec![("", &r.fields[..])])
            } else {
                let s = self.sums.get(name).filter(|s| !s.tparams.is_empty())?;
                let vs = s
                    .variants
                    .iter()
                    .map(|v| (v.name.as_str(), v.fields.as_deref().unwrap_or(&[])))
                    .collect();
                (&s.tparams, vs)
            };
        let is_sum = self.sums.contains_key(name) && !self.recs.contains_key(name);
        let inst = yan_core::subst::instance(&n.path, &n.args);
        if let Some(k) = knot(stack, &inst) {
            return Some(Ok(k));
        }
        stack.push((inst.clone(), Rc::default()));
        let built = (|| {
            let mut out = Vec::new();
            for (vname, fs) in &fields {
                let fs = fs
                    .iter()
                    .map(|f| {
                        let t = yan_core::subst::apply(&f.ty, tparams, &n.args);
                        Ok((f.name.clone(), self.field_shape(name, &f.ty, &t, stack)?))
                    })
                    .collect::<R<Vec<_>>>()?;
                out.push(((*vname).to_owned(), fs));
            }
            Ok(if is_sum {
                Shape::Sum(inst, out)
            } else {
                Shape::Rec(inst, out.pop().map(|(_, f)| f).unwrap_or_default())
            })
        })();
        Some(tie(stack, built))
    }

    pub(super) fn shape_in(&self, t: &y::Type, stack: &mut Stack) -> R<Shape> {
        match t {
            y::Type::Tuple(ts) => Ok(Shape::Tuple(
                ts.iter()
                    .map(|t| self.shape_in(t, stack))
                    .collect::<R<_>>()?,
            )),
            y::Type::Named(n) if n.path.len() == 1 => {
                let name = n.path[0].as_str();
                match (name, n.args.as_slice()) {
                    ("Str", []) => return Ok(Shape::Str),
                    ("Bytes", []) => return Ok(Shape::Bytes),
                    ("ByteArray", []) => return Ok(Shape::ByteArray),
                    ("bigint", []) => return Ok(Shape::BigInt),
                    ("Path", []) => return Ok(Shape::Path),
                    ("WindowsPath", []) => return Ok(Shape::WinPath),
                    // A capability handler is its index in `yan_rt` (`rand`, `fshandler`,
                    // `loghandler`), unless the program declares a type of that name.
                    ("Rand" | "Fs" | "Log" | "Env" | "Time", [])
                        if !self.recs.contains_key(name) && !self.sums.contains_key(name) =>
                    {
                        return Ok(Shape::Scalar(I64));
                    }
                    // The handler of `std.proc` is the dict `fake_proc` was given; 0 is the host.
                    ("Proc", [])
                        if !self.recs.contains_key(name) && !self.sums.contains_key(name) =>
                    {
                        let out = y::Type::Named(y::Named {
                            path: vec!["Output".into()],
                            args: Vec::new(),
                        });
                        return Ok(Shape::Map(
                            Box::new(Shape::Str),
                            Box::new(self.shape_in(&out, stack)?),
                        ));
                    }
                    // The scheduler `pct` builds is `yan_rt::sched`'s, not a value.
                    ("Sched", [])
                        if !self.recs.contains_key(name) && !self.sums.contains_key(name) =>
                    {
                        return Ok(Shape::Scalar(I64));
                    }
                    ("Ptr", [y::TArg::Type(_)]) if !self.recs.contains_key(name) => {
                        return Ok(Shape::Ptr);
                    }
                    ("Arena", []) if !self.recs.contains_key(name) => return Ok(Shape::Arena),
                    ("Ref", [y::TArg::Type(e)]) if !self.recs.contains_key(name) => {
                        return Ok(Shape::Ref(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("Atomic", [y::TArg::Type(e)]) if !self.recs.contains_key(name) => {
                        return match self.shape_in(e, stack)? {
                            Shape::Scalar(t @ (Ty::Int(_) | Ty::Bool)) => Ok(Shape::Atomic(t)),
                            _ => unsupported(
                                "an `Atomic` of a type that is not an integer or `bool`",
                            ),
                        };
                    }
                    ("char", []) => return Ok(Shape::Char),
                    ("Secret", [y::TArg::Type(e)]) if !self.recs.contains_key(name) => {
                        return Ok(Shape::Rec(
                            "Secret".into(),
                            vec![("value".into(), self.shape_in(e, stack)?)],
                        ));
                    }
                    // No lazy adapters natively: an `iter` from `std.itertools` is its list.
                    ("List" | "Iter", [y::TArg::Type(e)]) => {
                        return Ok(Shape::List(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("Map", [y::TArg::Type(k), y::TArg::Type(v)]) => {
                        return Ok(Shape::Map(
                            Box::new(self.shape_in(k, stack)?),
                            Box::new(self.shape_in(v, stack)?),
                        ));
                    }
                    ("Heap", [y::TArg::Type(e)]) => {
                        return Ok(Shape::Heap(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("Deque", [y::TArg::Type(e)]) => {
                        return Ok(Shape::Deque(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("SortedMap", [y::TArg::Type(k), y::TArg::Type(v)]) => {
                        return Ok(super::sorted::sorted_dict(
                            self.shape_in(k, stack)?,
                            self.shape_in(v, stack)?,
                        ));
                    }
                    ("SortedSet", [y::TArg::Type(t)]) => {
                        return Ok(super::sorted::sorted_set(self.shape_in(t, stack)?));
                    }
                    ("Option", [y::TArg::Type(t)]) => {
                        let t = self.shape_in(t, stack)?;
                        return Ok(Shape::Sum(
                            "Option".into(),
                            vec![
                                ("Some".into(), vec![("value".into(), t)]),
                                ("None".into(), vec![]),
                            ],
                        ));
                    }
                    ("Result", [y::TArg::Type(t), y::TArg::Type(e)]) => {
                        return Ok(result(self.shape_in(t, stack)?, self.shape_in(e, stack)?));
                    }
                    ("Sender" | "Receiver", [y::TArg::Type(e)]) => {
                        let sender = name == "Sender";
                        return Ok(Shape::Chan(Box::new(self.shape_in(e, stack)?), sender));
                    }
                    // The overflow policy is the actor's, at run time.
                    ("Actor" | "SendOnly", [y::TArg::Type(s), ..]) => {
                        return Ok(Shape::Actor(Box::new(self.shape_in(s, stack)?)));
                    }
                    ("Set", [y::TArg::Type(e)]) => {
                        return Ok(Shape::Set(Box::new(self.shape_in(e, stack)?)));
                    }
                    // A `Remote` is its handle: every node runs in this process (R417).
                    ("Remote", [y::TArg::Type(h)]) => return self.shape_in(h, stack),
                    // A `Group` is its index in `yan_rt`; its members are one-word handles.
                    ("Group", [y::TArg::Type(a)]) => {
                        if !matches!(self.shape_in(a, stack)?, Shape::Actor(_) | Shape::Scalar(_)) {
                            return unsupported(format!(
                                "type `{name}[..]` of members that are not handles"
                            ));
                        }
                        return Ok(Shape::Scalar(I64));
                    }
                    ("Undelivered", [y::TArg::Type(a), y::TArg::Type(r)]) => {
                        let (a, r) = (self.shape_in(a, stack)?, self.shape_in(r, stack)?);
                        return Ok(actor::undelivered(a, r));
                    }
                    (_, []) => {}
                    _ => {
                        if let Some(s) = self.generic_shape(n, stack) {
                            return s;
                        }
                        return unsupported(format!("type `{name}[..]`"));
                    }
                }
                if let Some(t) = scalar_ty(name) {
                    return Ok(Shape::Scalar(t));
                }
                if let Some(k) = knot(stack, name) {
                    return Ok(k);
                }
                stack.push((name.to_owned(), Rc::default()));
                let fields = |fs: &[y::Field], stack: &mut Stack| {
                    fs.iter()
                        .map(|f| Ok((f.name.clone(), self.field_shape(name, &f.ty, &f.ty, stack)?)))
                        .collect::<R<Vec<_>>>()
                };
                let built = (|| {
                    Ok(
                        if let Some(r) = self.recs.get(name).filter(|r| r.tparams.is_empty()) {
                            Shape::Rec(name.to_owned(), fields(&r.fields, stack)?)
                        } else if let Some(s) = self.sums.get(name).filter(|s| s.tparams.is_empty())
                        {
                            let vs = s
                                .variants
                                .iter()
                                .map(|v| {
                                    let fs = v.fields.as_deref().unwrap_or(&[]);
                                    Ok((v.name.clone(), fields(fs, stack)?))
                                })
                                .collect::<R<_>>()?;
                            Shape::Sum(name.to_owned(), vs)
                        } else if STD_FAILURES.contains(&name) {
                            Shape::Rec(name.to_owned(), Vec::new())
                        } else if name == "Duration" {
                            actor::duration()
                        } else if name == "i128" {
                            time::wide("i128")
                        } else if let Some(s) = proc::std_type(name) {
                            s
                        } else if let Some(s) = civil::std_type(name) {
                            s
                        } else if name == "Instant" {
                            time::shape(true)
                        } else if name == "Stopped" {
                            actor::stopped()
                        } else if let Some(s) = cluster::std_type(name) {
                            s
                        } else {
                            return unsupported(format!("type `{name}`"));
                        },
                    )
                })();
                tie(stack, built)
            }
            y::Type::Fn(ft) => {
                if ft.params.iter().any(|(c, _)| *c == y::Conv::Inout) {
                    return unsupported("a closure type with `inout` parameters");
                }
                let ps = ft
                    .params
                    .iter()
                    .map(|(_, t)| self.shape_in(t, stack))
                    .collect::<R<_>>()?;
                let ok = self.shape_in(&ft.ret, stack)?;
                let ret = match &ft.fail {
                    Some(e) => result(ok, self.shape_in(e, stack)?),
                    None => ok,
                };
                Ok(Shape::Fn(ps, Box::new(ret)))
            }
            // A union is a sum over its members, in the order written.
            y::Type::Union(ts) => Ok(Shape::Sum(
                "Union".into(),
                ts.iter()
                    .map(|t| {
                        let s = self.shape_in(t, stack)?;
                        Ok((yan_core::type_text(t), vec![("value".into(), s)]))
                    })
                    .collect::<R<_>>()?,
            )),
            _ => unsupported("a compound type"),
        }
    }
}

/// The names of the types `t` holds in its own slots: not those behind a block.
fn inline_heads<'t>(t: &'t y::Type, out: &mut Vec<&'t str>) {
    match t {
        y::Type::Named(n) => {
            let head = n.path.last().expect("named").as_str();
            if matches!(
                head,
                "List"
                    | "Iter"
                    | "Map"
                    | "Heap"
                    | "Deque"
                    | "Set"
                    | "SortedMap"
                    | "SortedSet"
                    | "Sender"
                    | "Receiver"
                    | "Actor"
                    | "SendOnly"
                    | "Group"
                    | "Ref"
                    | "Ptr"
            ) {
                return;
            }
            out.push(head);
            for a in &n.args {
                if let y::TArg::Type(t) = a {
                    inline_heads(t, out);
                }
            }
        }
        y::Type::Tuple(ts) | y::Type::Union(ts) => ts.iter().for_each(|t| inline_heads(t, out)),
        y::Type::Refined { ty, .. } => inline_heads(ty, out),
        y::Type::Dyn(_) | y::Type::Fn(_) => {}
    }
}

/// The fieldless failure types of section 9, which std declares.
/// Failures a `par` ranks below every other: a cancelled wait, and the `SessionClosed` a session
/// operation turns it into (R394, R634).
pub(super) const CANCEL_LIKE: &[&str] = &["Cancelled", "SessionClosed"];
pub(super) const STD_FAILURES: &[&str] = &[
    "Cancelled",
    "Timeout",
    "Draining",
    "Deadlock",
    "Full",
    "Unreachable",
    "Rejected",
    "SchemaMismatch",
];

/// The union of the fieldless failures `names`, in order.
pub(super) fn failure_union(names: &[&str]) -> Shape {
    Shape::Sum(
        "Union".into(),
        names
            .iter()
            .map(|n| {
                let m = Shape::Rec((*n).into(), Vec::new());
                ((*n).into(), vec![("value".into(), m)])
            })
            .collect(),
    )
}

pub(super) fn result(ok: Shape, err: Shape) -> Shape {
    Shape::Sum(
        "Result".into(),
        vec![
            ("Ok".into(), vec![("value".into(), ok)]),
            ("Err".into(), vec![("error".into(), err)]),
        ],
    )
}
