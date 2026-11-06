//! `csv.parse[T]` and `csv.dump[T]` over `derive(Wire)` rows (17.3, R878 to R880). The
//! elaborator passes the row type's schema as JSON; see `yan-elab/src/csvwire.rs` for its shape.

use serde_json::Value as S;
use yan_rt::csv::{self, Cell, Reason as Kernel};
use yan_rt::json::{self, Json};
use yan_rt::wire::Limits;

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Value, panic};
use Value::*;

const TAG: &str = "tag";

enum Why {
    Syntax(String),
    Header(String),
    Mismatch(String, String),
    Missing(String),
    Unknown(String),
    Invalid(String),
    Limit,
}

/// A failure at byte offset `at`, in data row `row` (0 is the header) and column `column`.
struct Fail {
    at: usize,
    row: usize,
    column: String,
    why: Why,
}

type D<T> = Result<T, Fail>;

fn data(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Data {
        name: name.into(),
        fields: fields.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    }
}

fn text(s: &str) -> Value {
    Str(s.into())
}

fn name(f: &S) -> &str {
    f["n"].as_str().unwrap_or("")
}

/// How a cell reads in a mismatch message.
fn found(c: &Cell) -> String {
    if c.text.is_empty() && !c.quoted {
        "empty".into()
    } else if matches!(c.text.as_str(), "true" | "false") {
        "bool".into()
    } else if c.text.parse::<i64>().is_ok() {
        "int".into()
    } else if c.text.parse::<f64>().is_ok() {
        "float".into()
    } else {
        "str".into()
    }
}

