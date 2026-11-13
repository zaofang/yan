//! `env` handlers and zones for native code: handler 0 is the host environment; `fake_env` and
//! `env.only` register entries and return their index (RFC 0019 D6). A `TimeZone` is its interned
//! ID, so equal zones are equal words (R600).

use crate::abi::{List, str_block, yan_rt_list_new, yan_rt_list_push};
use crate::map::{Map, yan_rt_map_push};
use std::collections::HashMap;
use std::sync::Mutex;

enum Env {
    Host,
    Fake {
        args: Vec<String>,
        vars: Vec<(String, String)>,
        zone: i64,
    },
    Only {
        inner: u64,
        vars: Vec<String>,
    },
}

static ENVS: Mutex<Vec<Env>> = Mutex::new(Vec::new());
static ZONES: Mutex<Option<HashMap<String, i64>>> = Mutex::new(None);

fn with_envs<T>(f: impl FnOnce(&mut Vec<Env>) -> T) -> T {
    let mut e = ENVS.lock().expect("env table");
    if e.is_empty() {
        e.push(Env::Host);
    }
    f(&mut e)
}

fn offset_text(mins: i64) -> String {
    format!(
        "{}{:02}:{:02}",
        if mins < 0 { '-' } else { '+' },
        mins.abs() / 60,
        mins.abs() % 60
    )
}

/// `+HH:MM`, in minutes.
fn offset(id: &str) -> Option<i64> {
    let b = id.as_bytes();
    let digits = |s: &[u8]| {
        (s.len() == 2 && s.iter().all(u8::is_ascii_digit))
            .then(|| i64::from((s[0] - b'0') * 10 + (s[1] - b'0')))
    };
    if b.len() != 6 || b[3] != b':' {
        return None;
    }
    let (h, m) = (digits(&b[1..3])?, digits(&b[4..6])?);
    if h > 23 || m > 59 {
        return None;
    }
    Some(if b[0] == b'-' { -1 } else { 1 } * (h * 60 + m))
}

/// The canonical ID of `id` (`UTC`, a fixed `+HH:MM` or an IANA zone of the bundled tzdb).
fn canonical(id: &str) -> Option<String> {
    if id == "UTC" {
        return Some(id.to_owned());
    }
    if id.starts_with(['+', '-']) {
        return offset(id).map(offset_text);
    }
    let tz = jiff::tz::TimeZone::get(id).ok()?;
    Some(tz.iana_name().unwrap_or(id).to_owned())
}

fn intern(id: &str) -> Option<i64> {
    let id = canonical(id)?;
    let mut z = ZONES.lock().expect("zone table");
    let z = z.get_or_insert_with(HashMap::new);
    Some(
        *z.entry(id.clone())
            .or_insert_with(|| Box::into_raw(Box::new(id)) as i64),
    )
}

/// The interned zone of `id`, or 0 when there is none.
///
/// # Safety
/// `id` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_zone_lookup(id: *const String) -> i64 {
    // SAFETY: the caller's contract.
    intern(unsafe { &*id }).unwrap_or(0)
}

/// The ID text of an interned zone.
pub(crate) fn zone_id(z: i64) -> String {
    // SAFETY: zones are leaked `String`s made by `intern`.
    unsafe { &*(z as *const String) }.clone()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_zone_id(z: i64) -> *mut String {
    str_block(&zone_id(z))
}

fn strings(l: *const List) -> Vec<String> {
    // SAFETY: the caller's contract; each element is a `Str` block.
    unsafe { &*l }
        .data
        .iter()
        .map(|&w| unsafe { (*(w as *const String)).clone() })
        .collect()
}

/// `fake_env(args, vars, zone)`, whose arguments the caller still owns.
///
/// # Safety
/// `args` is a `list[str]` block and `vars` a `dict[str, str]` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_env_fake(args: *const List, vars: *const Map, zone: i64) -> u64 {
    let args = strings(args);
    // SAFETY: the caller's contract; entries are two `Str` blocks.
    let mut vars: Vec<(String, String)> = unsafe { &*vars }
        .slots()
        .chunks(2)
        .map(|e| unsafe {
            (
                (*(e[0] as *const String)).clone(),
                (*(e[1] as *const String)).clone(),
            )
        })
        .collect();
    vars.sort();
    with_envs(|e| {
        e.push(Env::Fake { args, vars, zone });
        e.len() as u64 - 1
    })
}

