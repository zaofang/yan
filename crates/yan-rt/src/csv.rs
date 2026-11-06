//! The CSV kernel behind `std.csv`: RFC 4180 records over a configurable delimiter. Strict like
//! the JSON kernel: a stray quote, a bare CR or rows of different widths fail with a position.

#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    Syntax(&'static str),
    /// A row has `found` fields where the first row has `expected`.
    Ragged {
        expected: usize,
        found: usize,
    },
    /// The delimiter is a quote, CR or LF.
    Delimiter,
}

/// `line` and `col` start at 1 and `col` counts chars; `row` is the index of the record the
/// failure is in, 0 for the first.
#[derive(Clone, Debug, PartialEq)]
pub struct CsvError {
    pub line: usize,
    pub col: usize,
    pub row: usize,
    /// The byte offset of the failure.
    pub at: usize,
    pub reason: Reason,
}

type R<T> = Result<T, (usize, Reason)>;

fn locate(s: &str, at: usize, row: usize, reason: Reason) -> CsvError {
    let before = &s[..at];
    CsvError {
        row,
        at,
        line: before.matches('\n').count() + 1,
        col: before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1,
        reason,
    }
}

/// One field: its text, whether it was written in quotes, and the byte offset where it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub text: String,
    pub quoted: bool,
    pub at: usize,
}

/// Every record of `s`, all as wide as the first. An empty input has no records, and one final
/// line break is not a record. A blank line is a record of one empty field.
pub fn parse(s: &str, delim: char) -> Result<Vec<Vec<String>>, CsvError> {
    let rows = parse_cells(s, delim)?;
    Ok(rows
        .into_iter()
        .map(|r| r.into_iter().map(|c| c.text).collect())
        .collect())
}

/// [`parse`] keeping, for each field, whether it was quoted and where it starts.
pub fn parse_cells(s: &str, delim: char) -> Result<Vec<Vec<Cell>>, CsvError> {
    if matches!(delim, '"' | '\r' | '\n') {
        return Err(locate(s, 0, 0, Reason::Delimiter));
    }
    let mut out = Vec::new();
    match rows(s, delim, &mut out) {
        Ok(()) => Ok(out),
        Err((at, r)) => Err(locate(s, at, out.len(), r)),
    }
}

/// `line` and `col` of byte offset `at` in `s`.
pub fn line_col(s: &str, at: usize) -> (usize, usize) {
    let e = locate(s, at, 0, Reason::Delimiter);
    (e.line, e.col)
}

fn rows(s: &str, delim: char, out: &mut Vec<Vec<Cell>>) -> R<()> {
    let mut it = s.char_indices().peekable();
    let mut row: Vec<Cell> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut field_at = 0;
    let mut row_at = 0;
    let finish_row = |row: &mut Vec<Cell>,
                      field: &mut String,
                      quoted: &mut bool,
                      field_at: usize,
                      out: &mut Vec<Vec<Cell>>,
                      at| {
        row.push(Cell {
            text: std::mem::take(field),
            quoted: std::mem::take(quoted),
            at: field_at,
        });
        if let Some(first) = out.first()
            && first.len() != row.len()
        {
            return Err((
                at,
                Reason::Ragged {
                    expected: first.len(),
                    found: row.len(),
                },
            ));
        }
        out.push(std::mem::take(row));
        Ok(())
    };
    while let Some(&(at, c)) = it.peek() {
        if field.is_empty() && c == '"' {
            it.next();
            quoted = true;
            loop {
                match it.next() {
                    None => return Err((s.len(), Reason::Syntax("unterminated quoted field"))),
                    Some((_, '"')) if it.peek().is_some_and(|&(_, c)| c == '"') => {
                        it.next();
                        field.push('"');
                    }
                    Some((_, '"')) => break,
                    Some((_, c)) => field.push(c),
                }
            }
            // a closing quote is followed by a delimiter, a line break or the end
            match it.peek() {
                None => {}
                Some(&(_, c)) if c == delim || c == '\n' || c == '\r' => {}
                Some(&(at, _)) => return Err((at, Reason::Syntax("text after a closing quote"))),
            }
            if it.peek().is_none() {
                finish_row(&mut row, &mut field, &mut quoted, field_at, out, row_at)?;
            }
            continue;
        }
        it.next();
        match c {
            c if c == delim => {
                row.push(Cell {
                    text: std::mem::take(&mut field),
                    quoted: std::mem::take(&mut quoted),
                    at: field_at,
                });
                field_at = it.peek().map_or(s.len(), |&(i, _)| i);
            }
            '"' => return Err((at, Reason::Syntax("quote inside an unquoted field"))),
            '\r' if it.peek().is_some_and(|&(_, c)| c == '\n') => {
                it.next();
                finish_row(&mut row, &mut field, &mut quoted, field_at, out, row_at)?;
                row_at = it.peek().map_or(s.len(), |&(i, _)| i);
                field_at = row_at;
            }
            '\r' => return Err((at, Reason::Syntax("bare CR"))),
            '\n' => {
                finish_row(&mut row, &mut field, &mut quoted, field_at, out, row_at)?;
                row_at = it.peek().map_or(s.len(), |&(i, _)| i);
                field_at = row_at;
            }
            '\u{feff}' if at == 0 => return Err((at, Reason::Syntax("byte order mark"))),
            c => field.push(c),
        }
        if it.peek().is_none() && !(field.is_empty() && row.is_empty()) {
            finish_row(&mut row, &mut field, &mut quoted, field_at, out, row_at)?;
        }
    }
    Ok(())
}

