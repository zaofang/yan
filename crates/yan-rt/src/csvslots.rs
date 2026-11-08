//! `csv.parse[T]` and `csv.dump[T]` over native slots (17.3, R878 to R881): the same schema and
//! rules as `yan-interp/src/csvwire.rs`, on the slot layout of `yan-mir`'s shapes (see
//! `jsonslots`). A cell is its slots: a scalar is one, `Option` a tag (`Some` 0, `None` 1) then
//! the payload, and a fieldless sum one tag.

use crate::abi::List;
use crate::csv::{self, Cell, Reason as Kernel};
use crate::json::{self, Json};
use crate::jsonslots::{Slot, get, items, wire, word};
use crate::wire::Limits;

/// Slots of a `CsvError`: line, col, row, column, then the reason's tag and every variant's fields.
pub const ERROR_WIDTH: usize = 12;

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
type Cols = [(String, Option<usize>)];

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

fn expected(t: &Json) -> String {
    match t {
        Json::Str(s) => s.clone(),
        Json::Object(o) => match o.first() {
            Some((k, v)) if k == "opt" => expected(v),
            Some((_, v)) => items(v).iter().map(wire).collect::<Vec<_>>().join("|"),
            None => String::new(),
        },
        _ => String::new(),
    }
}

fn is_integer(s: &str) -> bool {
    let d = s.strip_prefix(['+', '-']).unwrap_or(s);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

fn zeros(n: usize) -> impl Iterator<Item = Slot> {
    (0..n).map(|_| Slot::Word(0))
}

fn cell_width(t: &Json) -> usize {
    match t {
        Json::Object(o) => match o.first() {
            Some((k, v)) if k == "opt" => 1 + cell_width(v),
            _ => 1,
        },
        _ => 1,
    }
}

fn fields_width(decl: &Json) -> usize {
    items(decl)
        .iter()
        .map(|f| cell_width(get(f, "t").unwrap_or(&Json::Null)))
        .sum()
}

fn delimiter(d: &str) -> Option<char> {
    let mut cs = d.chars();
    cs.next()
        .filter(|c| cs.next().is_none() && !matches!(c, '"' | '\r' | '\n'))
}

struct Cx<'a> {
    def: &'a Json,
}

