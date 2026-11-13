//! Lazy `iter` pipelines (17.3, R508 to R515) in native code: the sources `iter.count`,
//! `repeat`, `once` and `empty` and the adapters on them are not values but a description kept
//! while lowering, which the consumer (`for`, `collect`, `next`, `count`, `sum`, `fold`) expands
//! into one pull loop. An adapter takes its source (`sink self`), so a pipeline name is consumed
//! by the next stage and cannot leave its function.

use super::*;

/// One stage of a pipeline; the state it keeps lives in locals created with it.
pub(super) enum Pipe {
    Count {
        cur: Local,
        step: Operand,
    },
    /// `left` is `None` for the unbounded `repeat`.
    Repeat {
        x: Vec<Operand>,
        left: Option<Local>,
    },
    Once {
        x: Vec<Operand>,
        spent: Local,
    },
    Empty(Shape),
    Map {
        src: Box<Pipe>,
        f: Vec<Operand>,
        from: Shape,
        to: Shape,
    },
    Filter {
        src: Box<Pipe>,
        f: Vec<Operand>,
        e: Shape,
    },
    TakeWhile {
        src: Box<Pipe>,
        f: Vec<Operand>,
        e: Shape,
    },
    DropWhile {
        src: Box<Pipe>,
        f: Vec<Operand>,
        e: Shape,
        dropping: Local,
    },
    Take {
        src: Box<Pipe>,
        left: Local,
    },
    Skip {
        src: Box<Pipe>,
        left: Local,
    },
    Enumerate {
        src: Box<Pipe>,
        idx: Local,
    },
}

/// A pipeline bound to a name, with the lookahead of the `for` protocol (`done`, `current`).
pub(super) struct Lazy {
    pipe: Pipe,
    e: Shape,
    /// The element `done` pulled, and whether it is waiting.
    buf: Vec<Local>,
    has: Local,
    end: Local,
    /// Whether `done`, `current` or `next` ran, so a consumer cannot start from the front.
    touched: bool,
}

impl<'a, 'm> Fn<'a, 'm> {
    fn flag(&mut self, v: bool) -> Local {
        let l = self.local(Ty::Bool);
        self.emit(Inst::Copy {
            dst: l,
            src: Operand::Bool(v),
        });
        l
    }

    fn counter(&mut self, init: Operand) -> Local {
        let l = self.local(I64);
        self.emit(Inst::Copy { dst: l, src: init });
        l
    }

    fn pipe_arg(&mut self, a: &y::Atom) -> Option<String> {
        match a {
            y::Atom::Name(n, _) if self.pipes.contains_key(n) => Some(n.clone()),
            _ => None,
        }
    }

    /// Whether `id` called on these arguments builds or consumes a pipeline.
    pub(super) fn is_pipe_call(&mut self, id: registry::Id, c: &y::Call) -> bool {
        if [
            id::STD_ITER_COUNT,
            id::STD_ITER_REPEAT,
            id::STD_ITER_ONCE,
            id::STD_ITER_EMPTY,
        ]
        .contains(&id)
        {
            return true;
        }
        let Ok(args) = Self::value_args(c) else {
            return false;
        };
        args.first()
            .is_some_and(|(_, a)| self.pipe_arg(a).is_some())
    }

    /// The pipeline a `let` binds to `n` when its right-hand side is one, in place of a value.
    pub(super) fn pipe_let(&mut self, n: &str, ty: &y::Type, rhs: &y::Rhs) -> R<bool> {
        let moved = match rhs {
            y::Rhs::Atom(y::Atom::Name(m, _)) => Some(m),
            y::Rhs::Move(p) | y::Rhs::Copy(p) | y::Rhs::Proj(p) if p.projs.is_empty() => {
                Some(&p.root)
            }
            _ => None,
        };
        if let Some(m) = moved {
            if let Some(l) = self.pipes.remove(m) {
                self.pipes.insert(n.to_owned(), l);
                return Ok(true);
            }
            return Ok(false);
        }
        let y::Rhs::Call(c) = rhs else {
            return Ok(false);
        };
        let Some(id) = registry::id(&c.path.join(".")) else {
            return Ok(false);
        };
        let building = [
            id::STD_ITER_COUNT,
            id::STD_ITER_REPEAT,
            id::STD_ITER_ONCE,
            id::STD_ITER_EMPTY,
            id::STD_ITER_OPS_MAP,
            id::STD_ITER_OPS_FILTER,
            id::STD_ITER_OPS_TAKE_WHILE,
            id::STD_ITER_OPS_DROP_WHILE,
            id::STD_ITER_OPS_TAKE,
            id::STD_ITER_OPS_SKIP,
            id::STD_ITER_OPS_ENUMERATE,
        ];
        if !building.contains(&id) || !self.is_pipe_call(id, c) {
            return Ok(false);
        }
        let Shape::List(e) = self.shape(ty)? else {
            return unsupported(format!("call of `{}` on this iterator", c.path.join(".")));
        };
        let lazy = self.pipe_stage(id, c, &e)?;
        self.pipes.insert(n.to_owned(), lazy);
        Ok(true)
    }

