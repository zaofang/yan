//! std.path: lexical `Path` (`/` on every host) and `WindowsPath` text (R587 to R592).

use std::cmp::Ordering;
use std::rc::Rc;

use yan_rt::path::Parsed;
use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{R, Value, panic};
use Value::*;

/// Windows path text; equal, ordered and hashed by its case-folded form.
#[derive(Clone, Debug)]
pub(crate) struct WinPath {
    pub text: Rc<str>,
    folded: Rc<str>,
}

impl PartialEq for WinPath {
    fn eq(&self, o: &Self) -> bool {
        self.folded == o.folded
    }
}

impl WinPath {
    pub fn folded(&self) -> &str {
        &self.folded
    }
}

/// The `Value` of a parsed path.
trait ParsedValue {
    fn value(&self) -> Value;
}

impl ParsedValue for Parsed {
    fn value(&self) -> Value {
        let t = self.text();
        if self.win {
            WindowsPath(Rc::new(WinPath {
                folded: t.to_lowercase().into(),
                text: t.into(),
            }))
        } else {
            Path(t.into())
        }
    }
}

fn parsed(v: &Value) -> Parsed {
    match v {
        Path(s) => Parsed::posix(s),
        WindowsPath(w) => Parsed::windows(&w.text),
        Str(s) => Parsed::posix(s),
        _ => unreachable!("a path"),
    }
}

/// `show(p)`: the path text.
pub(super) fn text(v: &Value) -> &str {
    match v {
        Path(s) => s,
        WindowsPath(w) => &w.text,
        _ => unreachable!("a path"),
    }
}

/// Paths order by their parts; Windows parts compare case-folded.
pub(super) fn order(a: &Value, b: &Value) -> Ordering {
    parsed(a).folded_parts().cmp(&parsed(b).folded_parts())
}

fn with_name(p: &Parsed, n: &str) -> R<Value> {
    if p.name().is_empty() || n.is_empty() || n == "." || n.contains(['/', '\\']) {
        return panic("value");
    }
    let mut segs = p.segs.clone();
    *segs.last_mut().expect("named") = n.to_owned();
    Ok(p.with_segs(segs).value())
}

fn with_suffix(p: &Parsed, s: &str) -> R<Value> {
    if !s.is_empty() && (!s.starts_with('.') || s == "." || s.contains(['/', '\\'])) {
        return panic("value");
    }
    let n = p.name();
    let stem = &n[..n.len() - p.suffix().len()];
    with_name(p, &format!("{stem}{s}"))
}