impl Cx<'_> {
    /// The tag column of a sum, `@wire(tag: ...)` or `tag` (R886).
    fn tag(&self) -> &str {
        match get(self.def, "tag") {
            Some(Json::Str(s)) => s,
            _ => "tag",
        }
    }

    /// Whether the type has `@wire(unknown: "ignore")` (R886).
    fn ignores(&self) -> bool {
        matches!(get(self.def, "unk"), Some(Json::Bool(true)))
    }

    fn sum(&self) -> Option<&[Json]> {
        get(self.def, "sum").map(items)
    }

    fn rec(&self) -> &Json {
        get(self.def, "rec").unwrap_or(&Json::Null)
    }

    /// The slots of one row.
    fn width(&self) -> usize {
        match self.sum() {
            Some(vs) => {
                1 + vs
                    .iter()
                    .map(|v| fields_width(get(v, "fs").unwrap_or(&Json::Null)))
                    .sum::<usize>()
            }
            None => fields_width(self.rec()),
        }
    }

    /// The column names of the row type, in output order: the tag first for a sum, then each
    /// field name at its first appearance.
    fn columns(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(vs) = self.sum() {
            out.push(self.tag().to_owned());
            for f in vs
                .iter()
                .flat_map(|v| items(get(v, "fs").unwrap_or(&Json::Null)))
            {
                if !out.iter().any(|n| n == wire(f)) {
                    out.push(wire(f).to_owned());
                }
            }
        } else {
            out.extend(items(self.rec()).iter().map(|f| wire(f).to_owned()));
        }
        out
    }

    fn cell(&self, t: &Json, c: &Cell, row: usize, column: &str, out: &mut Vec<Slot>) -> D<()> {
        let fail = |why| Fail {
            at: c.at,
            row,
            column: column.into(),
            why,
        };
        let bad = || fail(Why::Mismatch(expected(t), found(c)));
        match t {
            Json::Str(n) => match n.as_str() {
                "str" => out.push(Slot::Str(c.text.clone())),
                "bool" => match c.text.as_str() {
                    "true" => out.push(Slot::Word(1)),
                    "false" => out.push(Slot::Word(0)),
                    _ => return Err(bad()),
                },
                "int" => match c.text.parse::<i64>() {
                    Ok(i) => out.push(Slot::Word(i as u64)),
                    Err(_) if is_integer(&c.text) => {
                        return Err(fail(Why::Invalid("integer out of range".into())));
                    }
                    Err(_) => return Err(bad()),
                },
                _ => match c.text.parse::<f64>() {
                    Ok(f) if f.is_finite() => out.push(Slot::Word(f.to_bits())),
                    _ => return Err(bad()),
                },
            },
            Json::Object(o) => {
                let (k, v) = o.first().expect("a schema");
                if k == "opt" {
                    if c.text.is_empty() && !c.quoted {
                        out.push(word(1));
                        out.extend(zeros(cell_width(v)));
                    } else {
                        out.push(word(0));
                        self.cell(v, c, row, column, out)?;
                    }
                } else {
                    let at = items(v).iter().position(|x| wire(x) == c.text);
                    match at {
                        Some(i) => out.push(word(i)),
                        None => return Err(fail(Why::Mismatch(expected(t), c.text.clone()))),
                    }
                }
            }
            _ => return Err(bad()),
        }
        Ok(())
    }

    /// The fields of `decl` read from `cells`, whose columns are `cols`.
    fn fields(
        &self,
        decl: &Json,
        cells: &[Cell],
        cols: &Cols,
        row: usize,
        at: usize,
        out: &mut Vec<Slot>,
    ) -> D<()> {
        for f in items(decl) {
            let n = wire(f);
            let t = get(f, "t").unwrap_or(&Json::Null);
            match cols.iter().find(|(c, _)| c == n).and_then(|(_, i)| *i) {
                Some(i) => self.cell(t, &cells[i], row, n, out)?,
                None if get(f, "d").is_some() => {
                    let c = match get(f, "d").unwrap_or(&Json::Null) {
                        Json::Null => Cell {
                            text: String::new(),
                            quoted: false,
                            at,
                        },
                        Json::Str(s) => Cell {
                            text: s.clone(),
                            quoted: true,
                            at,
                        },
                        d => Cell {
                            text: json::dump(d, None).unwrap_or_default(),
                            quoted: false,
                            at,
                        },
                    };
                    self.cell(t, &c, row, n, out)?;
                }
                None if get(t, "opt").is_some() => {
                    out.push(word(1));
                    out.extend(zeros(cell_width(t) - 1));
                }
                None => {
                    return Err(Fail {
                        at,
                        row,
                        column: n.into(),
                        why: Why::Missing(n.into()),
                    });
                }
            }
        }
        Ok(())
    }

    fn row(&self, cells: &[Cell], cols: &Cols, row: usize, out: &mut Vec<Slot>) -> D<()> {
        let at = cells.first().map_or(0, |c| c.at);
        let Some(vs) = self.sum() else {
            return self.fields(self.rec(), cells, cols, row, at, out);
        };
        let i = cols
            .iter()
            .find(|(c, _)| c == self.tag())
            .and_then(|(_, i)| *i)
            .expect("a tag column");
        let tag = &cells[i];
        let Some(chosen) = vs.iter().position(|v| wire(v) == tag.text) else {
            let names = vs.iter().map(wire).collect::<Vec<_>>().join("|");
            return Err(Fail {
                at: tag.at,
                row,
                column: self.tag().into(),
                why: Why::Mismatch(names, tag.text.clone()),
            });
        };
        let v = &vs[chosen];
        let decl = get(v, "fs").unwrap_or(&Json::Null);
        for (c, i) in cols {
            if let Some(i) = i
                && c != self.tag()
                && !items(decl).iter().any(|f| wire(f) == c)
                && !cells[*i].text.is_empty()
            {
                return Err(Fail {
                    at: cells[*i].at,
                    row,
                    column: c.clone(),
                    why: Why::Invalid(format!("`{}` has no field `{c}`", wire(v))),
                });
            }
        }
        out.push(word(chosen));
        for (k, v) in vs.iter().enumerate() {
            let fs = get(v, "fs").unwrap_or(&Json::Null);
            if k == chosen {
                self.fields(fs, cells, cols, row, at, out)?;
            } else {
                out.extend(zeros(fields_width(fs)));
            }
        }
        Ok(())
    }

    fn decode(&self, rows: &[Vec<Cell>]) -> D<Vec<Slot>> {
        let header = &rows[0];
        let mut cols: Vec<(String, Option<usize>)> =
            self.columns().into_iter().map(|n| (n, None)).collect();
        for (i, h) in header.iter().enumerate() {
            let fail = |why| Fail {
                at: h.at,
                row: 0,
                column: h.text.clone(),
                why,
            };
            let Some(slot) = cols.iter_mut().find(|(n, _)| *n == h.text) else {
                if self.ignores() {
                    continue;
                }
                return Err(fail(Why::Unknown(h.text.clone())));
            };
            if slot.1.replace(i).is_some() {
                return Err(fail(Why::Header(format!("duplicate column {:?}", h.text))));
            }
        }
        let at = header[0].at;
        // A record's absent columns are checked once, here; a sum's per row, by its variant.
        for f in items(self.rec()) {
            let absent = cols.iter().any(|(n, i)| n == wire(f) && i.is_none());
            let t = get(f, "t").unwrap_or(&Json::Null);
            if absent && get(f, "d").is_none() && get(t, "opt").is_none() {
                return Err(Fail {
                    at,
                    row: 0,
                    column: wire(f).into(),
                    why: Why::Missing(wire(f).into()),
                });
            }
        }
        if self.sum().is_some() && cols[0].1.is_none() {
            return Err(Fail {
                at,
                row: 0,
                column: self.tag().into(),
                why: Why::Missing(self.tag().into()),
            });
        }
        let mut out = Vec::new();
        for (r, cells) in rows.iter().enumerate().skip(1) {
            self.row(cells, &cols, r, &mut out)?;
        }
        Ok(out)
    }

    /// The text of a cell and whether it must be quoted even if plain (an empty `str`).
    ///
    /// # Safety
    /// The `Str` slot in `w` points to a live block.
    unsafe fn encode_cell(&self, t: &Json, w: &[u64]) -> Result<(String, bool), ()> {
        Ok(match t {
            Json::Str(n) => match n.as_str() {
                "bool" => ((w[0] & 0xff != 0).to_string(), false),
                "int" => ((w[0] as i64).to_string(), false),
                "float" if f64::from_bits(w[0]).is_finite() => (
                    json::dump(&Json::Float(f64::from_bits(w[0])), None).map_err(|_| ())?,
                    false,
                ),
                "str" => {
                    // SAFETY: the caller's contract.
                    let s = unsafe { &*(w[0] as *const String) };
                    (s.clone(), s.is_empty())
                }
                _ => return Err(()),
            },
            Json::Object(o) => {
                let (k, t) = o.first().ok_or(())?;
                if k == "opt" {
                    if w[0] == 1 {
                        (String::new(), false)
                    } else {
                        // SAFETY: forwarded.
                        unsafe { self.encode_cell(t, &w[1..])? }
                    }
                } else {
                    (
                        wire(items(t).get(w[0] as usize).ok_or(())?).to_owned(),
                        false,
                    )
                }
            }
            _ => return Err(()),
        })
    }

    /// # Safety
    /// `w` holds one row; its `Str` slots point to live blocks.
    unsafe fn encode_row(&self, w: &[u64], columns: &[String]) -> Result<Vec<(String, bool)>, ()> {
        let (tag, decl, mut at) = match self.sum() {
            Some(vs) => {
                let chosen = w[0] as usize;
                let at = 1 + vs[..chosen.min(vs.len())]
                    .iter()
                    .map(|v| fields_width(get(v, "fs").unwrap_or(&Json::Null)))
                    .sum::<usize>();
                let v = vs.get(chosen).ok_or(())?;
                (wire(v), get(v, "fs").unwrap_or(&Json::Null), at)
            }
            None => ("", self.rec(), 0),
        };
        // Where each field of the row's variant starts.
        let mut starts = Vec::new();
        for f in items(decl) {
            starts.push(at);
            at += cell_width(get(f, "t").unwrap_or(&Json::Null));
        }
        let mut cells = Vec::new();
        for c in columns {
            let cell = if c == self.tag() && self.sum().is_some() {
                (tag.to_owned(), false)
            } else {
                match items(decl).iter().position(|f| wire(f) == c) {
                    Some(i) => {
                        let t = get(&items(decl)[i], "t").unwrap_or(&Json::Null);
                        // SAFETY: forwarded.
                        unsafe { self.encode_cell(t, &w[starts[i]..])? }
                    }
                    None => (String::new(), false),
                }
            };
            cells.push(cell);
        }
        Ok(cells)
    }
}

