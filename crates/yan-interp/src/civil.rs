//! std.time, civil time: `PlainDate`, `PlainTime`, `PlainDateTime`, `Period`, `TimeZone` and
//! `ZonedDateTime` (R598 to R602), over Jiff and its bundled tzdb.

use std::rc::Rc;

use jiff::civil::{Date, DateTime, Time};
use jiff::fmt::strtime;
use jiff::tz::{Offset, TimeZone};
use jiff::{Span, Timestamp, Unit, Zoned};
use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::time::{
    DAY, NS, Reader, civil_from_days, days_from_civil, dur, error, frac, in_instant, year,
};
use super::{Name, R, Value, panic};
use Value::*;

/// A zone and the ID it shows; equal by ID (R600).
#[derive(Clone, Debug)]
pub(crate) struct Zone {
    id: Rc<str>,
    tz: TimeZone,
}

impl PartialEq for Zone {
    fn eq(&self, o: &Zone) -> bool {
        self.id == o.id
    }
}

impl Zone {
    pub(super) fn id(&self) -> &str {
        &self.id
    }
}

pub(super) fn utc() -> Value {
    Value::Zone(Rc::new(Zone {
        id: "UTC".into(),
        tz: TimeZone::UTC,
    }))
}

fn offset_text(secs: i128) -> String {
    let a = secs.abs();
    let mut s = format!(
        "{}{:02}:{:02}",
        if secs < 0 { '-' } else { '+' },
        a / 3600,
        a / 60 % 60
    );
    if a % 60 != 0 {
        s += &format!(":{:02}", a % 60);
    }
    s
}

fn fixed(secs: i128) -> Option<Value> {
    let off = Offset::from_seconds(i32::try_from(secs).ok()?).ok()?;
    Some(Value::Zone(Rc::new(Zone {
        id: offset_text(secs).into(),
        tz: TimeZone::fixed(off),
    })))
}

/// An IANA ID in the bundled tzdb, `UTC`, or a fixed `+HH:MM`.
pub(super) fn lookup(id: &str) -> Option<Value> {
    if id == "UTC" {
        return Some(utc());
    }
    if id.starts_with(['+', '-']) {
        let mut r = Reader::new(id);
        let mins = r.offset().ok()??;
        r.end().ok()?;
        return fixed(mins * 60);
    }
    let tz = TimeZone::get(id).ok()?;
    let id = tz.iana_name().unwrap_or(id).into();
    Some(Value::Zone(Rc::new(Zone { id, tz })))
}

fn new_date(y: i128, m: i128, d: i128) -> Option<Date> {
    Date::new(
        i16::try_from(y).ok()?,
        i8::try_from(m).ok()?,
        i8::try_from(d).ok()?,
    )
    .ok()
}

fn new_time(h: i128, mi: i128, s: i128, ns: i128) -> Option<Time> {
    Time::new(
        i8::try_from(h).ok()?,
        i8::try_from(mi).ok()?,
        i8::try_from(s).ok()?,
        i32::try_from(ns).ok()?,
    )
    .ok()
}

fn time_ns(t: Time) -> i128 {
    (i128::from(t.hour()) * 3600 + i128::from(t.minute()) * 60 + i128::from(t.second())) * NS
        + i128::from(t.subsec_nanosecond())
}

fn time_from_ns(ns: i128) -> Time {
    let s = ns / NS;
    new_time(s / 3600, s / 60 % 60, s % 60, ns % NS).expect("within a day")
}

/// Nanoseconds from 1970-01-01T00:00:00 on the wall clock.
fn wall_ns(dt: DateTime) -> i128 {
    let d = dt.date();
    days_from_civil(d.year().into(), d.month().into(), d.day().into()) * DAY + time_ns(dt.time())
}

fn from_wall(ns: i128) -> Option<DateTime> {
    let ns = in_instant(ns)?;
    let (y, m, d) = civil_from_days(ns.div_euclid(DAY));
    Some(new_date(y, m, d)?.to_datetime(time_from_ns(ns.rem_euclid(DAY))))
}

fn span(p: &[i64; 4]) -> Option<Span> {
    Span::new()
        .try_years(p[0])
        .ok()?
        .try_months(p[1])
        .ok()?
        .try_weeks(p[2])
        .ok()?
        .try_days(p[3])
        .ok()
}

