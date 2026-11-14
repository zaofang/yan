//! std.time, exact time: `Duration`, `Instant`, `MonoInstant` and the `time` handlers
//! (R593 to R597).

use std::sync::LazyLock;

pub(super) use yan_rt::time::{
    DAY, NS, civil_from_days, days_from_civil, days_in_month, div_half_even, floor_div, frac,
    in_duration, in_instant, show_duration, show_instant, year,
};

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Interp, R, Value, panic};
use Value::*;

pub(super) fn or_overflow(v: Option<i128>, f: fn(i128) -> Value) -> R<Value> {
    match v {
        Some(n) => Ok(f(n)),
        None => panic("overflow"),
    }
}

pub(super) fn error(at: usize, kind: &str) -> Value {
    Data {
        name: "TimeError".into(),
        fields: vec![
            ("at".into(), Int(at as i128)),
            (
                "reason".into(),
                Data {
                    name: kind.into(),
                    fields: Vec::new(),
                },
            ),
        ],
    }
}

/// `invalid time at N` and the rest (R596, R604).
pub(super) fn show_error(fields: &[(super::Name, Value)]) -> String {
    let [(_, Int(at)), (_, Data { name, .. })] = fields else {
        unreachable!("a TimeError")
    };
    let what = match name.as_str() {
        "Syntax" => "invalid time",
        "Range" => "time out of range",
        "Zone" => "unknown time zone",
        _ => "time zone offset mismatch",
    };
    format!("{what} at {at}")
}

/// A strict RFC 3339 reader over chars, failing at the first bad char (R595).
pub(super) struct Reader {
    pub(super) cs: Vec<char>,
    pub(super) i: usize,
}

impl Reader {
    pub(super) fn new(s: &str) -> Reader {
        Reader {
            cs: s.chars().collect(),
            i: 0,
        }
    }

    pub(super) fn syntax<T>(&self) -> Result<T, Value> {
        Err(error(self.i, "Syntax"))
    }

    pub(super) fn digits(&mut self, n: usize) -> Result<i128, Value> {
        let mut v = 0;
        for _ in 0..n {
            match self.cs.get(self.i).and_then(|c| c.to_digit(10)) {
                Some(d) => v = v * 10 + i128::from(d),
                None => return self.syntax(),
            }
            self.i += 1;
        }
        Ok(v)
    }

    pub(super) fn lit(&mut self, c: char) -> Result<(), Value> {
        if self.cs.get(self.i) != Some(&c) {
            return self.syntax();
        }
        self.i += 1;
        Ok(())
    }

    /// A field that must lie in `lo..=hi`, reported at its first char.
    pub(super) fn field(&mut self, n: usize, lo: i128, hi: i128) -> Result<i128, Value> {
        let at = self.i;
        let v = self.digits(n)?;
        if !(lo..=hi).contains(&v) {
            return Err(error(at, "Range"));
        }
        Ok(v)
    }

    /// `YYYY-MM-DD`.
    pub(super) fn date(&mut self) -> Result<(i128, i128, i128), Value> {
        let y = self.digits(4)?;
        self.lit('-')?;
        let m = self.field(2, 1, 12)?;
        self.lit('-')?;
        let d = self.field(2, 1, days_in_month(y, m))?;
        Ok((y, m, d))
    }

    /// `HH:MM:SS` with up to 9 fractional digits; no leap second.
    pub(super) fn clock(&mut self) -> Result<(i128, i128, i128, i128), Value> {
        let h = self.field(2, 0, 23)?;
        self.lit(':')?;
        let mi = self.field(2, 0, 59)?;
        self.lit(':')?;
        let sec = self.field(2, 0, 59)?;
        let mut ns = 0;
        if self.cs.get(self.i) == Some(&'.') {
            self.i += 1;
            let start = self.i;
            while self.cs.get(self.i).is_some_and(char::is_ascii_digit) {
                if self.i - start == 9 {
                    return self.syntax();
                }
                self.i += 1;
            }
            if self.i == start {
                return self.syntax();
            }
            let text: String = self.cs[start..self.i].iter().collect();
            ns = format!("{text:0<9}").parse().expect("digits");
        }
        Ok((h, mi, sec, ns))
    }