    fn lazy(&mut self, pipe: Pipe, e: Shape) -> Lazy {
        let buf = e.tys().into_iter().map(|t| self.local(t)).collect();
        let has = self.flag(false);
        let end = self.flag(false);
        Lazy {
            pipe,
            e,
            buf,
            has,
            end,
            touched: false,
        }
    }

    fn pipe_stage(&mut self, id: registry::Id, c: &y::Call, e: &Shape) -> R<Lazy> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let atom_at = |i: usize| {
            args.get(i)
                .map(|(_, a)| *a)
                .ok_or(())
                .or_else(|()| unsupported::<&y::Atom>(format!("call of `{name}`")))
        };
        let boolean = Shape::Scalar(Ty::Bool);
        match id {
            id::STD_ITER_COUNT => {
                let start = self.scalar1(atom_at(0)?)?;
                let step = self.scalar1(atom_at(1)?)?;
                let cur = self.counter(start);
                let step = Operand::Local(self.counter(step));
                Ok(self.lazy(Pipe::Count { cur, step }, e.clone()))
            }
            id::STD_ITER_REPEAT => {
                if Self::has_heap(e) {
                    return unsupported("`iter.repeat` of a value with a heap block");
                }
                let x = self.atom(atom_at(0)?, e)?;
                let x = self.hold(e, x);
                let n = self.int_or(atom_at(1)?, Operand::Int(-1))?;
                let unbounded =
                    self.scalar_op(ScalarOp::Eq, I64, vec![n.clone(), Operand::Int(-1)]);
                let (inf, fin) = self.branch(unbounded);
                let left = self.local(I64);
                let join = self.block_new();
                self.cur = inf;
                self.emit(Inst::Copy {
                    dst: left,
                    src: Operand::Int(-1),
                });
                self.goto(join);
                self.cur = fin;
                self.emit(Inst::Copy { dst: left, src: n });
                self.goto(join);
                Ok(self.lazy(
                    Pipe::Repeat {
                        x,
                        left: Some(left),
                    },
                    e.clone(),
                ))
            }
            id::STD_ITER_ONCE => {
                if Self::has_heap(e) {
                    return unsupported("`iter.once` of a value with a heap block");
                }
                let x = self.atom(atom_at(0)?, e)?;
                let x = self.hold(e, x);
                let spent = self.flag(false);
                Ok(self.lazy(Pipe::Once { x, spent }, e.clone()))
            }
            id::STD_ITER_EMPTY => Ok(self.lazy(Pipe::Empty(e.clone()), e.clone())),
            _ => {
                let Some(src) = self.pipe_arg(atom_at(0)?) else {
                    return unsupported(format!("call of `{name}` on this iterator"));
                };
                let src = self.pipes.remove(&src).expect("checked above");
                let from = src.e.clone();
                let from_pipe = Box::new(src.pipe);
                let stage = match id {
                    id::STD_ITER_OPS_MAP => {
                        let f = self.atom(
                            atom_at(1)?,
                            &Shape::Fn(vec![from.clone()], Box::new(e.clone())),
                        )?;
                        Pipe::Map {
                            src: from_pipe,
                            f,
                            from,
                            to: e.clone(),
                        }
                    }
                    id::STD_ITER_OPS_FILTER
                    | id::STD_ITER_OPS_TAKE_WHILE
                    | id::STD_ITER_OPS_DROP_WHILE => {
                        let f = self.atom(
                            atom_at(1)?,
                            &Shape::Fn(vec![from.clone()], Box::new(boolean)),
                        )?;
                        match id {
                            id::STD_ITER_OPS_FILTER => Pipe::Filter {
                                src: from_pipe,
                                f,
                                e: from,
                            },
                            id::STD_ITER_OPS_TAKE_WHILE => Pipe::TakeWhile {
                                src: from_pipe,
                                f,
                                e: from,
                            },
                            _ => {
                                let dropping = self.flag(true);
                                Pipe::DropWhile {
                                    src: from_pipe,
                                    f,
                                    e: from,
                                    dropping,
                                }
                            }
                        }
                    }
                    id::STD_ITER_OPS_TAKE | id::STD_ITER_OPS_SKIP => {
                        let n = self.scalar1(atom_at(1)?)?;
                        let left = self.counter(n);
                        if id == id::STD_ITER_OPS_TAKE {
                            Pipe::Take {
                                src: from_pipe,
                                left,
                            }
                        } else {
                            Pipe::Skip {
                                src: from_pipe,
                                left,
                            }
                        }
                    }
                    id::STD_ITER_OPS_ENUMERATE => {
                        let idx = self.counter(Operand::Int(0));
                        Pipe::Enumerate {
                            src: from_pipe,
                            idx,
                        }
                    }
                    _ => return unsupported(format!("call of `{name}` on this iterator")),
                };
                Ok(self.lazy(stage, e.clone()))
            }
        }
    }

    /// The operands `x` copied into locals the pipeline keeps.
    fn hold(&mut self, e: &Shape, x: Vec<Operand>) -> Vec<Operand> {
        let v = self.var(e.clone());
        self.copy_to(&v.locals, x);
        Self::ops(&v)
    }

    /// The next element of `p`, or a jump to `done` when it has none; the code continues in a
    /// block after the pull.
    fn pull(&mut self, p: &Pipe, e: &Shape, done: BlockId) -> R<Vec<Operand>> {
        Ok(match p {
            Pipe::Count { cur, step } => {
                let x = self.local(I64);
                self.emit(Inst::Copy {
                    dst: x,
                    src: Operand::Local(*cur),
                });
                let next = self.scalar_op(
                    ScalarOp::AddChecked,
                    I64,
                    vec![Operand::Local(*cur), step.clone()],
                );
                self.emit(Inst::Copy {
                    dst: *cur,
                    src: next,
                });
                vec![Operand::Local(x)]
            }
            Pipe::Repeat { x, left } => {
                if let Some(left) = left {
                    let inf = self.scalar_op(
                        ScalarOp::Eq,
                        I64,
                        vec![Operand::Local(*left), Operand::Int(-1)],
                    );
                    let (go, bounded) = self.branch(inf);
                    let join = self.block_new();
                    self.cur = bounded;
                    let out = self.scalar_op(
                        ScalarOp::Le,
                        I64,
                        vec![Operand::Local(*left), Operand::Int(0)],
                    );
                    let (stop, more) = self.branch(out);
                    self.cur = stop;
                    self.goto(done);
                    self.cur = more;
                    let next = self.scalar_op(
                        ScalarOp::SubChecked,
                        I64,
                        vec![Operand::Local(*left), Operand::Int(1)],
                    );
                    self.emit(Inst::Copy {
                        dst: *left,
                        src: next,
                    });
                    self.goto(join);
                    self.cur = go;
                    self.goto(join);
                    self.cur = join;
                }
                x.clone()
            }
            Pipe::Once { x, spent } => {
                let s = Operand::Local(*spent);
                let (stop, go) = self.branch(s);
                self.cur = stop;
                self.goto(done);
                self.cur = go;
                self.emit(Inst::Copy {
                    dst: *spent,
                    src: Operand::Bool(true),
                });
                x.clone()
            }
            Pipe::Empty(shape) => {
                self.goto(done);
                self.cur = self.block_new();
                let v = self.var(shape.clone());
                Self::ops(&v)
            }
            Pipe::Map { src, f, from, to } => {
                let x = self.pull(src, from, done)?;
                let r = self.call_closure(f, &[(from, &x)], to);
                self.glue(from, &x);
                r
            }
            Pipe::Filter { src, f, e } => {
                let head = self.block_new();
                self.goto(head);
                self.cur = head;
                let x = self.pull(src, e, done)?;
                let keep = self.call_closure(f, &[(e, &x)], &Shape::Scalar(Ty::Bool));
                let (yes, no) = self.branch(keep[0].clone());
                self.cur = no;
                self.glue(e, &x);
                self.goto(head);
                self.cur = yes;
                x
            }
            Pipe::TakeWhile { src, f, e } => {
                let x = self.pull(src, e, done)?;
                let keep = self.call_closure(f, &[(e, &x)], &Shape::Scalar(Ty::Bool));
                let (yes, no) = self.branch(keep[0].clone());
                self.cur = no;
                self.glue(e, &x);
                self.goto(done);
                self.cur = yes;
                x
            }
            Pipe::DropWhile {
                src,
                f,
                e,
                dropping,
            } => {
                let head = self.block_new();
                self.goto(head);
                self.cur = head;
                let x = self.pull(src, e, done)?;
                let (check, pass) = self.branch(Operand::Local(*dropping));
                self.cur = check;
                let keep = self.call_closure(f, &[(e, &x)], &Shape::Scalar(Ty::Bool));
                let (again, first) = self.branch(keep[0].clone());
                self.cur = again;
                self.glue(e, &x);
                self.goto(head);
                self.cur = first;
                self.emit(Inst::Copy {
                    dst: *dropping,
                    src: Operand::Bool(false),
                });
                let join = self.block_new();
                self.goto(join);
                self.cur = pass;
                self.goto(join);
                self.cur = join;
                x
            }
            Pipe::Take { src, left } => {
                let out = self.scalar_op(
                    ScalarOp::Le,
                    I64,
                    vec![Operand::Local(*left), Operand::Int(0)],
                );
                let (stop, go) = self.branch(out);
                self.cur = stop;
                self.goto(done);
                self.cur = go;
                let next = self.scalar_op(
                    ScalarOp::SubChecked,
                    I64,
                    vec![Operand::Local(*left), Operand::Int(1)],
                );
                self.emit(Inst::Copy {
                    dst: *left,
                    src: next,
                });
                self.pull(src, e, done)?
            }
            Pipe::Skip { src, left } => {
                let head = self.block_new();
                self.goto(head);
                self.cur = head;
                let out = self.scalar_op(
                    ScalarOp::Le,
                    I64,
                    vec![Operand::Local(*left), Operand::Int(0)],
                );
                let (go, skip) = self.branch(out);
                self.cur = skip;
                let x = self.pull(src, e, done)?;
                self.glue(e, &x);
                let next = self.scalar_op(
                    ScalarOp::SubChecked,
                    I64,
                    vec![Operand::Local(*left), Operand::Int(1)],
                );
                self.emit(Inst::Copy {
                    dst: *left,
                    src: next,
                });
                self.goto(head);
                self.cur = go;
                self.pull(src, e, done)?
            }
            Pipe::Enumerate { src, idx } => {
                let Shape::Tuple(ts) = e else {
                    return unsupported("`enumerate` of this element");
                };
                let x = self.pull(src, &ts[1], done)?;
                let i = self.local(I64);
                self.emit(Inst::Copy {
                    dst: i,
                    src: Operand::Local(*idx),
                });
                let next = self.scalar_op(
                    ScalarOp::AddChecked,
                    I64,
                    vec![Operand::Local(*idx), Operand::Int(1)],
                );
                self.emit(Inst::Copy {
                    dst: *idx,
                    src: next,
                });
                let mut out = vec![Operand::Local(i)];
                out.extend(x);
                out
            }
        })
    }

    /// Calls on a pipeline name: the `for` protocol and the consumers.
    pub(super) fn pipe_consume(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let Some(src) = args.first().and_then(|(_, a)| self.pipe_arg(a)) else {
            return unsupported(format!("call of `{name}` on this iterator"));
        };
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let e = self.pipes[&src].e.clone();
        match id {
            id::STD_ITER_OPS_DONE | id::STD_ITER_OPS_NEXT => {
                let mut lazy = self.pipes.remove(&src).expect("checked");
                lazy.touched = true;
                let (has, end) = (lazy.has, lazy.end);
                let r = self.local(Ty::Bool);
                let join = self.block_new();
                let (waiting, fresh) = self.branch(Operand::Local(has));
                self.cur = waiting;
                if id == id::STD_ITER_OPS_NEXT {
                    // `next` after a `done` takes the element `done` pulled.
                    self.emit(Inst::Copy {
                        dst: has,
                        src: Operand::Bool(false),
                    });
                }
                self.emit(Inst::Copy {
                    dst: r,
                    src: Operand::Bool(id == id::STD_ITER_OPS_NEXT),
                });
                self.goto(join);
                self.cur = fresh;
                let (ended, live) = self.branch(Operand::Local(end));
                self.cur = ended;
                self.emit(Inst::Copy {
                    dst: r,
                    src: Operand::Bool(id == id::STD_ITER_OPS_DONE),
                });
                self.goto(join);
                self.cur = live;
                let exhausted = self.block_new();
                let x = self.pull(&lazy.pipe, &e, exhausted)?;
                for (b, v) in lazy.buf.iter().zip(x) {
                    self.emit(Inst::Copy { dst: *b, src: v });
                }
                self.emit(Inst::Copy {
                    dst: has,
                    src: Operand::Bool(id == id::STD_ITER_OPS_DONE),
                });
                self.emit(Inst::Copy {
                    dst: r,
                    src: Operand::Bool(id == id::STD_ITER_OPS_NEXT),
                });
                self.goto(join);
                self.cur = exhausted;
                self.emit(Inst::Copy {
                    dst: end,
                    src: Operand::Bool(true),
                });
                self.emit(Inst::Copy {
                    dst: r,
                    src: Operand::Bool(id == id::STD_ITER_OPS_DONE),
                });
                self.goto(join);
                self.cur = join;
                let buf: Vec<Operand> = lazy.buf.iter().map(|b| Operand::Local(*b)).collect();
                self.pipes.insert(src, lazy);
                if id == id::STD_ITER_OPS_DONE {
                    set(self, vec![Operand::Local(r)]);
                } else if let Some(d) = &dst {
                    // `r` is whether an element came out: wrap `buf` in the `T | None`.
                    let ds = d.shape.clone();
                    let (some, none) = self.branch(Operand::Local(r));
                    let out = self.block_new();
                    self.cur = some;
                    self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "Some", buf));
                    self.goto(out);
                    self.cur = none;
                    self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "None", vec![]));
                    self.goto(out);
                    self.cur = out;
                }
            }
            id::STD_ITER_OPS_CURRENT => {
                let l = &self.pipes[&src];
                let (has, buf) = (l.has, l.buf.clone());
                let x: Vec<Operand> = buf.iter().map(|b| Operand::Local(*b)).collect();
                self.emit(Inst::Copy {
                    dst: has,
                    src: Operand::Bool(false),
                });
                set(self, x);
            }
            _ => {
                let lazy = self.pipes.remove(&src).expect("checked");
                if lazy.touched {
                    return unsupported(format!("call of `{name}` on a partly consumed iterator"));
                }
                let r = self.pipe_drain(id, c, &args, lazy, dst.clone())?;
                if let Some(r) = r {
                    set(self, r);
                }
            }
        }
        Ok(())
    }

    /// A consumer that runs the pipeline to its end.
    fn pipe_drain(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        args: &[(y::Conv, &y::Atom)],
        lazy: Lazy,
        dst: Option<Var>,
    ) -> R<Option<Vec<Operand>>> {
        let name = c.path.join(".");
        let e = lazy.e.clone();
        let head = self.block_new();
        let exhausted = self.block_new();
        match id {
            id::STD_ITER_OPS_COLLECT => {
                let out = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                self.goto(head);
                self.cur = head;
                let x = self.pull(&lazy.pipe, &e, exhausted)?;
                let slot = self.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                self.write(&At::Mem(slot, 0), x);
                self.goto(head);
                self.cur = exhausted;
                Ok(Some(vec![out]))
            }
            id::STD_ITER_OPS_COUNT | id::STD_ITER_OPS_SUM => {
                if id == id::STD_ITER_OPS_SUM && e != Shape::Scalar(I64) {
                    return unsupported("`sum` of a pipeline of this element");
                }
                let acc = self.counter(Operand::Int(0));
                self.goto(head);
                self.cur = head;
                let x = self.pull(&lazy.pipe, &e, exhausted)?;
                let add = if id == id::STD_ITER_OPS_SUM {
                    x[0].clone()
                } else {
                    self.glue(&e, &x);
                    Operand::Int(1)
                };
                let next =
                    self.scalar_op(ScalarOp::AddChecked, I64, vec![Operand::Local(acc), add]);
                self.emit(Inst::Copy {
                    dst: acc,
                    src: next,
                });
                self.goto(head);
                self.cur = exhausted;
                Ok(Some(vec![Operand::Local(acc)]))
            }
            id::STD_ITER_OPS_FOLD => {
                let Some(d) = &dst else {
                    return Ok(None);
                };
                let a = d.shape.clone();
                if Self::has_heap(&a) {
                    return unsupported("`fold` of a pipeline into a heap value");
                }
                let init = self.atom(args[1].1, &a)?;
                let f = self.atom(
                    args[2].1,
                    &Shape::Fn(vec![a.clone(), e.clone()], Box::new(a.clone())),
                )?;
                let acc = self.var(a.clone());
                self.copy_to(&acc.locals, init);
                self.goto(head);
                self.cur = head;
                let x = self.pull(&lazy.pipe, &e, exhausted)?;
                let r = self.call_closure(&f, &[(&a, &Self::ops(&acc)), (&e, &x)], &a);
                self.glue(&e, &x);
                self.copy_to(&acc.locals, r);
                self.goto(head);
                self.cur = exhausted;
                Ok(Some(Self::ops(&acc)))
            }
            _ => unsupported(format!("call of `{name}` on this iterator")),
        }
    }
}
