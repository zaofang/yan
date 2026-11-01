# 0008: std.path

- Status: draft
- Research: none; this RFC is the proposal (Wave 3 of the std build plan)
- Rows: filled in when accepted

## Summary

The module table of 17 lists `std.path` as "path manipulation" and nothing more. This RFC defines a
`Path` value type with lexical operations only: building, joining, splitting and normalizing paths
never touch the file system and need no capability. The design follows Python's `PurePosixPath`, with
the parts that make results depend on the host removed, so a path computed on one machine is the same
path on every other.

## Decisions

### D1: `Path` is a value type over `str`

`Path` is an immutable value holding a UTF-8 string. It is built by `Path(s: str)`, and `p.text()`
returns the string. It implements `Eq`, `Ord` (by `parts()`, element by element) and `Hash`.
`show(p)` prints the path text, so `f"{p}"` reads like Python's `str(p)`.

Options weighed:
- Functions on `str`, as in Python's `os.path`. These are rejected because nothing then marks a
  string as a path, and `Eq` would compare raw text, so `"a//b"` would differ from `"a/b"`.
- An OS string type, as in Rust's `OsStr`. Its value is that it carries non-UTF-8 names, which D5
  handles at the `fs` boundary instead.

Recommendation: accept.

### D2: one separator on every host

`Path` always uses `/` as its separator and has no Windows semantics: no drive letters, no `\`
separator and no UNC prefixes. A path's meaning never depends on the host, as design law 7 requires
for std results. The `fs` handler maps a `Path` to the host's native form at the capability call, and
on Windows it maps `/` to `\` there. Windows-style parsing, for tools that process Windows path text
as data, can come later as a separate `WindowsPath` type.

Recommendation: accept.

### D3: construction normalizes, `..` is kept

`Path(s)` stores a canonical form:
- Repeated separators collapse to one.
- `.` segments are dropped.
- A trailing `/` is dropped, except on the root.
- An empty string becomes `.`.

So `Path("a//./b/") == Path("a/b")`. `..` is kept, because removing it without the file system can
change the meaning of a path once symlinks are involved. Python's `PurePath` makes the same choices.
`p.normalize()` is the explicit lexical step:
- `x/..` is removed.
- `/..` becomes `/`.
- A leading `..` on a relative path stays.

Recommendation: accept.

### D4: the method set

Each method is lexical and total unless marked:

| Method | Result | Example |
| --- | --- | --- |
| `join[P: AsPath](part: P)` | the joined path; an absolute `part` replaces the receiver | `Path("a").join("b/c")` is `a/b/c` |
| `parent()` | the path without its last part; the parent of `/` is `/`, and the parent of `a` is `.` | |
| `name()` | the last part, `""` for `/` and `.` | `name()` of `a/b.tar.gz` is `b.tar.gz` |
| `suffix()` | the text from the last `.` in `name()`, `""` if there is none or the name starts with its only `.` | `.gz`; `""` for `.bashrc` |
| `stem()` | `name()` without `suffix()` | `b.tar` |
| `with_name(n)` | the path with its name replaced; panics with `value` if `n` is empty or contains `/` | |
| `with_suffix(s)` | the path with its suffix replaced; `s` is empty or starts with `.` and contains no `/` (panics with `value` otherwise) | |
| `parts()` | `list[str]`; an absolute path's first part is `/` | `["/", "usr", "lib"]` |
| `is_absolute()` | whether the path starts with `/` | |
| `relative_to(base)` | `Path \| None`: the rest of the path after `base`, compared by parts, with no `..` added | |
| `starts_with(base)` | whether `base`'s parts are a prefix of the path's parts | |

`trait AsPath` has one method, `as_path(self) -> Path`, and `str` and `Path` implement it. `join` is a
method, not `/`: section 10 binds `/` to `Field`, and a path is not a field.

Recommendation: accept.

### D5: `fs` takes `Path` and never returns a non-UTF-8 name

When `Path` lands, each `fs` operation that takes a path takes `P: AsPath`, so existing calls with a
`str` keep working. Directory listings return `Path`. A host file name that is not valid UTF-8 is not silently
replaced: the listing fails with `FsError` and a `NotUnicode` reason that carries the name's bytes as
`bytes`. A lossy listing hides one file behind another name, and that is worse than failing.

Recommendation: accept. This sets the `fs` rule now, so the `fs` wave does not need to reopen it.

## Spec changes

- 17.3: a `std.path` paragraph with D1 to D4, and a row-table entry.
- 17.3 `fs`: D5.