fn error(src: &str, f: Fail) -> Vec<Slot> {
    let (line, col) = csv::line_col(src, f.at);
    let (tag, pick) = match f.why {
        Why::Syntax(m) => (0, vec![Slot::Str(m)]),
        Why::Header(m) => (1, vec![Slot::Str(m)]),
        Why::Mismatch(e, g) => (2, vec![Slot::Str(e), Slot::Str(g)]),
        Why::Missing(n) => (3, vec![Slot::Str(n)]),
        Why::Unknown(n) => (4, vec![Slot::Str(n)]),
        Why::Invalid(m) => (5, vec![Slot::Str(m)]),
        Why::Limit => (6, vec![]),
    };
    let mut pick = pick.into_iter();
    let mut out = vec![
        word(line),
        word(col),
        word(f.row),
        Slot::Str(f.column),
        word(tag),
    ];
    // Field counts of Syntax, Header, Mismatch, Missing, Unknown, Invalid, Limit.
    for (i, n) in [1, 1, 2, 1, 1, 1, 0].into_iter().enumerate() {
        for _ in 0..n {
            out.push(if i == tag {
                pick.next().unwrap_or(Slot::Word(0))
            } else {
                Slot::Word(0)
            });
        }
    }
    out
}

fn schema(sc: &str) -> Json {
    json::parse(sc, 128).expect("the elaborator's schema")
}