    /// `Z` (None) or `+HH:MM`, in minutes.
    pub(super) fn offset(&mut self) -> Result<Option<i128>, Value> {
        match self.cs.get(self.i) {
            Some('Z') => {
                self.i += 1;
                Ok(None)
            }
            Some(c @ ('+' | '-')) => {
                let sign = if *c == '-' { -1 } else { 1 };
                self.i += 1;
                let oh = self.field(2, 0, 23)?;
                self.lit(':')?;
                let om = self.field(2, 0, 59)?;
                Ok(Some(sign * (oh * 60 + om)))
            }
            _ => self.syntax(),
        }
    }

    /// An RFC 9557 `[tag]`, without a critical `!`.
    pub(super) fn tag(&mut self) -> Result<String, Value> {
        self.lit('[')?;
        let Some(len) = self.cs[self.i..].iter().position(|c| *c == ']') else {
            return self.syntax();
        };
        let tag: String = self.cs[self.i..self.i + len].iter().collect();
        self.i += len + 1;
        Ok(tag.strip_prefix('!').unwrap_or(&tag).to_owned())
    }

    pub(super) fn end(&self) -> Result<(), Value> {
        if self.i != self.cs.len() {
            return self.syntax();
        }
        Ok(())
    }
}

fn parse_instant(s: &str) -> Result<Value, Value> {
    let mut r = Reader::new(s);
    let (y, m, d) = r.date()?;
    r.lit('T')?;
    let (h, mi, sec, ns) = r.clock()?;
    let off_at = r.i;
    let offset = r.offset()?.unwrap_or(0);
    let off_text: String = r.cs[off_at..r.i].iter().collect();
    if r.cs.get(r.i) == Some(&'[') {
        let at = r.i;
        let tag = r.tag()?;
        if !(matches!(tag.as_str(), "UTC" | "Etc/UTC") || tag == off_text) {
            return Err(error(at, "Zone"));
        }
    }
    r.end()?;
    let secs = days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60 + sec - offset * 60;
    in_instant(secs * NS + ns)
        .map(Instant)
        .ok_or_else(|| error(0, "Range"))
}

pub(super) fn dur(v: &Value) -> i128 {
    match v {
        Duration(n) | Instant(n) | Mono(n) => *n,
        _ => unreachable!("a time value"),
    }
}

/// A `fixed_time` or `virtual` handler: its start and the index of its clock in the interpreter.
fn handler(it: &mut Interp, name: &str, start: &Value) -> Value {
    it.clocks.push(0);
    Data {
        name: name.into(),
        fields: vec![
            ("start".into(), start.clone()),
            ("clock".into(), Int(it.clocks.len() as i128 - 1)),
        ],
    }
}

/// Nanoseconds slept under handler `h`, or `None` for the host clock.
fn slept<'a>(it: &'a mut Interp<'_>, h: &Value) -> Option<&'a mut i128> {
    match h {
        Data { fields, .. } => match fields[1].1 {
            Int(c) => it.clocks.get_mut(c as usize),
            _ => None,
        },
        _ => None,
    }
}

fn now(it: &mut Interp, h: &Value) -> Value {
    match h {
        Data { name, fields } if name == "FixedTime" => fields[0].1.clone(),
        Data { fields, .. } => {
            let start = dur(&fields[0].1);
            Instant(start + slept(it, h).map_or(0, |s| *s))
        }
        _ => {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as i128);
            Instant(t)
        }
    }
}

static START: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);

/// The monotonic reading std's own timers use: the virtual clock when one is bound.
pub(crate) fn mono_now(it: &Interp) -> i128 {
    it.clocks
        .last()
        .copied()
        .unwrap_or_else(|| START.elapsed().as_nanos() as i128)
}

fn monotonic(it: &mut Interp, h: &Value) -> Value {
    match slept(it, h) {
        Some(s) => Mono(*s),
        None => Mono(START.elapsed().as_nanos() as i128),
    }
}