fn expected(t: &S) -> String {
    match t {
        S::String(s) => s.clone(),
        S::Object(o) => match o.iter().next() {
            Some((k, v)) if k == "opt" => expected(v),
            Some((_, v)) => v
                .as_array()
                .map(|vs| {
                    vs.iter()
                        .filter_map(S::as_str)
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .unwrap_or_default(),
            None => String::new(),
        },
        _ => String::new(),
    }
}

struct Cx<'a> {
    def: &'a S,
}

impl Cx<'_> {
    fn sum(&self) -> Option<&Vec<S>> {
        self.def["sum"].as_array()
    }

    /// The columns of the row type, in output order: the tag first for a sum, then each field
    /// name at its first appearance, with its type.
    fn columns(&self) -> Vec<(String, Option<&S>)> {
        let mut out = Vec::new();
        if let Some(vs) = self.sum() {
            out.push((TAG.to_owned(), None));
            for f in vs
                .iter()
                .flat_map(|v| v["fs"].as_array().into_iter().flatten())
            {
                if !out.iter().any(|(n, _)| n == name(f)) {
                    out.push((name(f).to_owned(), Some(&f["t"])));
                }
            }
        } else {
            for f in self.def["rec"].as_array().into_iter().flatten() {
                out.push((name(f).to_owned(), Some(&f["t"])));
            }
        }
        out
    }

    fn cell(&self, t: &S, c: &Cell, row: usize, column: &str) -> D<Value> {
        let fail = |why| Fail {
            at: c.at,
            row,
            column: column.into(),
            why,
        };
        let bad = || fail(Why::Mismatch(expected(t), found(c)));
        Ok(match t {
            S::String(n) => match n.as_str() {
                "str" => text(&c.text),
                "bool" => match c.text.as_str() {
                    "true" => Bool(true),
                    "false" => Bool(false),
                    _ => return Err(bad()),
                },
                "int" => match c.text.parse::<i64>() {
                    Ok(i) => Int(i128::from(i)),
                    Err(_) if is_integer(&c.text) => {
                        return Err(fail(Why::Invalid("integer out of range".into())));
                    }
                    Err(_) => return Err(bad()),
                },
                _ => match c.text.parse::<f64>() {
                    Ok(f) if f.is_finite() => Float(f),
                    _ => return Err(bad()),
                },
            },
            S::Object(o) => match o.iter().next().expect("a schema") {
                (k, _) if k == "opt" && c.text.is_empty() && !c.quoted => opt(None),
                (k, t) if k == "opt" => opt(Some(self.cell(t, c, row, column)?)),
                (_, vs) => {
                    let vs = vs.as_array().expect("a schema");
                    match vs.iter().find(|v| **v == c.text.as_str()) {
                        Some(v) => data(v.as_str().unwrap_or(""), vec![]),
                        None => {
                            return Err(fail(Why::Mismatch(expected(t), c.text.clone())));
                        }
                    }
                }
            },
            _ => return Err(bad()),
        })
    }

    /// The fields of `decl` read from `cells`, whose columns are `cols` (indexed by `names`).
    fn fields(
        &self,
        decl: &[S],
        cells: &[Cell],
        cols: &[(String, Option<usize>)],
        row: usize,
        at: usize,
    ) -> D<Vec<(String, Value)>> {
        let mut out = Vec::new();
        for f in decl {
            let n = name(f);
            let v = match cols.iter().find(|(c, _)| c == n).and_then(|(_, i)| *i) {
                Some(i) => self.cell(&f["t"], &cells[i], row, n)?,
                None if f.get("d").is_some() => {
                    let d = &f["d"];
                    let c = Cell {
                        text: d.as_str().map_or_else(|| d.to_string(), str::to_owned),
                        quoted: d.is_string(),
                        at,
                    };
                    let c = if d.is_null() {
                        Cell {
                            text: String::new(),
                            quoted: false,
                            ..c
                        }
                    } else {
                        c
                    };
                    self.cell(&f["t"], &c, row, n)?
                }
                None if f["t"].get("opt").is_some() => opt(None),
                None => {
                    return Err(Fail {
                        at,
                        row,
                        column: n.into(),
                        why: Why::Missing(n.into()),
                    });
                }
            };
            out.push((n.to_owned(), v));
        }
        Ok(out)
    }

    fn row(&self, cells: &[Cell], cols: &[(String, Option<usize>)], row: usize) -> D<Value> {
        let at = cells.first().map_or(0, |c| c.at);
        let root = self.def["name"].as_str().unwrap_or("");
        let Some(vs) = self.sum() else {
            let decl = self.def["rec"].as_array().expect("a schema");
            return Ok(data_owned(root, self.fields(decl, cells, cols, row, at)?));
        };
        let i = cols
            .iter()
            .find(|(c, _)| c == TAG)
            .and_then(|(_, i)| *i)
            .expect("a tag column");
        let tag = &cells[i];
        let Some(v) = vs
            .iter()
            .find(|v| *v["n"].as_str().unwrap_or("") == *tag.text)
        else {
            let names = vs.iter().map(name).collect::<Vec<_>>().join("|");
            return Err(Fail {
                at: tag.at,
                row,
                column: TAG.into(),
                why: Why::Mismatch(names, tag.text.clone()),
            });
        };
        let decl = v["fs"].as_array().expect("a schema");
        for (c, i) in cols {
            if let Some(i) = i
                && c != TAG
                && !decl.iter().any(|f| name(f) == c)
                && !cells[*i].text.is_empty()
            {
                return Err(Fail {
                    at: cells[*i].at,
                    row,
                    column: c.clone(),
                    why: Why::Invalid(format!("`{}` has no field `{c}`", name(v))),
                });
            }
        }
        Ok(data_owned(
            name(v),
            self.fields(decl, cells, cols, row, at)?,
        ))
    }

    fn decode(&self, rows: &[Vec<Cell>]) -> D<Value> {
        let header = &rows[0];
        let known = self.columns();
        let mut cols: Vec<(String, Option<usize>)> =
            known.iter().map(|(n, _)| (n.clone(), None)).collect();
        for (i, h) in header.iter().enumerate() {
            let fail = |why| Fail {
                at: h.at,
                row: 0,
                column: h.text.clone(),
                why,
            };
            let Some(slot) = cols.iter_mut().find(|(n, _)| *n == h.text) else {
                return Err(fail(Why::Unknown(h.text.clone())));
            };
            if slot.1.replace(i).is_some() {
                return Err(fail(Why::Header(format!("duplicate column {:?}", h.text))));
            }
        }
        let at = header[0].at;
        // A record's absent columns are checked once, here; a sum's per row, by its variant.
        for f in self.def["rec"].as_array().into_iter().flatten() {
            let absent = cols.iter().any(|(n, i)| n == name(f) && i.is_none());
            if absent && f.get("d").is_none() && f["t"].get("opt").is_none() {
                return Err(Fail {
                    at,
                    row: 0,
                    column: name(f).into(),
                    why: Why::Missing(name(f).into()),
                });
            }
        }
        if self.sum().is_some() && cols[0].1.is_none() {
            return Err(Fail {
                at,
                row: 0,
                column: TAG.into(),
                why: Why::Missing(TAG.into()),
            });
        }
        let mut out = Vec::new();
        for (r, cells) in rows.iter().enumerate().skip(1) {
            out.push(self.row(cells, &cols, r)?);
        }
        Ok(List(out))
    }

    fn encode(&self, rows: &[Value], delim: char) -> Result<String, ()> {
        let columns = self.columns();
        let mut out = String::new();
        let line = |cells: &[(String, bool)], out: &mut String| {
            for (i, (f, force)) in cells.iter().enumerate() {
                if i > 0 {
                    out.push(delim);
                }
                csv::quote(f, delim, *force, out);
            }
            out.push('\n');
        };
        let header: Vec<_> = columns
            .iter()
            .map(|(n, _)| (n.clone(), n.is_empty()))
            .collect();
        line(&header, &mut out);
        for r in rows {
            let Data {
                name: variant,
                fields,
            } = r
            else {
                return Err(());
            };
            let decl = match self.sum() {
                Some(vs) => vs.iter().find(|v| name(v) == variant.as_str()).ok_or(())?["fs"]
                    .as_array()
                    .expect("a schema"),
                None => self.def["rec"].as_array().expect("a schema"),
            };
            let mut cells = Vec::new();
            for (c, _) in &columns {
                let cell = if c == TAG && self.sum().is_some() {
                    (variant.to_string(), false)
                } else {
                    match decl.iter().position(|f| name(f) == c) {
                        Some(i) => self.encode_cell(&decl[i]["t"], &fields.get(i).ok_or(())?.1)?,
                        None => (String::new(), false),
                    }
                };
                cells.push(cell);
            }
            line(&cells, &mut out);
        }
        Ok(out)
    }

    /// The text of a cell and whether it must be quoted even if plain (an empty `str`).
    fn encode_cell(&self, t: &S, v: &Value) -> Result<(String, bool), ()> {
        Ok(match (t, v) {
            (S::String(n), _) => match (n.as_str(), v) {
                ("bool", Bool(b)) => (b.to_string(), false),
                ("int", Int(i)) => (i64::try_from(*i).map_err(|_| ())?.to_string(), false),
                ("float", Float(f)) if f.is_finite() => {
                    (json::dump(&Json::Float(*f), None).map_err(|_| ())?, false)
                }
                ("str", Str(s)) => (s.to_string(), s.is_empty()),
                _ => return Err(()),
            },
            (S::Object(o), Data { name: n, fields }) => {
                let (k, t) = o.iter().next().expect("a schema");
                match (k.as_str(), n.as_str(), fields.as_slice()) {
                    ("opt", "None", _) => (String::new(), false),
                    ("opt", _, [(_, x)]) => self.encode_cell(t, x)?,
                    ("enum", n, []) => (n.to_owned(), false),
                    _ => return Err(()),
                }
            }
            _ => return Err(()),
        })
    }
}