/// `env.only(vars)` over handler `h`.
///
/// # Safety
/// `vars` is a `list[str]` block the caller still owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_env_only(h: u64, vars: *const List) -> u64 {
    let vars = strings(vars);
    with_envs(|e| {
        e.push(Env::Only { inner: h, vars });
        e.len() as u64 - 1
    })
}

fn granted(e: &[Env], h: u64, name: &str) -> bool {
    match &e[h as usize] {
        Env::Only { inner, vars } => vars.iter().any(|v| v == name) && granted(e, *inner, name),
        _ => true,
    }
}

fn base(e: &[Env], h: u64) -> &Env {
    match &e[h as usize] {
        Env::Only { inner, .. } => base(e, *inner),
        b => b,
    }
}

fn str_list(xs: impl Iterator<Item = String>) -> *mut List {
    let l = yan_rt_list_new(1);
    for s in xs {
        // SAFETY: `l` is a fresh list of one-word elements that nothing else borrows.
        unsafe { *yan_rt_list_push(l) = str_block(&s) as u64 };
    }
    l
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_env_args(h: u64) -> *mut List {
    let args = with_envs(|e| match base(e, h) {
        Env::Fake { args, .. } => args.clone(),
        _ => std::env::args().skip(1).collect(),
    });
    str_list(args.into_iter())
}

/// The variable `name`, or null when it is unset or not granted.
///
/// # Safety
/// `name` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_env_var(h: u64, name: *const String) -> *mut String {
    // SAFETY: the caller's contract.
    let name = unsafe { &*name };
    let v = with_envs(|e| {
        if !granted(e, h, name) {
            return None;
        }
        match base(e, h) {
            Env::Fake { vars, .. } => vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
            _ => std::env::var(name).ok(),
        }
    });
    v.map_or(std::ptr::null_mut(), |v| str_block(&v))
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_env_vars(h: u64) -> *mut Map {
    let all = with_envs(|e| {
        let mut vs: Vec<(String, String)> = match base(e, h) {
            Env::Fake { vars, .. } => vars.clone(),
            _ => {
                let mut vs: Vec<_> = std::env::vars_os()
                    .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                    .collect();
                vs.sort();
                vs
            }
        };
        vs.retain(|(k, _)| granted(e, h, k));
        vs
    });
    let m = Map::new("s", 1, 1);
    for (k, v) in all {
        let key = [str_block(&k) as u64];
        // SAFETY: `m` is fresh and keys are distinct; the value slot is valid until the next push.
        unsafe { *yan_rt_map_push(m, key.as_ptr()) = str_block(&v) as u64 };
    }
    m
}

/// `TZ`, then the zone `/etc/localtime` links to, then UTC (R603).
fn host_zone() -> i64 {
    let tz = std::env::var("TZ").ok();
    let link = std::fs::read_link("/etc/localtime").ok().and_then(|p| {
        let s = p.to_str()?.to_owned();
        Some(s.split_once("zoneinfo/")?.1.to_owned())
    });
    tz.as_deref()
        .map(|t| t.trim_start_matches(':'))
        .into_iter()
        .chain(link.as_deref())
        .find_map(intern)
        .or_else(|| intern("UTC"))
        .expect("UTC")
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_env_zone(h: u64) -> i64 {
    match with_envs(|e| match base(e, h) {
        Env::Fake { zone, .. } => Some(*zone),
        _ => None,
    }) {
        Some(z) => z,
        None => host_zone(),
    }
}