pub fn quote(f: &str, delim: char, lone: bool, out: &mut String) {
    let needs = f.is_empty() && lone
        || f.chars()
            .any(|c| c == delim || matches!(c, '"' | '\r' | '\n'));
    if needs {
        out.push('"');
        out.push_str(&f.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(f);
    }
}

/// `rows` as CSV, each record ended by CRLF. Fields are quoted only when they must be, and an
/// empty field alone in its record is quoted so the line is not blank.
pub fn write(rows: &[Vec<String>], delim: char) -> Result<String, Reason> {
    if matches!(delim, '"' | '\r' | '\n') {
        return Err(Reason::Delimiter);
    }
    let mut out = String::new();
    for r in rows {
        for (i, f) in r.iter().enumerate() {
            if i > 0 {
                out.push(delim);
            }
            quote(f, delim, r.len() == 1, &mut out);
        }
        out.push_str("\r\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<Vec<String>> {
        parse(s, ',').unwrap_or_else(|e| panic!("{s:?}: {e:?}"))
    }

    fn e(s: &str) -> CsvError {
        parse(s, ',').expect_err(s)
    }

    fn t(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|r| r.iter().map(|f| (*f).to_owned()).collect())
            .collect()
    }

    #[test]
    fn plain_records() {
        assert_eq!(p("a,b\n1,2\n"), t(&[&["a", "b"], &["1", "2"]]));
        assert_eq!(p("a,b\r\n1,2\r\n"), t(&[&["a", "b"], &["1", "2"]]));
        assert_eq!(p("a,b\n1,2"), t(&[&["a", "b"], &["1", "2"]]));
        assert_eq!(p("a,b\r\n1,2"), t(&[&["a", "b"], &["1", "2"]]));
        assert_eq!(p("a\n"), t(&[&["a"]]));
        assert_eq!(p("a"), t(&[&["a"]]));
        assert_eq!(p("a,b,c"), t(&[&["a", "b", "c"]]));
    }

    #[test]
    fn mixed_line_endings() {
        assert_eq!(p("a\nb\r\nc\n"), t(&[&["a"], &["b"], &["c"]]));
    }

    #[test]
    fn empty_input_and_fields() {
        assert!(p("").is_empty());
        assert_eq!(p(",\n"), t(&[&["", ""]]));
        assert_eq!(p("a,,c\n"), t(&[&["a", "", "c"]]));
        assert_eq!(p(",,"), t(&[&["", "", ""]]));
        assert_eq!(p("a,\n"), t(&[&["a", ""]]));
        assert_eq!(p("\"\"\n"), t(&[&[""]]));
        assert_eq!(p("\"\",\"\"\n"), t(&[&["", ""]]));
    }

    #[test]
    fn blank_lines_are_records() {
        assert_eq!(p("a\n\nb\n"), t(&[&["a"], &[""], &["b"]]));
        assert_eq!(p("\n"), t(&[&[""]]));
        assert_eq!(p("\n\n"), t(&[&[""], &[""]]));
        let r = e("a,b\n\n");
        assert_eq!(
            r.reason,
            Reason::Ragged {
                expected: 2,
                found: 1
            }
        );
        assert_eq!((r.line, r.col), (2, 1));
    }

    #[test]
    fn quoted_fields() {
        assert_eq!(p("\"a,b\",c\n"), t(&[&["a,b", "c"]]));
        assert_eq!(p("\"say \"\"hi\"\"\"\n"), t(&[&["say \"hi\""]]));
        assert_eq!(p("\"a\nb\",c\n"), t(&[&["a\nb", "c"]]));
        assert_eq!(p("\"a\r\nb\"\n"), t(&[&["a\r\nb"]]));
        assert_eq!(p("\"\"\"\"\n"), t(&[&["\""]]));
        assert_eq!(p("x,\"y\"\n"), t(&[&["x", "y"]]));
        assert_eq!(p("\"a\",\"b\""), t(&[&["a", "b"]]));
        assert_eq!(p("\"a\"\r\n\"b\"\r\n"), t(&[&["a"], &["b"]]));
        assert_eq!(p("\"é語😀\"\n"), t(&[&["é語😀"]]));
    }

    #[test]
    fn invalid_quoting() {
        for s in [
            "a\"b\n", "\"a\"b\n", "\"abc", "\"a\"\"", "a,\"b\n", "a,b\"\n", "\"a\" \n",
        ] {
            assert!(matches!(e(s).reason, Reason::Syntax(_)), "{s:?}");
        }
    }

    #[test]
    fn bare_cr_and_bom_fail() {
        assert!(matches!(e("a\rb\n").reason, Reason::Syntax("bare CR")));
        assert!(matches!(e("a\r").reason, Reason::Syntax("bare CR")));
        assert!(matches!(e("\u{feff}a,b\n").reason, Reason::Syntax(_)));
        // a BOM anywhere else is data
        assert_eq!(p("a\u{feff}\n"), t(&[&["a\u{feff}"]]));
    }

    #[test]
    fn ragged_rows_are_rejected() {
        let r = e("a,b\n1\n");
        assert_eq!(
            r.reason,
            Reason::Ragged {
                expected: 2,
                found: 1
            }
        );
        assert_eq!((r.line, r.col), (2, 1));
        let r = e("a,b\n1,2,3\n");
        assert_eq!(
            r.reason,
            Reason::Ragged {
                expected: 2,
                found: 3
            }
        );
        let r = e("a,b\n1,2\n3\n");
        assert_eq!((r.line, r.col), (3, 1));
        assert!(matches!(e("a\nb,c").reason, Reason::Ragged { .. }));
    }

    #[test]
    fn other_delimiters() {
        assert_eq!(
            parse("a;b\n1;2\n", ';').unwrap(),
            t(&[&["a", "b"], &["1", "2"]])
        );
        assert_eq!(parse("a\tb\n", '\t').unwrap(), t(&[&["a", "b"]]));
        assert_eq!(parse("a,b;c\n", ';').unwrap(), t(&[&["a,b", "c"]]));
        assert_eq!(parse("a|b\n", '|').unwrap(), t(&[&["a", "b"]]));
        assert_eq!(parse("a語b\n", '語').unwrap(), t(&[&["a", "b"]]));
        for d in ['"', '\r', '\n'] {
            assert_eq!(parse("a\n", d).unwrap_err().reason, Reason::Delimiter);
            assert_eq!(write(&[], d), Err(Reason::Delimiter));
        }
    }

    #[test]
    fn error_positions() {
        let r = e("a,b\nc\"d,e\n");
        assert_eq!((r.line, r.col), (2, 2));
        let r = e("\"é\"x\n");
        assert_eq!((r.line, r.col), (1, 4));
        let r = e("ab\ncd\r");
        assert_eq!((r.line, r.col), (2, 3));
        let r = e("a,\"b\nc");
        assert_eq!((r.line, r.col), (2, 2));
    }

    #[test]
    fn writer() {
        let w = |rows: &[&[&str]]| write(&t(rows), ',').unwrap();
        assert_eq!(w(&[&["a", "b"], &["1", "2"]]), "a,b\r\n1,2\r\n");
        assert_eq!(w(&[&["a,b", "c"]]), "\"a,b\",c\r\n");
        assert_eq!(w(&[&["say \"hi\""]]), "\"say \"\"hi\"\"\"\r\n");
        assert_eq!(w(&[&["a\nb"]]), "\"a\nb\"\r\n");
        assert_eq!(w(&[&["a\rb"]]), "\"a\rb\"\r\n");
        assert_eq!(w(&[&[""]]), "\"\"\r\n");
        assert_eq!(w(&[&["", ""]]), ",\r\n");
        assert_eq!(w(&[&["é語", " x "]]), "é語, x \r\n");
        assert_eq!(w(&[]), "");
        assert_eq!(write(&t(&[&["a;b", "c"]]), ';').unwrap(), "\"a;b\";c\r\n");
    }

    #[test]
    fn round_trips() {
        let rows = t(&[
            &["name", "note"],
            &["a,b", "say \"hi\""],
            &["", ""],
            &["line\nbreak", "cr\r\nlf"],
            &["é語😀", " padded "],
        ]);
        assert_eq!(p(&write(&rows, ',').unwrap()), rows);
        let lone = t(&[&[""], &["x"], &[""]]);
        assert_eq!(p(&write(&lone, ',').unwrap()), lone);
        assert_eq!(parse(&write(&rows, ';').unwrap(), ';').unwrap(), rows);
    }

    #[test]
    fn cells_keep_quoting_and_offsets() {
        let cs = parse_cells("a,b,c\n\"\",,x\n", ',').unwrap();
        let row = &cs[1];
        assert_eq!(
            row.iter().map(|c| (c.quoted, c.at)).collect::<Vec<_>>(),
            [(true, 6), (false, 9), (false, 10)]
        );
        assert_eq!(cs[0][1].at, 2);
    }

    #[test]
    fn errors_know_their_record() {
        assert_eq!(parse_cells("a,b\n1,2\n3\n", ',').unwrap_err().row, 2);
        assert_eq!(parse_cells("a\n\"b\"c\n", ',').unwrap_err().row, 1);
    }
}