fn data_owned(n: &str, fields: Vec<(String, Value)>) -> Value {
    Data {
        name: n.into(),
        fields: fields.into_iter().collect(),
    }
}

fn is_integer(s: &str) -> bool {
    let d = s.strip_prefix(['+', '-']).unwrap_or(s);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

fn error(s: &str, f: Fail) -> Value {
    let (line, col) = csv::line_col(s, f.at);
    let reason = match f.why {
        Why::Syntax(m) => data("Syntax", vec![("msg", text(&m))]),
        Why::Header(m) => data("Header", vec![("msg", text(&m))]),
        Why::Mismatch(e, g) => data(
            "Mismatch",
            vec![("expected", text(&e)), ("found", text(&g))],
        ),
        Why::Missing(n) => data("Missing", vec![("field", text(&n))]),
        Why::Unknown(n) => data("Unknown", vec![("field", text(&n))]),
        Why::Invalid(m) => data("Invalid", vec![("msg", text(&m))]),
        Why::Limit => data("Limit", vec![]),
    };
    data(
        "CsvError",
        vec![
            ("line", Int(line as i128)),
            ("col", Int(col as i128)),
            ("row", Int(f.row as i128)),
            ("column", text(&f.column)),
            ("reason", reason),
        ],
    )
}

/// `line L, col C: reason` (R880).
pub(super) fn show_error(fields: &[(super::Name, Value)]) -> String {
    let [
        (_, Int(line)),
        (_, Int(col)),
        _,
        _,
        (_, Data { name, fields: rf }),
    ] = fields
    else {
        unreachable!("a CsvError")
    };
    let reason = match (name.as_str(), rf.as_slice()) {
        ("Syntax" | "Header" | "Invalid", [(_, Str(m))]) => m.to_string(),
        ("Mismatch", [(_, Str(e)), (_, Str(f))]) => format!("expected {e}, found {f}"),
        ("Missing", [(_, Str(n))]) => format!("missing column {n:?}"),
        ("Unknown", [(_, Str(n))]) => format!("unknown column {n:?}"),
        _ => "input too large".into(),
    };
    format!("line {line}, col {col}: {reason}")
}

fn delimiter(d: &str) -> Option<char> {
    let mut cs = d.chars();
    cs.next().filter(|_| cs.next().is_none())
}

fn schema(s: &str) -> S {
    let mut sc: S = serde_json::from_str(s).expect("the elaborator's schema");
    sc["def"]["name"] = sc["root"].clone();
    sc
}

fn parse(src: &str, delim: &str, sc: &str) -> Result<Value, Value> {
    let sc = schema(sc);
    let cx = Cx { def: &sc["def"] };
    let lim = Limits::default();
    let top = |why| {
        error(
            src,
            Fail {
                at: 0,
                row: 0,
                column: String::new(),
                why,
            },
        )
    };
    let Some(d) = delimiter(delim).filter(|c| !matches!(c, '"' | '\r' | '\n')) else {
        return Err(top(Why::Syntax(
            "the delimiter is one character other than a quote or a line break".into(),
        )));
    };
    if src.len() > lim.bytes {
        return Err(top(Why::Limit));
    }
    let rows = csv::parse_cells(src, d).map_err(|e| {
        let why = match e.reason {
            Kernel::Syntax(m) => Why::Syntax(m.into()),
            Kernel::Ragged { expected, found } => {
                Why::Syntax(format!("row has {found} fields, expected {expected}"))
            }
            Kernel::Delimiter => Why::Syntax("invalid delimiter".into()),
        };
        error(
            src,
            Fail {
                at: e.at,
                row: e.row,
                column: String::new(),
                why,
            },
        )
    })?;
    if rows.is_empty() {
        return Err(top(Why::Header("missing header".into())));
    }
    if rows.len() > lim.len || rows.iter().flatten().any(|c| c.text.len() > lim.len) {
        return Err(top(Why::Limit));
    }
    cx.decode(&rows).map_err(|f| error(src, f))
}

pub(super) static BINDS: &[(yan_std::registry::Id, Bind)] = &[
    (
        yan_std::registry::id::STD_CSV_PARSE,
        Bind::Fallible(|a| {
            let [Str(src), Str(d), Str(sc)] = a else {
                return bad_args(yan_std::registry::id::STD_CSV_PARSE);
            };
            Ok(parse(src, d, sc))
        }),
    ),
    (
        yan_std::registry::id::STD_CSV_DUMP,
        Bind::Plain(|a| {
            let [List(rows), Str(d), Str(sc)] = a else {
                return bad_args(yan_std::registry::id::STD_CSV_DUMP);
            };
            let sc = schema(sc);
            let cx = Cx { def: &sc["def"] };
            let Some(d) = delimiter(d).filter(|c| !matches!(c, '"' | '\r' | '\n')) else {
                return panic("value");
            };
            match cx.encode(rows, d) {
                Ok(s) => Ok(Str(s.into())),
                Err(()) => panic("value"),
            }
        }),
    ),
];