fn sleep(it: &mut Interp, h: &Value, ns: i128) {
    if ns <= 0 {
        return;
    }
    match slept(it, h) {
        Some(s) => *s += ns,
        None => std::thread::sleep(std::time::Duration::from_nanos(ns as u64)),
    }
    crate::simfault::sweep(it);
}

/// Sleeps `ns`, or until the enclosing deadline when that comes first; `Err` is `Timeout` (9.4).
fn sleep_in_scope(it: &mut Interp, h: &Value, ns: i128) -> Result<Value, Value> {
    let Mono(now) = monotonic(it, h) else {
        unreachable!("a reading")
    };
    match it.deadline {
        Some(d) if now + ns > d => {
            sleep(it, h, d - now);
            Err(timeout())
        }
        _ => {
            sleep(it, h, ns);
            Ok(Unit)
        }
    }
}

pub(crate) fn timeout() -> Value {
    Data {
        name: "Timeout".to_owned(),
        fields: Vec::new(),
    }
}

/// For an operation that cannot complete, waits until the enclosing deadline and returns
/// `Timeout` (9.4); `None` when no deadline is open. The operation names no clock, so under
/// `fixed_time` or `virtual` handlers every such clock advances to the deadline.
pub(crate) fn wait_out(it: &mut Interp) -> Option<Value> {
    let d = it.deadline?;
    Some(wait_until(it, d))
}

/// Wait until `d`, then fail `Timeout`.
pub(crate) fn wait_until(it: &mut Interp, d: i128) -> Value {
    advance_to(it, d);
    timeout()
}

/// Moves the clock the std timers use to `d`: a real wait, or the virtual clocks jumping.
pub(crate) fn advance_to(it: &mut Interp, d: i128) {
    if it.clocks.is_empty() {
        let now = START.elapsed().as_nanos() as i128;
        if d > now {
            std::thread::sleep(std::time::Duration::from_nanos((d - now) as u64));
        }
    } else {
        it.clocks.iter_mut().for_each(|c| *c = (*c).max(d));
    }
}

/// The host time left before the enclosing deadline, for host I/O that can be cut short. Under
/// `fixed_time` or `virtual` handlers host I/O takes no time, so no deadline bounds it.
pub(crate) fn host_time_left(it: &Interp) -> Option<std::time::Duration> {
    let d = it.deadline.filter(|_| it.clocks.is_empty())?;
    let left = d - START.elapsed().as_nanos() as i128;
    Some(std::time::Duration::from_nanos(left.max(0) as u64))
}

macro_rules! f {
    ($id:ident, |$a:ident| { $($pat:pat => $e:expr),+ $(,)? }) => {
        (
            $id,
            Bind::Plain(|$a| {
                Ok(match &mut *$a {
                    $($pat => $e,)+
                    #[allow(unreachable_patterns)]
                    _ => return bad_args($id),
                })
            }),
        )
    };
}

