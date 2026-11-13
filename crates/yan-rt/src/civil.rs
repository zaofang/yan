//! Civil time for native code (R598 to R602): the operations of `yan-interp`'s `civil.rs` over
//! plain words. A `PlainDate` is its day number from 1970-01-01, a `PlainTime` its nanosecond of
//! the day, a `PlainDateTime` both, a `Period` its four counts, a `Duration` or `Instant` two
//! words (`hi`, `lo`), a `TimeZone` its interned ID and a `ZonedDateTime` the instant, the
//! wall-clock reading and the zone. One entry point runs an operation by name.

use crate::envhandler::{intern, zone_id};
use crate::time::{
    DAY, NS, civil_from_days, days_from_civil, days_in_month, frac, in_instant, show_instant, year,
};
use jiff::civil::{Date, DateTime, Time};
use jiff::fmt::strtime;
use jiff::tz::{Offset, TimeZone};
use jiff::{Span, Timestamp, Unit, Zoned};

/// What an operation came to.
pub enum Done {
    /// The result words.
    Ok(Vec<i64>),
    /// The `None` of an optional result.
    None,
    /// A parser's rejection: the char offset and the `TimeReason` tag.
    Err(i64, i64),
    /// A failure with its words (status 2).
    Fail(Vec<i64>),
    Overflow,
    Value,
}

const SYNTAX: i64 = 0;
const RANGE: i64 = 1;
const ZONE: i64 = 2;
const OFFSET: i64 = 3;

type Fail = (i64, i64);

fn join(hi: i64, lo: i64) -> i128 {
    (i128::from(hi) << 64) | i128::from(lo as u64)
}

