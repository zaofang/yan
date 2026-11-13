//! Civil time (`PlainDate` and the rest, R598 to R602): every operation is one call of
//! `yan_rt::civil` by name over the operands' words.

use super::*;

fn rec(name: &str, fields: &[(&str, Shape)]) -> Shape {
    Shape::Rec(
        name.into(),
        fields
            .iter()
            .map(|(n, s)| ((*n).into(), s.clone()))
            .collect(),
    )
}

fn sum(name: &str, variants: &[&str]) -> Shape {
    Shape::Sum(
        name.into(),
        variants.iter().map(|v| ((*v).into(), Vec::new())).collect(),
    )
}

/// The shape of a type `std.time` declares for civil time.
pub(super) fn std_type(name: &str) -> Option<Shape> {
    let i = Shape::Scalar(I64);
    Some(match name {
        "PlainDate" => rec(name, &[("days", i)]),
        "PlainTime" => rec(name, &[("ns", i)]),
        "PlainDateTime" => rec(name, &[("days", i.clone()), ("ns", i)]),
        "Period" => rec(
            name,
            &[
                ("years", i.clone()),
                ("months", i.clone()),
                ("weeks", i.clone()),
                ("days", i),
            ],
        ),
        "TimeReason" => sum(name, &["Syntax", "Range", "Zone", "Offset"]),
        "TimeError" => rec(name, &[("at", i), ("reason", std_type("TimeReason")?)]),
        "Disambiguation" => sum(name, &["Compatible", "Earlier", "Later"]),
        "TimeZone" => rec(name, &[("id", i)]),
        "Instant" => super::time::shape(true),
        "MonoInstant" => super::time::wide("MonoInstant"),
        "ZonedDateTime" => rec(
            name,
            &[
                ("instant", super::time::shape(true)),
                ("plain", std_type("PlainDateTime")?),
                ("zone", std_type("TimeZone")?),
            ],
        ),
        _ => return None,
    })
}