/// The slots of `Result[list[T], CsvError]` for parsing `src` against `sc`.
pub fn parse(src: &str, delim: &str, sc: &str) -> Vec<Slot> {
    let sc = schema(sc);
    let cx = Cx {
        def: get(&sc, "def").unwrap_or(&Json::Null),
    };
    let failed = |f: Fail| {
        let mut out = vec![word(1), Slot::Word(0)];
        out.extend(error(src, f));
        out
    };
    let top = |why| Fail {
        at: 0,
        row: 0,
        column: String::new(),
        why,
    };
    let Some(d) = delimiter(delim) else {
        return failed(top(Why::Syntax(
            "the delimiter is one character other than a quote or a line break".into(),
        )));
    };
    let lim = Limits::default();
    if src.len() > lim.bytes {
        return failed(top(Why::Limit));
    }
    let rows = match csv::parse_cells(src, d) {
        Ok(r) => r,
        Err(e) => {
            let why = match e.reason {
                Kernel::Syntax(m) => Why::Syntax(m.into()),
                Kernel::Ragged { expected, found } => {
                    Why::Syntax(format!("row has {found} fields, expected {expected}"))
                }
                Kernel::Delimiter => Why::Syntax("invalid delimiter".into()),
            };
            return failed(Fail {
                at: e.at,
                row: e.row,
                column: String::new(),
                why,
            });
        }
    };
    if rows.is_empty() {
        return failed(top(Why::Header("missing header".into())));
    }
    if rows.len() > lim.len || rows.iter().flatten().any(|c| c.text.len() > lim.len) {
        return failed(top(Why::Limit));
    }
    match cx.decode(&rows) {
        Ok(elems) => {
            // Ok is tag 0; the Err slots stay zero.
            let mut out = vec![word(0), Slot::List(cx.width(), elems)];
            out.extend(zeros(ERROR_WIDTH));
            out
        }
        Err(f) => failed(f),
    }
}