fn words(n: i128) -> [i64; 2] {
    [(n >> 64) as i64, n as i64]
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

fn date_of(days: i64) -> Date {
    let (y, m, d) = civil_from_days(days.into());
    new_date(y, m, d).expect("a day in range")
}

fn days_of(d: Date) -> i64 {
    days_from_civil(d.year().into(), d.month().into(), d.day().into()) as i64
}

fn time_ns(t: Time) -> i128 {
    (i128::from(t.hour()) * 3600 + i128::from(t.minute()) * 60 + i128::from(t.second())) * NS
        + i128::from(t.subsec_nanosecond())
}

fn time_of(ns: i64) -> Time {
    let ns = i128::from(ns);
    let s = ns / NS;
    new_time(s / 3600, s / 60 % 60, s % 60, ns % NS).expect("within a day")
}

fn dt_of(a: &[i64]) -> DateTime {
    date_of(a[0]).to_datetime(time_of(a[1]))
}

fn dt_words(dt: DateTime) -> Vec<i64> {
    vec![days_of(dt.date()), time_ns(dt.time()) as i64]
}

fn wall_ns(dt: DateTime) -> i128 {
    i128::from(days_of(dt.date())) * DAY + time_ns(dt.time())
}

fn from_wall(ns: i128) -> Option<DateTime> {
    let ns = in_instant(ns)?;
    let (y, m, d) = civil_from_days(ns.div_euclid(DAY));
    Some(new_date(y, m, d)?.to_datetime(time_of(ns.rem_euclid(DAY) as i64)))
}

fn span(p: &[i64]) -> Option<Span> {
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

fn period(s: Span) -> Vec<i64> {
    vec![
        s.get_years().into(),
        s.get_months().into(),
        s.get_weeks().into(),
        s.get_days().into(),
    ]
}

fn tz(z: i64) -> TimeZone {
    let id = zone_id(z);
    if id == "UTC" {
        return TimeZone::UTC;
    }
    if id.starts_with(['+', '-']) {
        let b = id.as_bytes();
        let n = |s: &str| s.parse::<i32>().expect("digits");
        let secs = (n(&id[1..3]) * 60 + n(&id[4..6])) * 60;
        let secs = if b[0] == b'-' { -secs } else { secs };
        return TimeZone::fixed(Offset::from_seconds(secs).expect("in range"));
    }
    TimeZone::get(&id).expect("an interned zone")
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

/// A `ZonedDateTime` as its words: the instant, the wall-clock reading and the zone.
fn zoned_words(ns: i128, z: i64) -> Option<Vec<i64>> {
    let ts = Timestamp::from_nanosecond(in_instant(ns)?).ok()?;
    let mut w = words(ns).to_vec();
    w.extend(dt_words(tz(z).to_datetime(ts)));
    w.push(z);
    Some(w)
}

fn from_jiff(z: Result<Zoned, jiff::Error>, zone: i64) -> Done {
    match z
        .ok()
        .and_then(|z| zoned_words(z.timestamp().as_nanosecond(), zone))
    {
        Some(w) => Done::Ok(w),
        None => Done::Overflow,
    }
}

fn jiff_zoned(a: &[i64]) -> Zoned {
    Timestamp::from_nanosecond(join(a[0], a[1]))
        .expect("in range")
        .to_zoned(tz(a[4]))
}

fn in_zone(dt: DateTime, z: i64, how: i64) -> Done {
    let amb = tz(z).to_ambiguous_zoned(dt);
    from_jiff(
        match how {
            1 => amb.earlier(),
            2 => amb.later(),
            _ => amb.compatible(),
        },
        z,
    )
}

fn show_date(d: Date) -> String {
    format!("{}-{:02}-{:02}", year(d.year().into()), d.month(), d.day())
}

fn show_time(t: Time) -> String {
    format!(
        "{:02}:{:02}:{:02}{}",
        t.hour(),
        t.minute(),
        t.second(),
        frac(t.subsec_nanosecond().into())
    )
}

fn show_datetime(dt: DateTime) -> String {
    format!("{}T{}", show_date(dt.date()), show_time(dt.time()))
}

fn show_period(p: &[i64]) -> String {
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

fn show_zoned(a: &[i64]) -> String {
    let (ns, dt) = (join(a[0], a[1]), dt_of(&a[2..4]));
    format!(
        "{}{}[{}]",
        show_datetime(dt),
        offset_text((wall_ns(dt) - ns) / NS),
        zone_id(a[4])
    )
}

fn show_error(at: i64, kind: i64) -> String {
    let what = match kind {
        SYNTAX => "invalid time",
        RANGE => "time out of range",
        ZONE => "unknown time zone",
        _ => "time zone offset mismatch",
    };
    format!("{what} at {at}")
}

/// A strict RFC 3339 reader over chars, failing at the first bad char (R595).
struct Reader {
    cs: Vec<char>,
    i: usize,
}

impl Reader {
    fn new(s: &str) -> Reader {
        Reader {
            cs: s.chars().collect(),
            i: 0,
        }
    }

    fn syntax<T>(&self) -> Result<T, Fail> {
        Err((self.i as i64, SYNTAX))
    }

    fn digits(&mut self, n: usize) -> Result<i128, Fail> {
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

    fn lit(&mut self, c: char) -> Result<(), Fail> {
        if self.cs.get(self.i) != Some(&c) {
            return self.syntax();
        }
        self.i += 1;
        Ok(())
    }

    fn field(&mut self, n: usize, lo: i128, hi: i128) -> Result<i128, Fail> {
        let at = self.i;
        let v = self.digits(n)?;
        if !(lo..=hi).contains(&v) {
            return Err((at as i64, RANGE));
        }
        Ok(v)
    }

    fn date(&mut self) -> Result<Date, Fail> {
        let y = self.digits(4)?;
        self.lit('-')?;
        let m = self.field(2, 1, 12)?;
        self.lit('-')?;
        let d = self.field(2, 1, days_in_month(y, m))?;
        Ok(new_date(y, m, d).expect("checked"))
    }

    fn clock(&mut self) -> Result<Time, Fail> {
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
        Ok(new_time(h, mi, sec, ns).expect("checked"))
    }

    /// `Z` (None) or `+HH:MM`, in minutes.
    fn offset(&mut self) -> Result<Option<i128>, Fail> {
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
    fn tag(&mut self) -> Result<String, Fail> {
        self.lit('[')?;
        let Some(len) = self.cs[self.i..].iter().position(|c| *c == ']') else {
            return self.syntax();
        };
        let tag: String = self.cs[self.i..self.i + len].iter().collect();
        self.i += len + 1;
        Ok(tag.strip_prefix('!').unwrap_or(&tag).to_owned())
    }

    fn end(&self) -> Result<(), Fail> {
        if self.i != self.cs.len() {
            return self.syntax();
        }
        Ok(())
    }

    fn datetime(&mut self) -> Result<DateTime, Fail> {
        let d = self.date()?;
        self.lit('T')?;
        Ok(d.to_datetime(self.clock()?))
    }
}

/// `P1Y2M`, `P3W`, `-P1D`: units in order, at least one, one sign (R599).
fn parse_period(s: &str) -> Result<Vec<i64>, Fail> {
    let mut r = Reader::new(s);
    let neg = r.cs.first() == Some(&'-');
    if neg {
        r.i += 1;
    }
    r.lit('P')?;
    let mut p = vec![0i64; 4];
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
            return Err((start as i64, RANGE));
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
    Ok(p)
}

/// RFC 9557 with a required zone; a numeric offset must agree with it (R601).
fn parse_zoned(s: &str) -> Result<Vec<i64>, Fail> {
    let mut r = Reader::new(s);
    let dt = r.datetime()?;
    let off_at = r.i;
    let off = r.offset()?;
    let at = r.i;
    let tag = r.tag()?;
    r.end()?;
    let Some(z) = intern(&tag) else {
        return Err((at as i64, ZONE));
    };
    let ns = wall_ns(dt) - off.unwrap_or(0) * 60 * NS;
    let Some(ts) = in_instant(ns).and_then(|ns| Timestamp::from_nanosecond(ns).ok()) else {
        return Err((0, RANGE));
    };
    if let Some(mins) = off
        && i128::from(tz(z).to_offset(ts).seconds()) != mins * 60
    {
        return Err((off_at as i64, OFFSET));
    }
    Ok(zoned_words(ns, z).expect("in range"))
}

fn parse_instant(s: &str) -> Result<Vec<i64>, Fail> {
    let mut r = Reader::new(s);
    let dt = r.datetime()?;
    let off_at = r.i;
    let offset = r.offset()?.unwrap_or(0);
    let off_text: String = r.cs[off_at..r.i].iter().collect();
    if r.cs.get(r.i) == Some(&'[') {
        let at = r.i;
        let tag = r.tag()?;
        if !(matches!(tag.as_str(), "UTC" | "Etc/UTC") || tag == off_text) {
            return Err((at as i64, ZONE));
        }
    }
    r.end()?;
    in_instant(wall_ns(dt) - offset * 60 * NS)
        .map(|ns| words(ns).to_vec())
        .ok_or((0, RANGE))
}

fn strptime<T>(
    s: &str,
    fmt: &str,
    to: fn(&strtime::BrokenDownTime) -> Result<T, jiff::Error>,
) -> Result<T, Fail> {
    strtime::parse(fmt.as_bytes(), s.as_bytes())
        .and_then(|b| to(&b))
        .map_err(|_| (0, SYNTAX))
}

fn strftime(fmt: &str, b: impl Into<strtime::BrokenDownTime>) -> Done {
    match strtime::format(fmt.as_bytes(), b) {
        Ok(s) => Done::Ok(vec![crate::abi::str_block(&s) as i64]),
        Err(_) => Done::Value,
    }
}

fn text(w: i64) -> &'static str {
    // SAFETY: a `Str` argument is a live `String` block for the call.
    unsafe { &*(w as *const String) }
}

fn str_out(s: &str) -> Done {
    Done::Ok(vec![crate::abi::str_block(s) as i64])
}

fn lift<T>(r: Result<T, Fail>, f: impl FnOnce(T) -> Vec<i64>) -> Done {
    match r {
        Ok(x) => Done::Ok(f(x)),
        Err((at, kind)) => Done::Err(at, kind),
    }
}

fn opt(v: Option<Vec<i64>>) -> Done {
    v.map_or(Done::None, Done::Ok)
}

fn int(n: impl Into<i64>) -> Done {
    Done::Ok(vec![n.into()])
}

fn or_overflow(v: Option<Vec<i64>>) -> Done {
    v.map_or(Done::Overflow, Done::Ok)
}

/// The words of a value described by `d` from `pos`: `s` str, `p` path, `i` int, `f` float, `b`
/// bool, `n` unit, `L` list of the next description, `{...}` a record of field descriptions, and
/// `<name|fields;...>` a sum of variants: the tag, then every variant's fields (zeros unless
/// selected).
fn width(d: &[u8], pos: &mut usize) -> usize {
    let c = d[*pos];
    *pos += 1;
    match c {
        b'L' => {
            width(d, pos);
            1
        }
        b'{' => {
            let mut w = 0;
            while d[*pos] != b'}' {
                w += width(d, pos);
            }
            *pos += 1;
            w
        }
        b'<' => {
            let mut w = 1;
            while d[*pos] != b'>' {
                while d[*pos] != b'|' {
                    *pos += 1;
                }
                *pos += 1;
                while d[*pos] != b';' {
                    w += width(d, pos);
                }
                *pos += 1;
            }
            *pos += 1;
            w
        }
        _ => 1,
    }
}

fn encode(v: &crate::cli::V, d: &str, pos: &mut usize, out: &mut Vec<i64>) {
    use crate::cli::V;
    let d = d.as_bytes();
    let c = d[*pos];
    *pos += 1;
    match (c, v) {
        (b's', V::Str(s)) => out.push(crate::abi::str_block(s) as i64),
        (b'p', V::Path(s)) => {
            let tmp = crate::abi::str_block(s);
            // SAFETY: `tmp` is a fresh `Str` block.
            let p = unsafe { crate::pathabi::yan_rt_path_new(tmp) };
            // SAFETY: as above; the original block is not used again.
            unsafe { crate::abi::yan_rt_str_free(tmp) };
            out.push(p as i64);
        }
        (b'i', V::Int(n)) => out.push(*n),
        (b'f', V::Float(x)) => out.push(x.to_bits() as i64),
        (b'b', V::Bool(b)) => out.push(i64::from(*b)),
        (b'n', _) => out.push(0),
        (b'L', V::List(xs)) => {
            let start = *pos;
            let w = width(d, &mut { start });
            let l = crate::abi::yan_rt_list_new(w as u64);
            for x in xs {
                let mut slots = Vec::new();
                *pos = start;
                encode(x, std::str::from_utf8(d).expect("ascii"), pos, &mut slots);
                // SAFETY: `l` is a fresh list of `w`-word elements that nothing else borrows.
                let e = unsafe { crate::abi::yan_rt_list_push(l) };
                for (i, s) in slots.iter().enumerate() {
                    // SAFETY: the element has `w` slots.
                    unsafe { *e.add(i) = *s as u64 };
                }
            }
            *pos = start;
            width(d, pos);
            out.push(l as i64);
        }
        (b'{', V::Rec(_, fields)) => {
            for (_, f) in fields {
                encode(f, std::str::from_utf8(d).expect("ascii"), pos, out);
            }
            assert_eq!(d[*pos], b'}');
            *pos += 1;
        }
        (b'<', _) => {
            let (name, fields): (&str, &[(String, V)]) = match v {
                V::Opt(None) => ("None", &[]),
                V::Opt(Some(_)) => ("Some", &[]),
                V::Variant(n) => (n, &[]),
                V::Rec(n, fs) => (n, fs),
                _ => panic!("cli value for a sum"),
            };
            let tag_at = out.len();
            out.push(0);
            let mut tag = 0;
            let mut i = 0;
            while d[*pos] != b'>' {
                let end = d[*pos..]
                    .iter()
                    .position(|b| *b == b'|')
                    .expect("variant name")
                    + *pos;
                let this = std::str::from_utf8(&d[*pos..end]).expect("ascii");
                *pos = end + 1;
                if this == name {
                    tag = i;
                    let mut k = 0;
                    while d[*pos] != b';' {
                        match v {
                            V::Opt(Some(x)) => {
                                encode(x, std::str::from_utf8(d).expect("ascii"), pos, out)
                            }
                            _ => {
                                encode(
                                    &fields[k].1,
                                    std::str::from_utf8(d).expect("ascii"),
                                    pos,
                                    out,
                                );
                                k += 1;
                            }
                        }
                    }
                } else {
                    while d[*pos] != b';' {
                        let w = width(d, pos);
                        out.extend(std::iter::repeat_n(0, w));
                    }
                }
                *pos += 1;
                i += 1;
            }
            *pos += 1;
            out[tag_at] = tag;
        }
        _ => panic!("cli value `{v:?}` for description `{}`", c as char),
    }
}

/// A `fixed_time` or `virtual` handler's clock: its start and the nanoseconds slept under it.
struct Clock {
    start: i128,
    fixed: bool,
    slept: i128,
}

static CLOCKS: std::sync::Mutex<Vec<Clock>> = std::sync::Mutex::new(Vec::new());
static START: std::sync::LazyLock<std::time::Instant> =
    std::sync::LazyLock::new(std::time::Instant::now);

fn host_monotonic() -> i128 {
    START.elapsed().as_nanos() as i128
}

/// Handler `h` (0 is the host clock): its `now` and `monotonic` readings.
fn clock_now(h: i64) -> i128 {
    let c = CLOCKS.lock().expect("clock table");
    match h.checked_sub(1).and_then(|i| c.get(i as usize)) {
        Some(c) if c.fixed => c.start,
        Some(c) => c.start + c.slept,
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i128),
    }
}

fn clock_monotonic(h: i64) -> i128 {
    let c = CLOCKS.lock().expect("clock table");
    match h.checked_sub(1).and_then(|i| c.get(i as usize)) {
        Some(c) => c.slept,
        None => host_monotonic(),
    }
}

/// The monotonic reading `std.sim` times its faults by: the innermost handler's, else the host's.
pub fn mono_now() -> i128 {
    clocks_last().unwrap_or_else(host_monotonic)
}

/// The monotonic reading of the innermost `fixed_time` or `virtual` handler made, if any.
pub fn clocks_last() -> Option<i128> {
    CLOCKS.lock().expect("clock table").last().map(|c| c.slept)
}

/// A wait that nothing can end runs every handler clock up to `d`, as host time would pass.
pub fn clocks_advance_to(d: i128) {
    for c in CLOCKS.lock().expect("clock table").iter_mut() {
        c.slept = c.slept.max(d);
    }
}

fn clock_sleep(h: i64, ns: i128) {
    if ns <= 0 {
        return;
    }
    let mut c = CLOCKS.lock().expect("clock table");
    match h.checked_sub(1).and_then(|i| c.get_mut(i as usize)) {
        Some(c) => c.slept += ns,
        None => {
            drop(c);
            std::thread::sleep(std::time::Duration::from_nanos(ns as u64));
        }
    }
}

/// Runs operation `op` on the argument words `a`.
pub fn run(op: &str, a: &[i64]) -> Done {
    match op {
        "date.new" => match new_date(a[0].into(), a[1].into(), a[2].into()) {
            Some(d) => int(days_of(d)),
            None => Done::Value,
        },
        "date.try_new" => {
            opt(new_date(a[0].into(), a[1].into(), a[2].into()).map(|d| vec![days_of(d)]))
        }
        "time.new" | "time.try_new" => {
            let t = new_time(a[0].into(), a[1].into(), a[2].into(), a[3].into());
            let r = t.map(|t| vec![time_ns(t) as i64]);
            if op == "time.new" {
                r.map_or(Done::Value, Done::Ok)
            } else {
                opt(r)
            }
        }
        "period.new" => {
            if a.iter().any(|n| *n < 0) && a.iter().any(|n| *n > 0) {
                return Done::Value;
            }
            Done::Ok(a.to_vec())
        }
        _ => run_more(op, a),
    }
}

fn whole<T>(s: &str, f: impl FnOnce(&mut Reader) -> Result<T, Fail>) -> Result<T, Fail> {
    let mut r = Reader::new(s);
    let x = f(&mut r)?;
    r.end()?;
    Ok(x)
}

fn run_more(op: &str, a: &[i64]) -> Done {
    match op {
        "date.parse" => lift(whole(text(a[0]), Reader::date), |d| vec![days_of(d)]),
        "time.parse" => lift(
            whole(text(a[0]), Reader::clock),
            |t| vec![time_ns(t) as i64],
        ),
        "datetime.parse" => lift(whole(text(a[0]), Reader::datetime), dt_words),
        "period.parse" => lift(parse_period(text(a[0])), |p| p),
        "zoned.parse" => lift(parse_zoned(text(a[0])), |w| w),
        "cli.parse" => {
            // SAFETY: `a` holds a `list[str]` block, a `Str` schema and a `Str` descriptor.
            let args: Vec<String> = unsafe { &*(a[0] as *const crate::abi::List) }
                .data
                .iter()
                .map(|&w| text(w as i64).to_owned())
                .collect();
            match crate::cli::parse_args(&args, text(a[1])) {
                Ok(v) => {
                    let mut out = Vec::new();
                    encode(&v, text(a[2]), &mut 0, &mut out);
                    Done::Ok(out)
                }
                Err(e) => Done::Fail(vec![
                    crate::abi::str_block(&e.message) as i64,
                    i64::from(e.help),
                ]),
            }
        }
        "clock.fixed" | "clock.virtual" => {
            let mut c = CLOCKS.lock().expect("clock table");
            c.push(Clock {
                start: join(a[0], a[1]),
                fixed: op == "clock.fixed",
                slept: 0,
            });
            int(c.len() as i64)
        }
        "clock.now" => {
            let t = clock_now(a[0]);
            crate::cluster::sweep();
            Done::Ok(words(t).to_vec())
        }
        "clock.monotonic" => {
            let t = clock_monotonic(a[0]);
            crate::cluster::sweep();
            Done::Ok(words(t).to_vec())
        }
        "clock.sleep" => {
            clock_sleep(a[0], join(a[1], a[2]));
            crate::cluster::sweep();
            int(0)
        }
        "clock.sleep_until" => {
            clock_sleep(a[0], join(a[1], a[2]) - clock_monotonic(a[0]));
            crate::cluster::sweep();
            int(0)
        }
        "mono.add" => Done::Ok(words(join(a[0], a[1]) + join(a[2], a[3])).to_vec()),
        "mono.sub" | "mono.since" => Done::Ok(words(join(a[0], a[1]) - join(a[2], a[3])).to_vec()),
        "show.mono" => str_out(&format!("MonoInstant({})", join(a[0], a[1]))),
        "instant.parse" => lift(parse_instant(text(a[0])), |w| w),
        "zone.parse" => match intern(text(a[0])) {
            Some(z) => int(z),
            None => Done::Err(0, ZONE),
        },
        "date.strptime" => lift(
            strptime(text(a[0]), text(a[1]), strtime::BrokenDownTime::to_date),
            |d| vec![days_of(d)],
        ),
        "time.strptime" => lift(
            strptime(text(a[0]), text(a[1]), strtime::BrokenDownTime::to_time),
            |t| vec![time_ns(t) as i64],
        ),
        "datetime.strptime" => lift(
            strptime(text(a[0]), text(a[1]), strtime::BrokenDownTime::to_datetime),
            dt_words,
        ),
        "date.year" => int(date_of(a[0]).year()),
        "date.month" => int(date_of(a[0]).month()),
        "date.day" => int(date_of(a[0]).day()),
        "date.weekday" => int(date_of(a[0]).weekday().to_monday_one_offset()),
        "date.day_of_year" => int(date_of(a[0]).day_of_year()),
        "date.days_in_month" => int(date_of(a[0]).days_in_month()),
        "date.in_leap_year" => int(i64::from(date_of(a[0]).in_leap_year())),
        "time.hour" => int(time_of(a[0]).hour()),
        "time.minute" => int(time_of(a[0]).minute()),
        "time.second" => int(time_of(a[0]).second()),
        "time.nanosecond" => int(time_of(a[0]).subsec_nanosecond()),
        "date.add" => or_overflow(
            span(&a[1..5])
                .and_then(|s| date_of(a[0]).checked_add(s).ok())
                .map(|d| vec![days_of(d)]),
        ),
        "date.checked_add" => opt(span(&a[1..5])
            .and_then(|s| date_of(a[0]).checked_add(s).ok())
            .map(|d| vec![days_of(d)])),
        "date.until" => Done::Ok(period(
            date_of(a[0])
                .until((Unit::Year, date_of(a[1])))
                .expect("dates in range"),
        )),
        "time.add" | "time.sub" => {
            let d = join(a[1], a[2]);
            let d = if op == "time.add" { d } else { -d };
            let ns = (i128::from(a[0]) + d).rem_euclid(DAY);
            int(ns as i64)
        }
        "datetime.add" | "datetime.sub" => {
            let d = join(a[2], a[3]);
            let d = if op == "datetime.add" { d } else { -d };
            or_overflow(from_wall(wall_ns(dt_of(a)) + d).map(dt_words))
        }
        "datetime.add_period" => or_overflow(
            span(&a[2..6])
                .and_then(|s| dt_of(a).checked_add(s).ok())
                .map(dt_words),
        ),
        "datetime.checked_add" => opt(span(&a[2..6])
            .and_then(|s| dt_of(a).checked_add(s).ok())
            .map(dt_words)),
        "datetime.until" => {
            let s = dt_of(a)
                .until((Unit::Year, dt_of(&a[2..4])))
                .expect("datetimes in range");
            let ns = (i128::from(s.get_hours()) * 3600
                + i128::from(s.get_minutes()) * 60
                + i128::from(s.get_seconds()))
                * NS
                + i128::from(s.get_milliseconds()) * 1_000_000
                + i128::from(s.get_microseconds()) * 1_000
                + i128::from(s.get_nanoseconds());
            let mut w = period(s);
            w.extend(words(ns));
            Done::Ok(w)
        }
        "date.strftime" => strftime(text(a[1]), date_of(a[0])),
        "time.strftime" => strftime(text(a[1]), time_of(a[0])),
        "datetime.strftime" => strftime(text(a[2]), dt_of(a)),
        "zoned.strftime" => strftime(text(a[5]), &jiff_zoned(a)),
        "instant.strftime" => match Timestamp::from_nanosecond(join(a[0], a[1])) {
            Ok(ts) => strftime(text(a[2]), &ts.to_zoned(TimeZone::UTC)),
            Err(_) => Done::Overflow,
        },
        "datetime.in_zone" => in_zone(dt_of(a), a[2], a[3]),
        "datetime.in_zone_exact" => {
            let amb = tz(a[2]).to_ambiguous_zoned(dt_of(a));
            if amb.is_ambiguous() {
                return Done::None;
            }
            match from_jiff(amb.unambiguous(), a[2]) {
                Done::Ok(w) => Done::Ok(w),
                other => other,
            }
        }
        "instant.in_zone" => or_overflow(zoned_words(join(a[0], a[1]), a[2])),
        "zoned.offset_seconds" => int(((wall_ns(dt_of(&a[2..4])) - join(a[0], a[1])) / NS) as i64),
        "zoned.add" => or_overflow(zoned_words(join(a[0], a[1]) + join(a[5], a[6]), a[4])),
        "zoned.sub" => or_overflow(zoned_words(join(a[0], a[1]) - join(a[5], a[6]), a[4])),
        "zoned.add_period" => {
            match span(&a[5..9]).and_then(|s| dt_of(&a[2..4]).checked_add(s).ok()) {
                Some(dt) => in_zone(dt, a[4], 0),
                None => Done::Overflow,
            }
        }
        "zone.fixed" => match i32::try_from(a[0])
            .ok()
            .and_then(|s| Offset::from_seconds(s).ok())
            .and_then(|_| intern(&offset_text(a[0].into())))
        {
            Some(z) => int(z),
            None => Done::Value,
        },
        "show.date" => str_out(&show_date(date_of(a[0]))),
        "show.time" => str_out(&show_time(time_of(a[0]))),
        "show.datetime" => str_out(&show_datetime(dt_of(a))),
        "show.period" => str_out(&show_period(a)),
        "show.zoned" => str_out(&show_zoned(a)),
        "show.error" => str_out(&show_error(a[0], a[1])),
        "show.instant" => str_out(&show_instant(join(a[0], a[1]))),
        _ => panic!("civil operation `{op}`"),
    }
}

thread_local! {
    static OUT: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Runs `op` named by the `Str` block `op` on the words `a0` to `a9`: 0 when it produced a result
/// (read with [`yan_rt_civil_out`]), 1 for `None`, 2 for an error (offset and reason in the
/// result), 3 for an `overflow` panic and 4 for a `value` panic.
///
/// # Safety
/// `op` is a `Str` block; any `Str` argument words are live `Str` blocks.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn yan_rt_civil(
    op: *const String,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
    a8: i64,
    a9: i64,
) -> i64 {
    // SAFETY: the caller's contract.
    let op = unsafe { &*op };
    let (status, out) = match run(op, &[a0, a1, a2, a3, a4, a5, a6, a7, a8, a9]) {
        Done::Ok(w) => (0, w),
        Done::None => (1, Vec::new()),
        Done::Err(at, kind) => (2, vec![at, kind]),
        Done::Fail(w) => (2, w),
        Done::Overflow => (3, Vec::new()),
        Done::Value => (4, Vec::new()),
    };
    OUT.with(|o| *o.borrow_mut() = out);
    status
}

/// Float word `i` of the last result.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_civil_out_f(i: i64) -> f64 {
    f64::from_bits(OUT.with(|o| o.borrow()[i as usize]) as u64)
}

/// Word `i` of the last result.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_civil_out(i: i64) -> i64 {
    OUT.with(|o| o.borrow()[i as usize])
}