fn period(s: Span) -> Value {
    Period([
        s.get_years().into(),
        s.get_months().into(),
        s.get_weeks().into(),
        s.get_days().into(),
    ])
}

fn span_of(v: &Value) -> Option<Span> {
    let Period(p) = v else {
        unreachable!("a Period")
    };
    span(p)
}

fn or_overflow<T>(v: Option<T>, f: fn(T) -> Value) -> R<Value> {
    match v {
        Some(x) => Ok(f(x)),
        None => panic("overflow"),
    }
}

fn or_value<T>(v: Option<T>, f: fn(T) -> Value) -> R<Value> {
    match v {
        Some(x) => Ok(f(x)),
        None => panic("value"),
    }
}

fn record(ns: i128, plain: DateTime, zone: &Rc<Zone>) -> Value {
    Data {
        name: "ZonedDateTime".into(),
        fields: vec![
            ("instant".into(), Instant(ns)),
            ("plain".into(), PlainDateTime(plain)),
            ("zone".into(), Value::Zone(zone.clone())),
        ],
    }
}

fn zoned(ns: i128, zone: &Rc<Zone>) -> R<Value> {
    let Some(ts) = in_instant(ns).and_then(|ns| Timestamp::from_nanosecond(ns).ok()) else {
        return panic("overflow");
    };
    Ok(record(ns, zone.tz.to_datetime(ts), zone))
}

fn from_jiff(z: Result<Zoned, jiff::Error>, zone: &Rc<Zone>) -> R<Value> {
    match z {
        Ok(z) => zoned(z.timestamp().as_nanosecond(), zone),
        Err(_) => panic("overflow"),
    }
}

fn parts(v: &Value) -> (i128, DateTime, &Rc<Zone>) {
    match v {
        Data { fields, .. } => match &fields[..] {
            [
                (_, Instant(ns)),
                (_, PlainDateTime(dt)),
                (_, Value::Zone(z)),
            ] => (*ns, *dt, z),
            _ => unreachable!("a ZonedDateTime"),
        },
        _ => unreachable!("a ZonedDateTime"),
    }
}

fn zone(v: &Value) -> &Rc<Zone> {
    let Value::Zone(z) = v else {
        unreachable!("a TimeZone")
    };
    z
}

fn jiff_zoned(v: &Value) -> Zoned {
    let (ns, _, z) = parts(v);
    Timestamp::from_nanosecond(ns)
        .expect("in range")
        .to_zoned(z.tz.clone())
}

fn in_zone(dt: DateTime, z: &Rc<Zone>, how: &str) -> R<Value> {
    let amb = z.tz.to_ambiguous_zoned(dt);
    let r = match how {
        "Earlier" => amb.earlier(),
        "Later" => amb.later(),
        _ => amb.compatible(),
    };
    from_jiff(r, z)
}

pub(super) fn show_date(d: Date) -> String {
    format!("{}-{:02}-{:02}", year(d.year().into()), d.month(), d.day())
}

pub(super) fn show_time(t: Time) -> String {
    format!(
        "{:02}:{:02}:{:02}{}",
        t.hour(),
        t.minute(),
        t.second(),
        frac(t.subsec_nanosecond().into())
    )
}

pub(super) fn show_datetime(dt: DateTime) -> String {
    format!("{}T{}", show_date(dt.date()), show_time(dt.time()))
}

pub(super) fn show_period(p: &[i64; 4]) -> String {
    if p.iter().all(|n| *n == 0) {
        return "P0D".into();
    }
    let mut s = String::from(if p.iter().any(|n| *n < 0) { "-P" } else { "P" });
    for (n, u) in p.iter().zip(['Y', 'M', 'W', 'D']) {
        if *n != 0 {
            s += &format!("{}{u}", n.unsigned_abs());
        }
    }
    s
}

/// RFC 9557: `2024-03-10T03:00:00-04:00[America/New_York]` (R601).
pub(super) fn show_zoned(v: &Value) -> String {
    let (ns, dt, z) = parts(v);
    format!(
        "{}{}[{}]",
        show_datetime(dt),
        offset_text((wall_ns(dt) - ns) / NS),
        z.id
    )
}