/// The csv text of the rows in `rows` against `sc`, or `None` when the delimiter is invalid or
/// a float is not finite.
///
/// # Safety
/// `rows` holds values of the schema's row type; their `Str` slots point to live blocks.
pub unsafe fn dump(rows: &List, delim: &str, sc: &str) -> Option<String> {
    let sc = schema(sc);
    let cx = Cx {
        def: get(&sc, "def").unwrap_or(&Json::Null),
    };
    let delim = delimiter(delim)?;
    let columns = cx.columns();
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
    let header: Vec<_> = columns.iter().map(|n| (n.clone(), n.is_empty())).collect();
    line(&header, &mut out);
    for r in rows.data.chunks(cx.width()) {
        // SAFETY: the caller's contract.
        line(&unsafe { cx.encode_row(r, &columns) }.ok()?, &mut out);
    }
    Some(out)
}

/// The slot count of one row of the schema's root type.
pub fn row_width(sc: &str) -> usize {
    Cx {
        def: get(&schema(sc), "def").unwrap_or(&Json::Null),
    }
    .width()
}

/// `line L, col C: reason` (R880) for the 12 slots of a `CsvError`.
///
/// # Safety
/// The `Str` slots of the chosen variant point to live blocks.
pub unsafe fn show_error(w: &[u64]) -> String {
    // SAFETY: the caller's contract.
    let st = |i: usize| unsafe { &*(w[i] as *const String) };
    let reason = match w[4] {
        0 => st(5).clone(),
        1 => st(6).clone(),
        2 => format!("expected {}, found {}", st(7), st(8)),
        3 => format!("missing column {:?}", st(9)),
        4 => format!("unknown column {:?}", st(10)),
        5 => st(11).clone(),
        _ => "input too large".into(),
    };
    format!("line {}, col {}: {reason}", w[0] as i64, w[1] as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str =
        r#"{"root":"P","def":{"rec":[{"n":"a","t":"int"},{"n":"b","t":{"opt":"str"}}]}}"#;

    #[test]
    fn parse_lays_out_a_result() {
        let r = parse("a,b\n7,\n8,x\n", ",", SCHEMA);
        // Ok tag, the list, then the Err slots
        assert_eq!(r.len(), 2 + ERROR_WIDTH);
        assert!(matches!(r[0], Slot::Word(0)));
        let Slot::List(3, elems) = &r[1] else {
            panic!("a list of 3-slot rows");
        };
        assert_eq!(elems.len(), 6);
        assert!(matches!(elems[0], Slot::Word(7)));
        assert!(matches!(elems[1], Slot::Word(1)), "None");
        assert!(matches!(elems[4], Slot::Word(0)), "Some");
    }

    #[test]
    fn parse_failure_is_an_err_with_a_position() {
        let r = parse("a,b\nx,\n", ",", SCHEMA);
        assert_eq!(r.len(), 2 + ERROR_WIDTH);
        assert!(matches!(r[0], Slot::Word(1)));
        assert!(matches!(r[2], Slot::Word(2)), "line");
        assert!(matches!(r[3], Slot::Word(1)), "col");
        assert!(matches!(r[4], Slot::Word(1)), "row");
        assert!(matches!(r[6], Slot::Word(2)), "Mismatch");
    }

    #[test]
    fn dump_reads_the_slots() {
        let name = Box::into_raw(Box::new("hi".to_owned()));
        let rows = List {
            words: 3,
            data: vec![5, 0, name as u64, 6, 1, 0],
            limit: -1,
        };
        // SAFETY: the rows hold `P`s and `name` is live.
        let out = unsafe { dump(&rows, ",", SCHEMA) };
        // SAFETY: reclaimed once.
        drop(unsafe { Box::from_raw(name) });
        assert_eq!(out.as_deref(), Some("a,b\n5,hi\n6,\n"));
    }

    #[test]
    fn a_type_can_ignore_unknown_columns_and_rename_enum_cells() {
        let schema = r#"{"root":"P","def":{"rec":[{"n":"c","t":{"enum":[{"n":"Red","w":"r"},{"n":"Green"}]}}],"unk":true}}"#;
        let r = parse("c,z\nr,1\nGreen,2\n", ",", schema);
        assert!(matches!(r[0], Slot::Word(0)));
        let Slot::List(1, elems) = &r[1] else {
            panic!("a list of 1-slot rows");
        };
        assert!(matches!(elems[..], [Slot::Word(0), Slot::Word(1)]));
        assert!(matches!(parse("c\nRed\n", ",", schema)[0], Slot::Word(1)));
    }
}
