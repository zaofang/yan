//! Exact time arithmetic and its text, shared by the interpreter and native code (R593 to R597).

use std::sync::LazyLock;

pub const NS: i128 = 1_000_000_000;

pub const DAY: i128 = 86_400 * NS;

/// Days from 1970-01-01 to the proleptic Gregorian date `y-m-d` (Hinnant's algorithm).
pub fn days_from_civil(y: i128, m: i128, d: i128) -> i128 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i128) -> (i128, i128, i128) {
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

pub fn days_in_month(y: i128, m: i128) -> i128 {
    match m {
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Instants cover years -9999 to 9999 (11.6); a `Duration` spans at most that range.
pub static MIN: LazyLock<i128> = LazyLock::new(|| days_from_civil(-9999, 1, 1) * DAY);

pub static MAX: LazyLock<i128> = LazyLock::new(|| days_from_civil(10000, 1, 1) * DAY - 1);

pub fn in_instant(ns: i128) -> Option<i128> {
    (*MIN..=*MAX).contains(&ns).then_some(ns)
}

pub fn in_duration(ns: i128) -> Option<i128> {
    (ns.abs() <= *MAX - *MIN).then_some(ns)
}

/// `.5`, `.123456789` or nothing: fractional seconds with trailing zeros dropped.
pub fn frac(ns: i128) -> String {
    if ns == 0 {
        return String::new();
    }
    format!(".{ns:09}").trim_end_matches('0').to_owned()
}

pub fn year(y: i128) -> String {
    if (0..=9999).contains(&y) {
        format!("{y:04}")
    } else {
        format!("{}{:06}", if y < 0 { '-' } else { '+' }, y.abs())
    }
}

pub fn show_instant(ns: i128) -> String {
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

pub fn show_duration(ns: i128) -> String {
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

pub fn floor_div(x: i128, y: i128) -> i128 {
    let q = x / y;
    if (x % y != 0) && ((x < 0) != (y < 0)) {
        q - 1
    } else {
        q
    }
}

/// `x / n` rounded half to even.
pub fn div_half_even(x: i128, n: i128) -> i128 {
    let q = floor_div(x, n);
    let r = x - q * n;
    let twice = 2 * r.abs();
    match twice.cmp(&n.abs()) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}
