//! `std.cluster` and `std.sim` in native code (RFC 0063): the `sim` handler's nodes in
//! `yan_rt::cluster`, which node code runs on (D1), and the `Wire` round trip of every value
//! that crosses to a node (R402), over `yan_rt::wireslots`. Requests, replies and sends are
//! faulted in `yan_rt::cluster`.

use super::*;

const WIRE_IDS: [registry::Id; 4] = [
    id::STD_CLUSTER_WIRE_SEND,
    id::STD_CLUSTER_WIRE,
    id::STD_CLUSTER_WIRE_REPLY,
    id::STD_CLUSTER_WIRE_START,
];

/// The shape of a type `std.cluster` or `std.sim` declares.
pub(super) fn std_type(name: &str) -> Option<Shape> {
    let duration = actor::duration;
    Some(match name {
        "Node" => Shape::Rec("Node".into(), vec![("id".into(), Shape::Str)]),
        // The handler is the `sim` cluster's index in `yan_rt::cluster`.
        "Cluster" => Shape::Scalar(I64),
        "Cut" => Shape::Rec(
            "Cut".into(),
            vec![
                ("at".into(), duration()),
                ("heal".into(), duration()),
                ("side".into(), Shape::Set(Box::new(Shape::Scalar(I64)))),
            ],
        ),
        "FaultPlan" => Shape::Rec(
            "FaultPlan".into(),
            vec![
                ("drop".into(), Shape::Scalar(Ty::F64)),
                ("delay".into(), duration()),
                ("partition".into(), duration()),
                ("heal".into(), duration()),
                ("cuts".into(), Shape::List(Box::new(std_type("Cut")?))),
            ],
        ),
        _ => return None,
    })
}