fn clock_date(r: &mut Reader) -> Result<Date, Value> {
    let (y, m, d) = r.date()?;
    Ok(new_date(y, m, d).expect("checked"))
}

fn clock_time(r: &mut Reader) -> Result<Time, Value> {
    let (h, mi, s, ns) = r.clock()?;
    Ok(new_time(h, mi, s, ns).expect("checked"))
}

fn clock_datetime(r: &mut Reader) -> Result<DateTime, Value> {
    let d = clock_date(r)?;
    r.lit('T')?;
    Ok(d.to_datetime(clock_time(r)?))
}

fn whole<T>(
    s: &str,
    f: fn(&mut Reader) -> Result<T, Value>,
    v: fn(T) -> Value,
) -> Result<Value, Value> {
    let mut r = Reader::new(s);
    let x = f(&mut r)?;
    r.end()?;
    Ok(v(x))
}

/// `P1Y2M`, `P3W`, `-P1D`: units in order, at least one, one sign (R599).
fn parse_period(s: &str) -> Result<Value, Value> {
    let mut r = Reader::new(s);
    let neg = r.cs.first() == Some(&'-');
    if neg {
        r.i += 1;
    }
    r.lit('P')?;
    let mut p = [0i64; 4];
    let mut next = 0;
    while r.i < r.cs.len() {
        let start = r.i;
        while r.cs.get(r.i).is_some_and(char::is_ascii_digit) {
            r.i += 1;
        }
        if r.i == start {
            return r.syntax();
        }
        let text: String = r.cs[start..r.i].iter().collect();
        let Ok(n) = text.parse::<i64>() else {
            return Err(error(start, "Range"));
        };
        let u = match r.cs.get(r.i) {
            Some('Y') => 0,
            Some('M') => 1,
            Some('W') => 2,
            Some('D') => 3,
            _ => return r.syntax(),
        };
        if u < next {
            return r.syntax();
        }
        p[u] = if neg { -n } else { n };
        next = u + 1;
        r.i += 1;
    }
    if next == 0 {
        return r.syntax();
    }
    Ok(Period(p))
}

/// RFC 9557 with a required zone; a numeric offset must agree with it (R601).
fn parse_zoned(s: &str) -> Result<Value, Value> {
    let mut r = Reader::new(s);
    let dt = clock_datetime(&mut r)?;
    let off_at = r.i;
    let off = r.offset()?;
    let at = r.i;
    let tag = r.tag()?;
    r.end()?;
    let Some(Value::Zone(z)) = lookup(&tag) else {
        return Err(error(at, "Zone"));
    };
    let wall = wall_ns(dt);
    let ns = wall - off.unwrap_or(0) * 60 * NS;
    let Some(ts) = in_instant(ns).and_then(|ns| Timestamp::from_nanosecond(ns).ok()) else {
        return Err(error(0, "Range"));
    };
    if let Some(mins) = off
        && i128::from(z.tz.to_offset(ts).seconds()) != mins * 60
    {
        return Err(error(off_at, "Offset"));
    }
    Ok(record(ns, z.tz.to_datetime(ts), &z))
}

fn strptime<T>(
    s: &Value,
    fmt: &Value,
    to: fn(&strtime::BrokenDownTime) -> Result<T, jiff::Error>,
    v: fn(T) -> Value,
) -> Result<Value, Value> {
    let (Str(s), Str(fmt)) = (s, fmt) else {
        unreachable!("two strs")
    };
    match strtime::parse(fmt.as_bytes(), s.as_bytes()).and_then(|b| to(&b)) {
        Ok(x) => Ok(v(x)),
        Err(_) => Err(error(0, "Syntax")),
    }
}

fn strftime(fmt: &Value, b: impl Into<strtime::BrokenDownTime>) -> R<Value> {
    let Str(fmt) = fmt else { unreachable!("a str") };
    match strtime::format(fmt.as_bytes(), b) {
        Ok(s) => Ok(Str(s.into())),
        Err(_) => panic("value"),
    }
}

fn day_of(v: &Value) -> Date {
    match v {
        PlainDate(d) => *d,
        PlainDateTime(dt) => dt.date(),
        _ => unreachable!("a date"),
    }
}

