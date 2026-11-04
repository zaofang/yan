//! std.time, exact time: `Duration`, `Instant`, `MonoInstant` and the `time` handlers
//! (R593 to R597).

use std::sync::LazyLock;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Interp, R, Value, panic};
use Value::*;

pub(super) const NS: i128 = 1_000_000_000;
pub(super) const DAY: i128 = 86_400 * NS;

/// Days from 1970-01-01 to the proleptic Gregorian date `y-m-d` (Hinnant's algorithm).
pub(super) fn days_from_civil(y: i128, m: i128, d: i128) -> i128 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(super) fn civil_from_days(z: i128) -> (i128, i128, i128) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i128::from(m <= 2), m, d)
}

fn days_in_month(y: i128, m: i128) -> i128 {
    match m {
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Instants cover years -9999 to 9999 (11.6); a `Duration` spans at most that range.
static MIN: LazyLock<i128> = LazyLock::new(|| days_from_civil(-9999, 1, 1) * DAY);
static MAX: LazyLock<i128> = LazyLock::new(|| days_from_civil(10000, 1, 1) * DAY - 1);

pub(super) fn in_instant(ns: i128) -> Option<i128> {
    (*MIN..=*MAX).contains(&ns).then_some(ns)
}

pub(super) fn in_duration(ns: i128) -> Option<i128> {
    (ns.abs() <= *MAX - *MIN).then_some(ns)
}

pub(super) fn or_overflow(v: Option<i128>, f: fn(i128) -> Value) -> R<Value> {
    match v {
        Some(n) => Ok(f(n)),
        None => panic("overflow"),
    }
}

/// `.5`, `.123456789` or nothing: fractional seconds with trailing zeros dropped.
pub(super) fn frac(ns: i128) -> String {
    if ns == 0 {
        return String::new();
    }
    format!(".{ns:09}").trim_end_matches('0').to_owned()
}

pub(super) fn year(y: i128) -> String {
    if (0..=9999).contains(&y) {
        format!("{y:04}")
    } else {
        format!("{}{:06}", if y < 0 { '-' } else { '+' }, y.abs())
    }
}

pub(super) fn show_instant(ns: i128) -> String {
    let (days, rem) = (ns.div_euclid(DAY), ns.rem_euclid(DAY));
    let (y, m, d) = civil_from_days(days);
    let secs = rem / NS;
    format!(
        "{}-{m:02}-{d:02}T{:02}:{:02}:{:02}{}Z",
        year(y),
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        frac(rem % NS)
    )
}

pub(super) fn show_duration(ns: i128) -> String {
    let a = ns.abs();
    let (h, m, s, f) = (a / (3600 * NS), a / (60 * NS) % 60, a / NS % 60, a % NS);
    let mut out = String::from(if ns < 0 { "-PT" } else { "PT" });
    if h > 0 {
        out += &format!("{h}H");
    }
    if m > 0 {
        out += &format!("{m}M");
    }
    if s > 0 || f > 0 || (h == 0 && m == 0) {
        out += &format!("{s}{}S", frac(f));
    }
    out
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
        Bind::Cap(|it, a| {
            let ns = dur(&a[1]);
            sleep(it, &a[0].clone(), ns);
            Ok(Unit)
        }),
    ),
    (
        STD_TIME_SLEEP_UNTIL,
        Bind::Cap(|it, a| {
            let h = a[0].clone();
            let Mono(now) = monotonic(it, &h) else {
                unreachable!("a reading")
            };
            sleep(it, &h, dur(&a[1]) - now);
            Ok(Unit)
        }),
    ),
];

fn floor_div(x: i128, y: i128) -> i128 {
    let q = x / y;
    if (x % y != 0) && ((x < 0) != (y < 0)) {
        q - 1
    } else {
        q
    }
}

/// `x / n` rounded half to even.
fn div_half_even(x: i128, n: i128) -> i128 {
    let q = floor_div(x, n);
    let r = x - q * n;
    let twice = 2 * r.abs();
    match twice.cmp(&n.abs()) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

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