/// The success type of a civil time call that fails with `TimeError`, for `?`.
pub(super) fn fail_ok(id: registry::Id) -> Option<&'static str> {
    Some(match id {
        id::STD_TIME_DATE_PARSE | id::STD_TIME_DATE_STRPTIME => "PlainDate",
        id::STD_TIME_PLAIN_TIME_PARSE | id::STD_TIME_PLAIN_TIME_STRPTIME => "PlainTime",
        id::STD_TIME_PLAIN_DATE_TIME_PARSE | id::STD_TIME_PLAIN_DATE_TIME_STRPTIME => {
            "PlainDateTime"
        }
        id::STD_TIME_PERIOD_PARSE => "Period",
        id::STD_TIME_PARSE => "ZonedDateTime",
        id::STD_TIME_ZONE_PARSE => "TimeZone",
        id::STD_TIME_INSTANT_PARSE => "Instant",
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum Kind {
    /// One result, which may panic.
    Plain,
    /// `T | None`.
    Opt,
    /// `T fail TimeError`.
    Fail,
}

/// Which words of a `PlainDateTime` first argument an operation reads.
#[derive(Clone, Copy, PartialEq)]
enum Recv {
    All,
    Date,
    Time,
}

use Kind::{Fail, Opt, Plain};
use Recv::{All, Date, Time};

fn table(id: registry::Id) -> Option<(&'static str, Kind, Recv)> {
    Some(match id {
        id::STD_PRELUDE_PLAINDATE => ("date.new", Plain, All),
        id::STD_TIME_DATE_TRY_NEW => ("date.try_new", Opt, All),
        id::STD_PRELUDE_PLAINTIME => ("time.new", Plain, All),
        id::STD_TIME_PLAIN_TIME_TRY_NEW => ("time.try_new", Opt, All),
        id::STD_PRELUDE_PERIOD => ("period.new", Plain, All),
        id::STD_TIME_DATE_PARSE => ("date.parse", Fail, All),
        id::STD_TIME_PLAIN_TIME_PARSE => ("time.parse", Fail, All),
        id::STD_TIME_PLAIN_DATE_TIME_PARSE => ("datetime.parse", Fail, All),
        id::STD_TIME_PERIOD_PARSE => ("period.parse", Fail, All),
        id::STD_TIME_PARSE => ("zoned.parse", Fail, All),
        id::STD_TIME_ZONE_PARSE => ("zone.parse", Fail, All),
        id::STD_TIME_INSTANT_PARSE => ("instant.parse", Fail, All),
        id::STD_TIME_DATE_STRPTIME => ("date.strptime", Fail, All),
        id::STD_TIME_PLAIN_TIME_STRPTIME => ("time.strptime", Fail, All),
        id::STD_TIME_PLAIN_DATE_TIME_STRPTIME => ("datetime.strptime", Fail, All),
        id::STD_TIME_DATE_YEAR => ("date.year", Plain, All),
        id::STD_TIME_DATE_MONTH => ("date.month", Plain, All),
        id::STD_TIME_DATE_DAY => ("date.day", Plain, All),
        id::STD_TIME_DATE_WEEKDAY => ("date.weekday", Plain, All),
        id::STD_TIME_DATE_DAY_OF_YEAR => ("date.day_of_year", Plain, All),
        id::STD_TIME_DATE_DAYS_IN_MONTH => ("date.days_in_month", Plain, All),
        id::STD_TIME_DATE_IN_LEAP_YEAR => ("date.in_leap_year", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_YEAR => ("date.year", Plain, Date),
        id::STD_TIME_PLAIN_DATE_TIME_MONTH => ("date.month", Plain, Date),
        id::STD_TIME_PLAIN_DATE_TIME_DAY => ("date.day", Plain, Date),
        id::STD_TIME_PLAIN_DATE_TIME_WEEKDAY => ("date.weekday", Plain, Date),
        id::STD_TIME_PLAIN_DATE_TIME_DAY_OF_YEAR => ("date.day_of_year", Plain, Date),
        id::STD_TIME_PLAIN_DATE_TIME_DAYS_IN_MONTH => ("date.days_in_month", Plain, Date),
        id::STD_TIME_PLAIN_DATE_TIME_IN_LEAP_YEAR => ("date.in_leap_year", Plain, Date),
        id::STD_TIME_PLAIN_TIME_HOUR => ("time.hour", Plain, All),
        id::STD_TIME_PLAIN_TIME_MINUTE => ("time.minute", Plain, All),
        id::STD_TIME_PLAIN_TIME_SECOND => ("time.second", Plain, All),
        id::STD_TIME_PLAIN_TIME_NANOSECOND => ("time.nanosecond", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_HOUR => ("time.hour", Plain, Time),
        id::STD_TIME_PLAIN_DATE_TIME_MINUTE => ("time.minute", Plain, Time),
        id::STD_TIME_PLAIN_DATE_TIME_SECOND => ("time.second", Plain, Time),
        id::STD_TIME_PLAIN_DATE_TIME_NANOSECOND => ("time.nanosecond", Plain, Time),
        id::STD_TIME_DATE_ADD => ("date.add", Plain, All),
        id::STD_TIME_DATE_CHECKED_ADD => ("date.checked_add", Opt, All),
        id::STD_TIME_DATE_UNTIL => ("date.until", Plain, All),
        id::STD_TIME_PLAIN_TIME_ADD => ("time.add", Plain, All),
        id::STD_TIME_PLAIN_TIME_SUB => ("time.sub", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_ADD => ("datetime.add", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_SUB => ("datetime.sub", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_ADD_PERIOD => ("datetime.add_period", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_CHECKED_ADD => ("datetime.checked_add", Opt, All),
        id::STD_TIME_PLAIN_DATE_TIME_UNTIL => ("datetime.until", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_IN_ZONE => ("datetime.in_zone", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_IN_ZONE_EXACT => ("datetime.in_zone_exact", Opt, All),
        id::STD_TIME_DATE_STRFTIME => ("date.strftime", Plain, All),
        id::STD_TIME_PLAIN_TIME_STRFTIME => ("time.strftime", Plain, All),
        id::STD_TIME_PLAIN_DATE_TIME_STRFTIME => ("datetime.strftime", Plain, All),
        id::STD_TIME_STRFTIME => ("zoned.strftime", Plain, All),
        id::STD_TIME_INSTANT_STRFTIME => ("instant.strftime", Plain, All),
        id::STD_TIME_INSTANT_IN_ZONE => ("instant.in_zone", Plain, All),
        id::STD_TIME_OFFSET_SECONDS => ("zoned.offset_seconds", Plain, All),
        id::STD_TIME_ZONED_ADD => ("zoned.add", Plain, All),
        id::STD_TIME_ZONED_SUB => ("zoned.sub", Plain, All),
        id::STD_TIME_ZONED_ADD_PERIOD => ("zoned.add_period", Plain, All),
        id::STD_TIME_ZONE_FIXED => ("zone.fixed", Plain, All),
        _ => return None,
    })
}

/// The clock operations: the name, and whether the call names a `time` handler.
fn clock_table(id: registry::Id) -> Option<(&'static str, bool)> {
    Some(match id {
        id::STD_TIME_NOW => ("clock.now", true),
        id::STD_TIME_MONOTONIC => ("clock.monotonic", true),
        id::STD_TIME_SLEEP => ("clock.sleep", true),
        id::STD_TIME_SLEEP_UNTIL => ("clock.sleep_until", true),
        id::STD_TEST_FIXED_TIME => ("clock.fixed", false),
        id::STD_TEST_VIRTUAL => ("clock.virtual", false),
        id::STD_TIME_MONO_ADD => ("mono.add", false),
        id::STD_TIME_MONO_SUB => ("mono.sub", false),
        id::STD_TIME_MONO_SINCE => ("mono.since", false),
        _ => return None,
    })
}

impl<'a, 'm> Fn<'a, 'm> {
    /// One `yan_rt::civil` call on `words` (at most eight): its status.
    fn civil_raw(&mut self, op: &str, mut words: Vec<Operand>) -> R<Operand> {
        if words.len() > 10 {
            return unsupported(format!("civil time operation `{op}` on this many words"));
        }
        words.resize(10, Operand::Int(0));
        let mut args = vec![self.lit(op)];
        args.extend(words);
        Ok(self.rt(RtFn::Civil, I64, args))
    }

    fn civil_out(&mut self, i: usize, ty: Ty) -> Operand {
        let w = self.rt(RtFn::CivilOut, I64, vec![Operand::Int(i as i128)]);
        if ty == Ty::Bool {
            self.scalar_op(ScalarOp::Ne, I64, vec![w, Operand::Int(0)])
        } else {
            w
        }
    }

    /// Panics `overflow` or `value` when `status` says so; the code after runs on a result.
    fn civil_panics(&mut self, status: &Operand) {
        let bad = self.scalar_op(ScalarOp::Ge, I64, vec![status.clone(), Operand::Int(3)]);
        let (e, o) = self.branch(bad);
        self.cur = e;
        let ov = self.scalar_op(ScalarOp::Eq, I64, vec![status.clone(), Operand::Int(3)]);
        let (e2, o2) = self.branch(ov);
        self.cur = e2;
        self.finish(Term::Panic("overflow"));
        self.cur = o2;
        self.finish(Term::Panic("value"));
        self.cur = o;
    }

    /// The text of civil value `v`, by the operation `op` (`show.date`, ...).
    pub(super) fn civil_show(&mut self, op: &str, v: Vec<Operand>) -> R<Operand> {
        self.civil_raw(op, v)?;
        Ok(self.civil_out(0, Ty::Ptr))
    }

    /// Lowers the civil time calls; false for any other call.
    pub(super) fn civil_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<bool> {
        let args = Self::value_args(c)?;
        match (id, args.as_slice()) {
            (id::STD_PRELUDE_PLAINDATETIME, [(_, d), (_, t)]) => {
                let mut v = self.atom_any(d)?.0;
                v.extend(self.atom_any(t)?.0);
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), v);
                }
                return Ok(true);
            }
            (id::STD_TIME_PLAIN_DATE_TIME_DATE | id::STD_TIME_PLAIN_DATE_TIME_TIME, [(_, d)]) => {
                let v = self.atom_any(d)?.0;
                let at = usize::from(id == id::STD_TIME_PLAIN_DATE_TIME_TIME);
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), vec![v[at].clone()]);
                }
                return Ok(true);
            }
            (
                id::STD_TIME_PERIOD_YEARS
                | id::STD_TIME_PERIOD_MONTHS
                | id::STD_TIME_PERIOD_WEEKS
                | id::STD_TIME_PERIOD_DAYS,
                [(_, p)],
            ) => {
                let v = self.atom_any(p)?.0;
                let at = match id {
                    id::STD_TIME_PERIOD_YEARS => 0,
                    id::STD_TIME_PERIOD_MONTHS => 1,
                    id::STD_TIME_PERIOD_WEEKS => 2,
                    _ => 3,
                };
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), vec![v[at].clone()]);
                }
                return Ok(true);
            }
            _ => {}
        }
        if let Some((op, cap)) = clock_table(id) {
            let Some(dst) = dst else {
                return unsupported(format!("`{op}` without a result"));
            };
            let mut words = Vec::new();
            if cap {
                // An unbound handler is the host clock (0).
                words.push(
                    match c.args.iter().find_map(|a| match a {
                        y::Arg::Cap(n, _) => self.names.get(n),
                        y::Arg::Value(..) => None,
                    }) {
                        Some(v) => Self::ops(v)[0].clone(),
                        None => Operand::Int(0),
                    },
                );
            }
            for (_, a) in &args {
                words.extend(self.time_words(a)?);
            }
            self.civil_raw(op, words)?;
            if dst.shape.variant("Ok").is_some() {
                // `sleep` fails only at a deadline, which native code has none of.
                let ok = Self::variant_value(&dst.shape, "Ok", vec![Operand::Unit]);
                self.copy_to(&dst.locals, ok);
            } else {
                let tys = dst.shape.tys();
                let out: Vec<Operand> = tys
                    .iter()
                    .enumerate()
                    .map(|(i, t)| self.civil_out(i, *t))
                    .collect();
                self.copy_to(&dst.locals, out);
            }
            return Ok(true);
        }
        let Some((op, kind, recv)) = table(id) else {
            return Ok(false);
        };
        let Some(dst) = dst else {
            return unsupported(format!("`{op}` without a result"));
        };
        let disamb = std_type("Disambiguation").expect("known");
        let mut words = Vec::new();
        for (k, (_, a)) in args.iter().enumerate() {
            let mut v = if id == id::STD_TIME_PLAIN_DATE_TIME_IN_ZONE && k == 2 {
                self.atom(a, &disamb)?
            } else {
                self.atom_any(a)?.0
            };
            if k == 0 {
                match recv {
                    Date => v.truncate(1),
                    Time => v = v.split_off(1),
                    All => {}
                }
            }
            words.extend(v);
        }
        let status = self.civil_raw(op, words)?;
        self.civil_panics(&status);
        let join = self.block_new();
        match kind {
            Plain => {
                let tys = dst.shape.tys();
                let out: Vec<Operand> = tys
                    .iter()
                    .enumerate()
                    .map(|(i, t)| self.civil_out(i, *t))
                    .collect();
                self.copy_to(&dst.locals, out);
            }
            Opt | Fail => {
                let (none, some) = if matches!(kind, Opt) {
                    ("None", "Some")
                } else {
                    ("Err", "Ok")
                };
                let fail = self.scalar_op(ScalarOp::Ne, I64, vec![status, Operand::Int(0)]);
                let (e, o) = self.branch(fail);
                self.cur = e;
                let bad = if matches!(kind, Opt) {
                    Vec::new()
                } else {
                    vec![self.civil_out(0, I64), self.civil_out(1, I64)]
                };
                let v = Self::variant_value(&dst.shape, none, bad);
                self.copy_to(&dst.locals, v);
                self.goto(join);
                self.cur = o;
                let Some((_, _, fields)) = dst.shape.variant(some) else {
                    return unsupported(format!("`{op}` with this result"));
                };
                let tys: Vec<Ty> = fields.iter().flat_map(|(_, s)| s.tys()).collect();
                let out: Vec<Operand> = tys
                    .iter()
                    .enumerate()
                    .map(|(i, t)| self.civil_out(i, *t))
                    .collect();
                let v = Self::variant_value(&dst.shape, some, out);
                self.copy_to(&dst.locals, v);
            }
        }
        self.goto(join);
        self.cur = join;
        Ok(true)
    }
}