fn clock_of(v: &Value) -> Time {
    match v {
        PlainTime(t) => *t,
        PlainDateTime(dt) => dt.time(),
        _ => unreachable!("a time"),
    }
}

fn int(v: &Value) -> i128 {
    let Int(n) = v else { unreachable!("an int") };
    *n
}

macro_rules! read {
    ($id:ident, $of:ident, |$x:ident| $e:expr) => {
        (
            $id,
            Bind::Plain(|a| {
                let $x = $of(&a[0]);
                Ok($e)
            }),
        )
    };
}

macro_rules! fallible {
    ($id:ident, |$a:ident| $e:expr) => {
        (
            $id,
            Bind::Fallible(|$a| match &*$a {
                [Str(_), ..] => Ok($e),
                _ => bad_args($id),
            }),
        )
    };
}

fn named(v: &Value) -> &Name {
    let Data { name, .. } = v else {
        unreachable!("a variant")
    };
    name
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_PRELUDE_PLAINDATE,
        Bind::Plain(|a| or_value(new_date(int(&a[0]), int(&a[1]), int(&a[2])), PlainDate)),
    ),
    (
        STD_TIME_DATE_TRY_NEW,
        Bind::Plain(|a| {
            Ok(opt(
                new_date(int(&a[0]), int(&a[1]), int(&a[2])).map(PlainDate)
            ))
        }),
    ),
    (
        STD_PRELUDE_PLAINTIME,
        Bind::Plain(|a| {
            let t = new_time(int(&a[0]), int(&a[1]), int(&a[2]), int(&a[3]));
            or_value(t, PlainTime)
        }),
    ),
    (
        STD_TIME_PLAIN_TIME_TRY_NEW,
        Bind::Plain(|a| {
            let t = new_time(int(&a[0]), int(&a[1]), int(&a[2]), int(&a[3]));
            Ok(opt(t.map(PlainTime)))
        }),
    ),
    (
        STD_PRELUDE_PLAINDATETIME,
        Bind::Plain(|a| Ok(PlainDateTime(day_of(&a[0]).to_datetime(clock_of(&a[1]))))),
    ),
    (
        STD_PRELUDE_PERIOD,
        Bind::Plain(|a| {
            let ns: Vec<i128> = a.iter().map(int).collect();
            if ns.iter().any(|n| *n < 0) && ns.iter().any(|n| *n > 0) {
                return panic("value");
            }
            let p: Option<Vec<i64>> = ns.iter().map(|n| i64::try_from(*n).ok()).collect();
            or_value(p, |p| Period([p[0], p[1], p[2], p[3]]))
        }),
    ),
    (
        STD_PRELUDE_TIMEZONE,
        Bind::Plain(|a| match &a[0] {
            Str(s) => or_value(lookup(s), |z| z),
            _ => bad_args(STD_PRELUDE_TIMEZONE),
        }),
    ),
    fallible!(STD_TIME_DATE_PARSE, |a| {
        let Str(s) = &a[0] else { unreachable!() };
        whole(s, clock_date, PlainDate)
    }),
    fallible!(STD_TIME_PLAIN_TIME_PARSE, |a| {
        let Str(s) = &a[0] else { unreachable!() };
        whole(s, clock_time, PlainTime)
    }),
    fallible!(STD_TIME_PLAIN_DATE_TIME_PARSE, |a| {
        let Str(s) = &a[0] else { unreachable!() };
        whole(s, clock_datetime, PlainDateTime)
    }),
    fallible!(STD_TIME_PERIOD_PARSE, |a| {
        let Str(s) = &a[0] else { unreachable!() };
        parse_period(s)
    }),
    fallible!(STD_TIME_PARSE, |a| {
        let Str(s) = &a[0] else { unreachable!() };
        parse_zoned(s)
    }),
    fallible!(STD_TIME_ZONE_PARSE, |a| {
        let Str(s) = &a[0] else { unreachable!() };
        lookup(s).ok_or_else(|| error(0, "Zone"))
    }),
    fallible!(STD_TIME_DATE_STRPTIME, |a| strptime(
        &a[0],
        &a[1],
        strtime::BrokenDownTime::to_date,
        PlainDate
    )),
    fallible!(STD_TIME_PLAIN_TIME_STRPTIME, |a| strptime(
        &a[0],
        &a[1],
        strtime::BrokenDownTime::to_time,
        PlainTime
    )),
    fallible!(STD_TIME_PLAIN_DATE_TIME_STRPTIME, |a| strptime(
        &a[0],
        &a[1],
        strtime::BrokenDownTime::to_datetime,
        PlainDateTime
    )),
    read!(STD_TIME_DATE_YEAR, day_of, |d| Int(d.year().into())),
    read!(STD_TIME_DATE_MONTH, day_of, |d| Int(d.month().into())),
    read!(STD_TIME_DATE_DAY, day_of, |d| Int(d.day().into())),
    read!(STD_TIME_DATE_WEEKDAY, day_of, |d| Int(d
        .weekday()
        .to_monday_one_offset()
        .into())),
    read!(STD_TIME_DATE_DAY_OF_YEAR, day_of, |d| Int(d
        .day_of_year()
        .into())),
    read!(STD_TIME_DATE_DAYS_IN_MONTH, day_of, |d| Int(d
        .days_in_month()
        .into())),
    read!(STD_TIME_DATE_IN_LEAP_YEAR, day_of, |d| Bool(
        d.in_leap_year()
    )),
    read!(STD_TIME_PLAIN_DATE_TIME_YEAR, day_of, |d| Int(d
        .year()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_MONTH, day_of, |d| Int(d
        .month()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_DAY, day_of, |d| Int(d
        .day()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_WEEKDAY, day_of, |d| Int(d
        .weekday()
        .to_monday_one_offset()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_DAY_OF_YEAR, day_of, |d| Int(d
        .day_of_year()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_DAYS_IN_MONTH, day_of, |d| Int(d
        .days_in_month()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_IN_LEAP_YEAR, day_of, |d| Bool(
        d.in_leap_year()
    )),
    read!(STD_TIME_PLAIN_DATE_TIME_DATE, day_of, |d| PlainDate(d)),
    read!(STD_TIME_PLAIN_DATE_TIME_TIME, clock_of, |t| PlainTime(t)),
    read!(STD_TIME_PLAIN_TIME_HOUR, clock_of, |t| Int(t.hour().into())),
    read!(STD_TIME_PLAIN_TIME_MINUTE, clock_of, |t| Int(t
        .minute()
        .into())),
    read!(STD_TIME_PLAIN_TIME_SECOND, clock_of, |t| Int(t
        .second()
        .into())),
    read!(STD_TIME_PLAIN_TIME_NANOSECOND, clock_of, |t| Int(t
        .subsec_nanosecond()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_HOUR, clock_of, |t| Int(t
        .hour()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_MINUTE, clock_of, |t| Int(t
        .minute()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_SECOND, clock_of, |t| Int(t
        .second()
        .into())),
    read!(STD_TIME_PLAIN_DATE_TIME_NANOSECOND, clock_of, |t| Int(t
        .subsec_nanosecond()
        .into())),
    (
        STD_TIME_DATE_ADD,
        Bind::Plain(|a| {
            let d = span_of(&a[1]).and_then(|s| day_of(&a[0]).checked_add(s).ok());
            or_overflow(d, PlainDate)
        }),
    ),
    (
        STD_TIME_DATE_CHECKED_ADD,
        Bind::Plain(|a| {
            let d = span_of(&a[1]).and_then(|s| day_of(&a[0]).checked_add(s).ok());
            Ok(opt(d.map(PlainDate)))
        }),
    ),
    (
        STD_TIME_DATE_UNTIL,
        Bind::Plain(|a| {
            let s = day_of(&a[0]).until((Unit::Year, day_of(&a[1])));
            Ok(period(s.expect("dates in range")))
        }),
    ),
    (
        STD_TIME_PLAIN_TIME_ADD,
        Bind::Plain(|a| {
            let ns = time_ns(clock_of(&a[0])) + dur(&a[1]);
            Ok(PlainTime(time_from_ns(ns.rem_euclid(DAY))))
        }),
    ),
    (
        STD_TIME_PLAIN_TIME_SUB,
        Bind::Plain(|a| {
            let ns = time_ns(clock_of(&a[0])) - dur(&a[1]);
            Ok(PlainTime(time_from_ns(ns.rem_euclid(DAY))))
        }),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_ADD,
        Bind::Plain(|a| {
            let PlainDateTime(dt) = &a[0] else {
                return bad_args(STD_TIME_PLAIN_DATE_TIME_ADD);
            };
            or_overflow(from_wall(wall_ns(*dt) + dur(&a[1])), PlainDateTime)
        }),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_SUB,
        Bind::Plain(|a| {
            let PlainDateTime(dt) = &a[0] else {
                return bad_args(STD_TIME_PLAIN_DATE_TIME_SUB);
            };
            or_overflow(from_wall(wall_ns(*dt) - dur(&a[1])), PlainDateTime)
        }),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_ADD_PERIOD,
        Bind::Plain(|a| {
            let PlainDateTime(dt) = &a[0] else {
                return bad_args(STD_TIME_PLAIN_DATE_TIME_ADD_PERIOD);
            };
            let r = span_of(&a[1]).and_then(|s| dt.checked_add(s).ok());
            or_overflow(r, PlainDateTime)
        }),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_CHECKED_ADD,
        Bind::Plain(|a| {
            let PlainDateTime(dt) = &a[0] else {
                return bad_args(STD_TIME_PLAIN_DATE_TIME_CHECKED_ADD);
            };
            let r = span_of(&a[1]).and_then(|s| dt.checked_add(s).ok());
            Ok(opt(r.map(PlainDateTime)))
        }),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_UNTIL,
        Bind::Plain(|a| match &a[..] {
            [PlainDateTime(x), PlainDateTime(y)] => {
                let s = x.until((Unit::Year, *y)).expect("datetimes in range");
                let ns = (i128::from(s.get_hours()) * 3600
                    + i128::from(s.get_minutes()) * 60
                    + i128::from(s.get_seconds()))
                    * NS
                    + i128::from(s.get_milliseconds()) * 1_000_000
                    + i128::from(s.get_microseconds()) * 1_000
                    + i128::from(s.get_nanoseconds());
                Ok(Tuple(vec![period(s), Duration(ns)]))
            }
            _ => bad_args(STD_TIME_PLAIN_DATE_TIME_UNTIL),
        }),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_IN_ZONE,
        Bind::Plain(|a| match &a[..] {
            [PlainDateTime(dt), Value::Zone(z), how] => in_zone(*dt, z, named(how)),
            _ => bad_args(STD_TIME_PLAIN_DATE_TIME_IN_ZONE),
        }),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_IN_ZONE_EXACT,
        Bind::Plain(|a| match &a[..] {
            [PlainDateTime(dt), Value::Zone(z)] => {
                let amb = z.tz.to_ambiguous_zoned(*dt);
                if amb.is_ambiguous() {
                    return Ok(opt(None));
                }
                Ok(opt(Some(from_jiff(amb.unambiguous(), z)?)))
            }
            _ => bad_args(STD_TIME_PLAIN_DATE_TIME_IN_ZONE_EXACT),
        }),
    ),
    (
        STD_TIME_DATE_STRFTIME,
        Bind::Plain(|a| strftime(&a[1], day_of(&a[0]))),
    ),
    (
        STD_TIME_PLAIN_TIME_STRFTIME,
        Bind::Plain(|a| strftime(&a[1], clock_of(&a[0]))),
    ),
    (
        STD_TIME_PLAIN_DATE_TIME_STRFTIME,
        Bind::Plain(|a| match &a[0] {
            PlainDateTime(dt) => strftime(&a[1], *dt),
            _ => bad_args(STD_TIME_PLAIN_DATE_TIME_STRFTIME),
        }),
    ),
    (
        STD_TIME_STRFTIME,
        Bind::Plain(|a| strftime(&a[1], &jiff_zoned(&a[0]))),
    ),
    (
        STD_TIME_INSTANT_STRFTIME,
        Bind::Plain(|a| match &a[0] {
            Instant(ns) => {
                let Ok(ts) = Timestamp::from_nanosecond(*ns) else {
                    return panic("overflow");
                };
                strftime(&a[1], &ts.to_zoned(TimeZone::UTC))
            }
            _ => bad_args(STD_TIME_INSTANT_STRFTIME),
        }),
    ),
    (
        STD_TIME_INSTANT_IN_ZONE,
        Bind::Plain(|a| match &a[..] {
            [Instant(ns), Value::Zone(z)] => zoned(*ns, z),
            _ => bad_args(STD_TIME_INSTANT_IN_ZONE),
        }),
    ),
    (
        STD_TIME_OFFSET_SECONDS,
        Bind::Plain(|a| {
            let (ns, dt, _) = parts(&a[0]);
            Ok(Int((wall_ns(dt) - ns) / NS))
        }),
    ),
    (
        STD_TIME_ZONED_ADD,
        Bind::Plain(|a| {
            let (ns, _, z) = parts(&a[0]);
            zoned(ns + dur(&a[1]), z)
        }),
    ),
    (
        STD_TIME_ZONED_SUB,
        Bind::Plain(|a| {
            let (ns, _, z) = parts(&a[0]);
            zoned(ns - dur(&a[1]), z)
        }),
    ),
    (
        STD_TIME_ZONED_ADD_PERIOD,
        Bind::Plain(|a| {
            let (_, dt, z) = parts(&a[0]);
            match span_of(&a[1]).and_then(|s| dt.checked_add(s).ok()) {
                Some(dt) => in_zone(dt, z, "Compatible"),
                None => panic("overflow"),
            }
        }),
    ),
    read!(STD_TIME_PERIOD_YEARS, period_of, |p| Int(p[0].into())),
    read!(STD_TIME_PERIOD_MONTHS, period_of, |p| Int(p[1].into())),
    read!(STD_TIME_PERIOD_WEEKS, period_of, |p| Int(p[2].into())),
    read!(STD_TIME_PERIOD_DAYS, period_of, |p| Int(p[3].into())),
    (STD_TIME_ZONE_UTC, Bind::Plain(|_| Ok(utc()))),
    (
        STD_TIME_ZONE_FIXED,
        Bind::Plain(|a| or_value(fixed(int(&a[0])), |z| z)),
    ),
    read!(STD_TIME_ZONE_ID, zone, |z| Str(z.id.clone())),
];

fn period_of(v: &Value) -> [i64; 4] {
    let Period(p) = v else {
        unreachable!("a Period")
    };
    *p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::show_error;

    fn shown(r: Result<Value, Value>) -> String {
        match r {
            Ok(Period(p)) => show_period(&p),
            Ok(v) => show_zoned(&v),
            Err(Data { fields, .. }) => show_error(&fields),
            Err(_) => unreachable!(),
        }
    }

    #[test]
    fn periods_show_and_parse_as_iso() {
        assert_eq!(show_period(&[0; 4]), "P0D");
        assert_eq!(show_period(&[1, 2, 0, 0]), "P1Y2M");
        assert_eq!(show_period(&[0, 0, 0, -1]), "-P1D");
        assert_eq!(shown(parse_period("P3W")), "P3W");
        assert_eq!(shown(parse_period("-P1Y2D")), "-P1Y2D");
        assert_eq!(shown(parse_period("P")), "invalid time at 1");
        assert_eq!(shown(parse_period("P1D1Y")), "invalid time at 4");
        assert_eq!(shown(parse_period("PT1H")), "invalid time at 1");
    }

    #[test]
    fn zoned_parse_checks_zone_and_offset() {
        let ny = "2024-03-10T03:00:00-04:00[America/New_York]";
        assert_eq!(shown(parse_zoned(ny)), ny);
        assert_eq!(
            shown(parse_zoned("2024-03-10T07:00:00Z[America/New_York]")),
            ny
        );
        assert_eq!(
            shown(parse_zoned("2024-03-10T03:00:00-05:00[America/New_York]")),
            "time zone offset mismatch at 19"
        );
        assert_eq!(
            shown(parse_zoned("2024-03-10T03:00:00-04:00[Mars/Base]")),
            "unknown time zone at 25"
        );
        assert_eq!(
            shown(parse_zoned("2024-03-10T03:00:00-04:00")),
            "invalid time at 25"
        );
    }

    #[test]
    fn fixed_zones_show_their_offset() {
        let Some(Value::Zone(z)) = fixed(19800) else {
            unreachable!()
        };
        assert_eq!(z.id(), "+05:30");
        assert_eq!(offset_text(-(3600 + 30 * 60 + 15)), "-01:30:15");
    }
}