fn strs(xs: Vec<String>) -> Value {
    List(xs.into_iter().map(|x| Str(x.into())).collect())
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

/// The binds shared by `Path` and `WindowsPath`, one table per type.
macro_rules! common {
    ($T:ident, $text:ident, $parent:ident, $name:ident, $suffix:ident, $stem:ident, $with_name:ident,
     $with_suffix:ident, $parts:ident, $abs:ident, $rel:ident, $starts:ident, $norm:ident) => {
        [
            f!($text, |a| { [p @ $T(_)] => Str(text(p).into()) }),
            f!($parent, |a| { [p @ $T(_)] => parsed(p).parent().value() }),
            f!($name, |a| { [p @ $T(_)] => Str(parsed(p).name().into()) }),
            f!($suffix, |a| { [p @ $T(_)] => Str(parsed(p).suffix().into()) }),
            f!($stem, |a| {
                [p @ $T(_)] => {
                    let p = parsed(p);
                    let n = p.name();
                    Str(n[..n.len() - p.suffix().len()].into())
                }
            }),
            f!($with_name, |a| { [p @ $T(_), Str(n)] => with_name(&parsed(p), n)? }),
            f!($with_suffix, |a| { [p @ $T(_), Str(s)] => with_suffix(&parsed(p), s)? }),
            f!($parts, |a| { [p @ $T(_)] => strs(parsed(p).parts()) }),
            f!($abs, |a| {
                [p @ $T(_)] => {
                    let p = parsed(p);
                    Bool(!p.root.is_empty() && (!p.win || !p.drive.is_empty()))
                }
            }),
            f!($rel, |a| {
                [p @ $T(_), b @ $T(_)] => {
                    let p = parsed(p);
                    opt(p.strip(&parsed(b)).map(|segs| {
                        Parsed { drive: String::new(), root: "", segs, win: p.win }.value()
                    }))
                }
            }),
            f!($starts, |a| { [p @ $T(_), b @ $T(_)] => Bool(parsed(p).strip(&parsed(b)).is_some()) }),
            f!($norm, |a| { [p @ $T(_)] => parsed(p).normalize().value() }),
        ]
    };
}

static POSIX: [(Id, Bind); 12] = common!(
    Path,
    STD_PATH_TEXT,
    STD_PATH_PARENT,
    STD_PATH_NAME,
    STD_PATH_SUFFIX,
    STD_PATH_STEM,
    STD_PATH_WITH_NAME,
    STD_PATH_WITH_SUFFIX,
    STD_PATH_PARTS,
    STD_PATH_IS_ABSOLUTE,
    STD_PATH_RELATIVE_TO,
    STD_PATH_STARTS_WITH,
    STD_PATH_NORMALIZE
);

static WINDOWS: [(Id, Bind); 12] = common!(
    WindowsPath,
    STD_PATH_WINDOWS_TEXT,
    STD_PATH_WINDOWS_PARENT,
    STD_PATH_WINDOWS_NAME,
    STD_PATH_WINDOWS_SUFFIX,
    STD_PATH_WINDOWS_STEM,
    STD_PATH_WINDOWS_WITH_NAME,
    STD_PATH_WINDOWS_WITH_SUFFIX,
    STD_PATH_WINDOWS_PARTS,
    STD_PATH_WINDOWS_IS_ABSOLUTE,
    STD_PATH_WINDOWS_RELATIVE_TO,
    STD_PATH_WINDOWS_STARTS_WITH,
    STD_PATH_WINDOWS_NORMALIZE
);

static OTHER: &[(Id, Bind)] = &[
    f!(STD_PRELUDE_PATH, |a| { [Str(s)] => Parsed::posix(s).value() }),
    f!(STD_PRELUDE_WINDOWSPATH, |a| { [Str(s)] => Parsed::windows(s).value() }),
    f!(STD_TEXT_AS_PATH, |a| { [Str(s)] => Parsed::posix(s).value() }),
    f!(STD_PATH_AS_PATH, |a| { [p @ Path(_)] => p.clone() }),
    f!(STD_PATH_JOIN, |a| { [p @ Path(_), q @ (Path(_) | Str(_))] => parsed(p).join(&parsed(q)).value() }),
    f!(STD_PATH_WINDOWS_JOIN, |a| {
        [p @ WindowsPath(_), Str(q)] => parsed(p).join(&Parsed::windows(q)).value(),
    }),
    f!(STD_PATH_WINDOWS_DRIVE, |a| { [p @ WindowsPath(_)] => Str(parsed(p).drive.into()) }),
    f!(STD_PATH_WINDOWS_ROOT, |a| { [p @ WindowsPath(_)] => Str(parsed(p).root.into()) }),
    f!(STD_PATH_WINDOWS_ANCHOR, |a| { [p @ WindowsPath(_)] => Str(parsed(p).anchor().into()) }),
    f!(STD_PATH_WINDOWS_TO_POSIX, |a| {
        [p @ WindowsPath(_)] => {
            let p = parsed(p);
            opt((p.drive.is_empty() && p.root.is_empty())
                .then(|| Parsed { win: false, ..p }.value()))
        }
    }),
];

pub(super) fn binds() -> impl Iterator<Item = &'static (Id, Bind)> {
    POSIX.iter().chain(&WINDOWS).chain(OTHER)
}