impl<'m> Fn<'_, 'm> {
    /// A `set` as the map block it is: keys with no value slots.
    pub(super) fn set_map(k: &Shape) -> Shape {
        Shape::Map(Box::new(k.clone()), Box::new(Shape::Tuple(Vec::new())))
    }

    /// A function-wide `i64` local, zero on entry.
    pub(super) fn entry_local(&mut self, init: i128) -> Local {
        let d = self.local(I64);
        let entry = &mut self.blocks[0];
        entry.insts.insert(
            0,
            Inst::Copy {
                dst: d,
                src: Operand::Int(init),
            },
        );
        entry.lines.insert(0, 0);
        d
    }

    /// Whether a value of the call through `Remote` being made was rejected by the codec: set
    /// by `wire` and `wire_reply`, read and cleared by `admit` and `arrive`, which come after
    /// them in the same body (R408).
    fn mismatch(&mut self) -> Local {
        match self.mismatch {
            Some(l) => l,
            None => {
                let l = self.entry_local(0);
                self.mismatch = Some(l);
                l
            }
        }
    }

    /// `with cluster = sim(..):` opens a supervisor scope whose nodes 1 to n-1 each get a root
    /// supervisor (D1); it ends as `with supervisor` does.
    pub(super) fn with_cluster(&mut self, h: &y::Atom, body: &'m y::Block) -> R<()> {
        let h = self.scalar1(h)?;
        let depth = self.sup_depth();
        self.emit(Inst::Rt {
            dst: Some(depth),
            f: RtFn::SimBegin,
            args: vec![Operand::Local(depth), h],
        });
        self.block(body)?;
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::SupEnd,
            args: Vec::new(),
        });
        Ok(())
    }

    pub(super) fn cluster_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<&Var>,
    ) -> R<bool> {
        if WIRE_IDS.contains(&id) {
            self.wire(id, c, dst)?;
            return Ok(true);
        }
        let args = Self::value_args(c)?;
        let what = || format!("call of `{}`", c.path.join("."));
        match (id, args.as_slice()) {
            (id::STD_SIM_FAULTS, _) => {
                let Some(d) = dst else { return Ok(true) };
                let Some(fs) = d.shape.fields() else {
                    return unsupported(what());
                };
                let mut v = Vec::new();
                for ((_, _, s), (conv, a)) in fs.iter().zip(&args) {
                    v.extend(self.owned(conv, a, s)?);
                }
                self.copy_to(&d.locals, v);
            }
            (id::STD_SIM_CHECK_TRACE, [(_, trace), (_, spec)]) => {
                let Some(d) = dst else { return Ok(true) };
                let trace = self.scalar1(trace)?;
                let machines = self.atom_any(spec)?.0[0].clone();
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(5)]);
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::SimCheck,
                    args: vec![out.clone(), trace, machines],
                });
                let v = self.read(&At::Mem(out.clone(), 0), &d.shape);
                self.free(RtFn::EnvFree, out);
                self.copy_to(&d.locals, v);
            }
            (id::STD_SIM_SIM, [(_, n), (_, seed), (_, plan)]) => {
                let (n, seed) = (self.scalar1(n)?, self.scalar1(seed)?);
                let s = std_type("FaultPlan").expect("a std type");
                let v = self.atom(plan, &s)?;
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v);
                let sim = self.rt(RtFn::SimNew, I64, vec![n, seed, buf.clone()]);
                // The plan is borrowed: free the block, not the value.
                self.free(RtFn::EnvFree, buf);
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![sim]);
                }
            }
            (id::STD_CLUSTER_NODES, [] | [_]) => {
                let l = self.rt(RtFn::ClusterNodes, Ty::Ptr, Vec::new());
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![l]);
                }
            }
            (id::STD_CLUSTER_SELF_NODE, [] | [_]) => {
                let n = self.rt(RtFn::ClusterSelfNode, Ty::Ptr, Vec::new());
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![n]);
                }
            }
            // The handler is the bound cluster's, which `yan_rt::cluster` keeps.
            (id::STD_CLUSTER_PLACE, [.., (_, node)]) => {
                let node = self.atom(node, &std_type("Node").expect("a std type"))?[0].clone();
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ClusterPlace,
                    args: vec![node],
                });
            }
            (id::STD_CLUSTER_LEAVE, []) => self.emit(Inst::Rt {
                dst: None,
                f: RtFn::ClusterLeave,
                args: Vec::new(),
            }),
            // A `Remote` is its handle; a lost request fails the call at `take` (D5).
            (id::STD_CLUSTER_REACH | id::STD_CLUSTER_HANDLE | id::STD_ACTOR_SHARE, [(_, h)]) => {
                let h = self.scalar1(h)?;
                if id == id::STD_CLUSTER_REACH {
                    self.emit(Inst::Rt {
                        dst: None,
                        f: RtFn::ClusterReach,
                        args: vec![h.clone()],
                    });
                }
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![h]);
                }
            }
            // A lost send fails at `send` (R976).
            (id::STD_CLUSTER_FAULT_SEND, [(_, h)]) => {
                let h = self.scalar1(h)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ClusterFaultSend,
                    args: vec![h],
                });
            }
            (id::STD_CLUSTER_ADMIT, []) => {
                let Some(d) = dst else {
                    return unsupported("`admit` without its result");
                };
                let flag = self.mismatch();
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ClusterAdmit,
                    args: vec![Operand::Local(flag)],
                });
                let ok = Self::variant_value(&d.shape, "Ok", vec![Operand::Unit]);
                self.mismatched(d, ok)?;
            }
            (id::STD_CLUSTER_ARRIVE, [(conv, outcome)]) => {
                let Some(d) = dst else {
                    return unsupported("`arrive` without its result");
                };
                let v = self.owned(conv, outcome, &d.shape)?;
                let shape = d.shape.clone();
                let flag = self.mismatch();
                let lost = self.rt(RtFn::ClusterArrive, I64, vec![Operand::Local(flag)]);
                let rejected = self.mismatched(d, v.clone())?;
                let Some(gone) = shape
                    .variant("Err")
                    .and_then(|(_, _, e)| Self::failure(&e[0].1, "Unreachable", Vec::new()))
                else {
                    return unsupported(
                        "a call through `Remote` whose fail type lacks `Unreachable`",
                    );
                };
                // A lost reply fails the call; the codec never sends one it rejected.
                let (dropped, delivered) = self.branch(lost);
                self.cur = dropped;
                self.copy_to(&d.locals, Self::variant_value(&shape, "Err", gone));
                self.glue(&shape, &v);
                self.goto(delivered);
                self.cur = delivered;
                // A reply the codec rejected is lost; the handler's result goes with it.
                let (drop, kept) = self.branch(rejected);
                self.cur = drop;
                self.glue(&shape, &v);
                self.goto(kept);
                self.cur = kept;
            }
            // The copy replaces its place unless the reply was lost; else the copy is dropped.
            (id::STD_CLUSTER_COMMIT, [(_, y::Atom::Name(n, _)), (_, copy)]) => {
                let place = self.name(n)?.clone();
                let v = self.atom(copy, &place.shape)?;
                let replied = self.rt(RtFn::ClusterReplied, I64, Vec::new());
                let join = self.block_new();
                let (took, lost) = self.branch(replied);
                self.cur = took;
                self.glue(&place.shape, &Self::ops(&place));
                self.copy_to(&place.locals, v.clone());
                self.goto(join);
                self.cur = lost;
                self.glue(&place.shape, &v);
                self.goto(join);
                self.cur = join;
            }
            (id::STD_CLUSTER_COMMIT, _) => {
                return unsupported("an `inout` argument in a call through `Remote` (#929)");
            }
            // The monitor keeps the sender end; one `Stopped` (two slots) arrives on the receiver.
            (id::STD_CLUSTER_MONITOR, [.., (_, h)]) => {
                let Some(d) = dst else {
                    return unsupported("`cluster.monitor` without its result");
                };
                let h = self.scalar1(h)?;
                let ch = self.rt(
                    RtFn::ChanNew,
                    Ty::Ptr,
                    vec![Operand::Int(1), Operand::Int(2)],
                );
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ClusterMonitor,
                    args: vec![h, ch.clone()],
                });
                self.copy_to(&d.locals, vec![ch]);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// `d` takes `Err(SchemaMismatch())` when a value of this call was rejected, else `good`;
    /// the flag is cleared. Returns whether it was rejected.
    fn mismatched(&mut self, d: &Var, good: Vec<Operand>) -> R<Operand> {
        let Some((_, _, err)) = d.shape.variant("Err") else {
            return unsupported("a call through `Remote` that is not a `Result`");
        };
        let Some(e) = Self::failure(&err[0].1, "SchemaMismatch", Vec::new()) else {
            return unsupported("a call through `Remote` whose fail type lacks `SchemaMismatch`");
        };
        let flag = self.mismatch();
        let bad = self.scalar_op(
            ScalarOp::Ne,
            I64,
            vec![Operand::Local(flag), Operand::Int(0)],
        );
        let join = self.block_new();
        let (rejected, fine) = self.branch(bad.clone());
        self.cur = rejected;
        self.copy_to(&d.locals, Self::variant_value(&d.shape, "Err", e));
        self.goto(join);
        self.cur = fine;
        self.copy_to(&d.locals, good);
        self.goto(join);
        self.cur = join;
        self.emit(Inst::Copy {
            dst: flag,
            src: Operand::Int(0),
        });
        Ok(bad)
    }

    fn wire(&mut self, id: registry::Id, c: &y::Call, dst: Option<&Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let ([(_, x), (_, schema)], Some(shape), Some(dst)) = (args.as_slice(), self.targ(c)?, dst)
        else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let v = self.atom(x, &shape)?;
        let schema = self.scalar1(schema)?;
        let n = Operand::Int(v.len() as i128);
        let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![n.clone()]);
        self.write(&At::Mem(buf.clone(), 0), v);
        let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![n]);
        let start = Operand::Int(i128::from(id == id::STD_CLUSTER_WIRE_START));
        let st = self.rt(
            RtFn::WireRoundtrip,
            I64,
            vec![out.clone(), buf.clone(), schema, start],
        );
        // The argument is borrowed: free the block, not the value.
        self.free(RtFn::EnvFree, buf);
        let back = self.read(&At::Mem(out.clone(), 0), &shape);
        self.free(RtFn::EnvFree, out);
        if id == id::STD_CLUSTER_WIRE_SEND {
            self.emit(Inst::Rt {
                dst: None,
                f: RtFn::ClusterSendMismatch,
                args: vec![st],
            });
            self.copy_to(&dst.locals, back);
            return Ok(());
        }
        // A rejected argument or reply fails the call `SchemaMismatch` at `admit` or `arrive`.
        let flag = self.mismatch();
        let any = self.scalar_op(ScalarOp::AddChecked, I64, vec![Operand::Local(flag), st]);
        self.emit(Inst::Copy {
            dst: flag,
            src: any,
        });
        self.copy_to(&dst.locals, back);
        Ok(())
    }
}