/// Each `Duration` constructor scales an `int` by its unit.
macro_rules! unit {
    ($id:ident, $scale:expr) => {
        f!($id, |a| { [Int(n)] => or_overflow(n.checked_mul($scale).and_then(in_duration), Duration)? })
    };
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    unit!(STD_TIME_DURATION_OF_NANOS, 1),
    unit!(STD_TIME_DURATION_OF_MICROS, 1_000),
    unit!(STD_TIME_DURATION_OF_MILLIS, 1_000_000),
    unit!(STD_TIME_DURATION_OF_SECONDS, NS),
    unit!(STD_TIME_DURATION_OF_MINUTES, 60 * NS),
    unit!(STD_TIME_DURATION_OF_HOURS, 3600 * NS),
    f!(STD_TIME_DURATION_NANOS, |a| { [Duration(n)] => Int(*n) }),
    f!(STD_TIME_DURATION_SECONDS, |a| { [Duration(n)] => Int(*n / NS) }),
    f!(STD_TIME_DURATION_AS_SECONDS_FLOAT, |a| { [Duration(n)] => Float(*n as f64 / 1e9) }),
    f!(STD_TIME_DURATION_ABS, |a| { [Duration(n)] => Duration(n.abs()) }),
    f!(STD_TIME_DURATION_NEG, |a| { [Duration(n)] => Duration(-*n) }),
    f!(STD_TIME_DURATION_CHECKED_ADD, |a| {
        [Duration(x), Duration(y)] => opt(x.checked_add(*y).and_then(in_duration).map(Duration)),
    }),
    f!(STD_TIME_DURATION_CHECKED_SUB, |a| {
        [Duration(x), Duration(y)] => opt(x.checked_sub(*y).and_then(in_duration).map(Duration)),
    }),
    f!(STD_TIME_DURATION_CHECKED_MUL, |a| {
        [Duration(x), Int(n)] => opt(x.checked_mul(*n).and_then(in_duration).map(Duration)),
    }),
    f!(STD_TIME_DURATION_ADD, |a| {
        [Duration(x), Duration(y)] => or_overflow(x.checked_add(*y).and_then(in_duration), Duration)?,
    }),
    f!(STD_TIME_DURATION_SUB, |a| {
        [Duration(x), Duration(y)] => or_overflow(x.checked_sub(*y).and_then(in_duration), Duration)?,
    }),
    f!(STD_TIME_DURATION_MUL, |a| {
        [Duration(x), Int(n)] => or_overflow(x.checked_mul(*n).and_then(in_duration), Duration)?,
    }),
    f!(STD_TIME_DURATION_FLOORDIV, |a| {
        [Duration(_), Int(0)] => return panic("div_zero"),
        [Duration(x), Int(n)] => Duration(floor_div(*x, *n)),
    }),
    f!(STD_TIME_DURATION_REM, |a| {
        [Duration(_), Int(0)] => return panic("div_zero"),
        [Duration(x), Int(n)] => Duration(*x - *n * floor_div(*x, *n)),
    }),
    f!(STD_TIME_DURATION_DIV, |a| {
        [Duration(_), Int(0)] => return panic("div_zero"),
        [Duration(x), Int(n)] => Duration(div_half_even(*x, *n)),
    }),
    f!(STD_TIME_DURATION_RATIO, |a| {
        [Duration(_), Duration(0)] => return panic("div_zero"),
        [Duration(x), Duration(y)] => Float(*x as f64 / *y as f64),
    }),
    f!(STD_TIME_DURATION_QUOTIENT, |a| {
        [Duration(_), Duration(0)] => return panic("div_zero"),
        [Duration(x), Duration(y)] => Int(floor_div(*x, *y)),
    }),
    f!(STD_TIME_INSTANT_FROM_UNIX_SECONDS, |a| {
        [Int(n)] => or_overflow(n.checked_mul(NS).and_then(in_instant), Instant)?,
    }),
    f!(STD_TIME_INSTANT_FROM_UNIX_NANOS, |a| { [Int(n)] => or_overflow(in_instant(*n), Instant)? }),
    f!(STD_TIME_INSTANT_UNIX_SECONDS, |a| { [Instant(n)] => Int(n.div_euclid(NS)) }),
    f!(STD_TIME_INSTANT_UNIX_NANOS, |a| { [Instant(n)] => Int(*n) }),
    f!(STD_TIME_INSTANT_ADD, |a| { [Instant(x), d] => or_overflow(in_instant(*x + dur(d)), Instant)? }),
    f!(STD_TIME_INSTANT_SUB, |a| { [Instant(x), d] => or_overflow(in_instant(*x - dur(d)), Instant)? }),
    f!(STD_TIME_INSTANT_SINCE, |a| { [Instant(x), Instant(y)] => Duration(*x - *y) }),
    f!(STD_TIME_INSTANT_CHECKED_ADD, |a| {
        [Instant(x), d] => opt(in_instant(*x + dur(d)).map(Instant)),
    }),
    f!(STD_TIME_INSTANT_CHECKED_SUB, |a| {
        [Instant(x), d] => opt(in_instant(*x - dur(d)).map(Instant)),
    }),
    (
        STD_TIME_INSTANT_PARSE,
        Bind::Fallible(|a| match a {
            [Str(s)] => Ok(parse_instant(s)),
            _ => bad_args(STD_TIME_INSTANT_PARSE),
        }),
    ),
    f!(STD_TIME_MONO_ADD, |a| { [Mono(x), d] => Mono(*x + dur(d)) }),
    f!(STD_TIME_MONO_SUB, |a| { [Mono(x), d] => Mono(*x - dur(d)) }),
    f!(STD_TIME_MONO_SINCE, |a| { [Mono(x), Mono(y)] => Duration(*x - *y) }),
    (
        STD_TEST_FIXED_TIME,
        Bind::Cap(|it, a| Ok(handler(it, "FixedTime", &a[0]))),
    ),
    (
        STD_TEST_VIRTUAL,
        Bind::Cap(|it, a| Ok(handler(it, "VirtualTime", &a[0]))),
    ),
    (STD_TIME_NOW, Bind::Cap(|it, a| Ok(now(it, &a[0])))),
    (
        STD_TIME_MONOTONIC,
        Bind::Cap(|it, a| Ok(monotonic(it, &a[0]))),
    ),
    (
        STD_TIME_SLEEP,
        Bind::CapFallible(|it, a| {
            let ns = dur(&a[1]);
            Ok(sleep_in_scope(it, &a[0].clone(), ns))
        }),
    ),
    (
        STD_TIME_SLEEP_UNTIL,
        Bind::CapFallible(|it, a| {
            let h = a[0].clone();
            let Mono(now) = monotonic(it, &h) else {
                unreachable!("a reading")
            };
            Ok(sleep_in_scope(it, &h, dur(&a[1]) - now))
        }),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<String, String> {
        match parse_instant(s) {
            Ok(Instant(n)) => Ok(show_instant(n)),
            Err(Data { fields, .. }) => Err(show_error(&fields)),
            _ => unreachable!(),
        }
    }

    #[test]
    fn instants_round_trip() {
        assert_eq!(p("2000-01-01T00:00:00Z").unwrap(), "2000-01-01T00:00:00Z");
        assert_eq!(
            p("2024-02-29T12:30:00.500+01:00").unwrap(),
            "2024-02-29T11:30:00.5Z"
        );
        assert_eq!(
            p("1969-12-31T23:59:59.999999999Z").unwrap(),
            "1969-12-31T23:59:59.999999999Z"
        );
        assert_eq!(
            p("2000-01-01T00:00:00Z[UTC]").unwrap(),
            "2000-01-01T00:00:00Z"
        );
        assert_eq!(
            p("2023-02-29T00:00:00Z").unwrap_err(),
            "time out of range at 8"
        );
        assert_eq!(p("2000-01-01 00:00:00Z").unwrap_err(), "invalid time at 10");
        assert_eq!(p("2000-01-01T00:00:00z").unwrap_err(), "invalid time at 19");
        assert_eq!(
            p("2000-01-01T00:00:60Z").unwrap_err(),
            "time out of range at 17"
        );
        assert_eq!(
            p("2000-01-01T00:00:00.1234567891Z").unwrap_err(),
            "invalid time at 29"
        );
        assert_eq!(
            p("2000-01-01T00:00:00Z[Europe/Paris]").unwrap_err(),
            "unknown time zone at 20"
        );
    }

    #[test]
    fn durations_show_as_iso() {
        assert_eq!(show_duration(0), "PT0S");
        assert_eq!(show_duration(1_500_000_000), "PT1.5S");
        assert_eq!(show_duration(-(2 * 3600 + 30 * 60) * NS), "-PT2H30M");
        assert_eq!(show_duration(3661 * NS), "PT1H1M1S");
        assert_eq!(div_half_even(5, 2), 2);
        assert_eq!(div_half_even(7, 2), 4);
        assert_eq!(div_half_even(-5, 2), -2);
    }
}
