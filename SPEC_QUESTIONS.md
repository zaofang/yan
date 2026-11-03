# Spec questions

## Conformance header layout

Spec 24.1 shows `# expect: E501 at 14:9` (Wen and Yong: `-- expect:`), `# stdout:` blocks and
`# exit: 101`, but not how a `stdout:` block is laid out.

Assumed in `tools/conform.py` until resolved: headers are the file's leading comment lines (`#` in
Xin, `--` in Wen and Yong); a `stdout:` block is the following comment lines with one space
stripped (a bare marker is an empty line) up to the next header or the first non-comment line;
several blocks concatenate. `patch/` cases are directories and are skipped until their format is
defined.

## Yong text lexical rules

Spec 3.5 uses `Name`, `Literal` and `String` without defining them. Assumed in `yong-core` until
resolved:

- `Name` is `[A-Za-z_][A-Za-z0-9_]*`; `Path` is names joined by `.`. Keywords are contextual.
- Integer literals are decimal, `0x`, `0o` or `0b`, with `_` separators and an optional leading
  `-`; float literals add a fraction and/or an exponent. Literals carry no type suffix.
- Strings are `"..."` and chars `'c'`, with escapes `\n \r \t \0 \\ \" \'` and `\u{hex}`.
  A `'` followed by a name and no closing quote is a label.
- `true`, `false` and `()` (the `Unit` value) are literals.
- Diagnostic columns are 1-based UTF-8 byte offsets.

## Yong canonical layout

Spec 3.5 fixes the grammar, not the printed layout, and comments are not part of the IR. Assumed:
four-space indentation, one statement per line, contracts on their own lines before the body,
records and sums on one line, one blank line between items. The printer keeps only the file's
leading comment lines (the conformance headers); round-trip means print(parse(print(parse(x))))
equals print(parse(x)).

## `uses` lists inside other lists

`Fn(...) -> T uses a, b` inside a parameter list or a type-argument list is ambiguous with that
list's commas. Assumed: a comma continues the capability set unless the next name is followed by
`:`, `[` or `.`.

## Checker readings not fixed by the spec

Assumed in `yong-check` until resolved:

- Scalars (integers, floats, `bool`, `char`, `Unit`) are copied, so a `sink` argument does not
  move them, and passing a scalar local to `become` with `let` is not E420.
- E602 applies to direct and mutual recursion found in function bodies; `example` and other
  contract calls are not edges.
- E401 is flow-sensitive across `if` and `match`: a branch that ends in `ret`, `fail`, `jump`,
  `become`, `break`, `continue` or `unreachable` does not carry its moves forward.
- A type the checker cannot work out (std results, generic instances, tuples, closures) is unknown
  and never produces E201. An integer literal fits any numeric type, a float literal any
  non-integer numeric type.
- `yong run` checks the module first and exits 1 with the diagnostics on stderr when it has errors.

## Standard library declarations (resolved: R474 to R476, spec rev 496)

Raised by the std build plan; Antonio accepted all three answers on 2026-10-31.

- R474: a std function with a native body is `@intrinsic def name(params) -> T: ...`. `@intrinsic`
  is reserved to the `std` package; the intrinsic list, including the Y4 scalar ops, lives in
  `yong-std`.
- R475: for step 2 of method resolution (17.2), `str` and `char` are declared in `std.text`,
  `list` in `std.list`, `dict` in `std.dict`, `set` in `std.set`, `bytes` in `std.encoding`, the
  scalars and `bigint` in `std.num`, and `Result` and `Option` in `std.result`.
- R476: in Yong text, `Option[T]` matches as `Some { value: v }` and `None`, alongside Y5's
  `Ok { value: v }` and `Err { error: e }`.

## Xin elaboration readings

Assumed in `yong-elab` until resolved:

- E210 exempts the arguments of a `become` call. `become count(n - 1, acc + 1)` with two `int`
  parameters is a run case (`xin/run/tail/become_constant_stack`) and appears in
  `xin/pass/syntax/every_production`, though 14.1's exemptions (literals, names equal to the
  parameter, positional-only parameters, the receiver) do not cover it.
- E210 is reported once per call, at the first argument that needs a keyword (the note in
  `xin/fail/types/e210_record_keyword_required`).

- Bitwise `&`, `|` and `^` on integers lower to the intrinsics `and_<int>`, `or_<int>` and
  `xor_<int>`, which Y4 does not name; they cannot overflow. Shifts stay unsupported until their
  overflow rule is settled.
- `_ = e` evaluates `e` and discards it.
- `par [e for x in xs]` runs sequentially in the interpreter. The result order and the
  lowest-index failure (9.2) are what a parallel run must also produce.
- `&xs[i]` and `&r.f` pass a copy that is stored back after the statement. Two `&` elements of
  the same list in one call insert `exclusive_check` on their indices first (4.3).
- A lambda's parameter types come from the expected callable type. `k = lambda x: x + 1` with
  no annotation takes them from the first call `k(...)`, where the closure is created; that is
  the same as creating it at the binding only because nothing it captures is reassigned, so a
  lambda that captures a reassigned name, or whose name is used before a call, is not lowered yet. Lambda arguments are lowered after the call's other
  arguments, so `twice(lambda s: s + "!", "hi")` finds `T` from `"hi"`; creating a closure has no
  effects, so the order is not observable.
- A lambda captures locals by copy, `var` parameters `inout` (E406 if it escapes) and
  capabilities by name (E306 if it escapes, R467).

## Std runtime readings (Wave 0)

Assumed in `yong-interp`'s native std functions until the spec says otherwise:

- `d[k]` on a dict without `k` panics with kind `bounds`; 8.3 lists no key-specific kind.
- `bigint` arithmetic intrinsics are spelled `<op>_bigint` (`add`, `sub`, `mul`, `floordiv`, `mod`
  and the six comparisons), with no `_checked` suffix because they cannot overflow. Floor division
  and modulo round toward negative infinity, as for the sized integers.
- `str.parse_int` accepts an optional sign and ASCII digits only (no spaces, underscores or radix
  prefixes) and returns None outside `int`. `str.parse_float` accepts the same plus `.`, an
  exponent, and `inf`, `infinity` and `nan` in any case.
- `str.splitlines` splits on CPython's line boundaries and drops a final empty line.

## std.regex readings (R482 to R486)

Assumed in the regex implementation until the spec says otherwise:

- `split` keeps each captured group between the pieces, as Python does. A group that did not
  take part is `""`, because the result is `list[str]`, where Python gives None.
- A `RegexError` reason, and the E545 message, is the crate's text (for example `unclosed group`).
  It is not Python's wording.
- `group(i)` past the last group, and `named(n)` for a name the pattern lacks, panic with kind
  `bounds`. Python raises IndexError there.
- A free std function is called by its module's name with no import (`regex.compile`,
  `math.sqrt`), as the N01 reading of `import` implies.

## std.collections readings (R488 to R493)

Assumed in the collections implementation until the spec says otherwise:

- std.collections declares one submodule per type (`std.collections.deque`, `.heap`,
  `.sorted_dict`, `.sorted_set`), because the four types share method names such as `len`, `pop`
  and `first`. R475's declaring module for each type is its submodule.
- `for k in m` over a `sorted_dict` iterates its keys in ascending order, as over a Python dict.
  `for` over a heap is E201, the code for a type that does not fit.
- `sorted_set(xs)` and `heap(xs)` take a list, and `sorted_dict(d)` takes a dict.
- `m[k] op= v` on a `sorted_dict` does not insert `Default` first yet; R438 is implemented for
  `dict` only, so a missing key panics with `bounds`.
- The interpreter keeps the sorted types as sorted vectors, with O(n) insertion. No program can
  tell this from a B-tree except by timing.

## Std elaboration readings (Wave 2)

Assumed in `yong-elab`'s std loading until the spec says otherwise:

- `FsError` is a record with `path: str` and `reason: str`. 17.3 says only that each module's
  error type is a record or sum with `Show`, `Eq` and `Wire`. A missing file has reason
  `not found`.
- `fake_fs(files)` takes the map, and `with fs = fake_fs(...)` installs it as the `fs`
  capability for the block. `fs.read` of a path the map lacks fails with `FsError`.
- Like the prelude, `std.test` is in scope without an import, which extends assumption A7 from
  `fake_fs` to every `std.test` function.
- A std class reaches a module when the module's source names the class or the class's module
  (for example `fs`). Such a class is lowered into the module as a record, unless the module
  defines that name itself.
- `Error` is a record with `message: str`, the wrapped failure's `Show` text, and
  `context: list[str]`, outermost first. Keeping the text instead of the value means
  `e is HttpError(...)` cannot downcast yet. `?` into a function that fails with `Error`
  converts through `std.error.wrap`. `f().context(msg)` calls `std.error.context` on
  `try f()`: on success it gives the value, and otherwise it fails with an `Error` that has
  `msg` added.

## Wen lexical and grammar readings

15.1 and 15.2 leave these open. Assumed in `yong-wen` until resolved:

- **Signature continuation.** A line whose first token is `fail` or `uses` continues the previous
  logical line when that line is indented less, starts (after `pub`, `unsafe`, `gen`, `sub`,
  `lemma` or `safe`) with a name followed by `(` or `[`, has no `=` outside brackets and does not
  end in `->`. Other continuation lines start with `.`, `and`, `or` or a binary operator other
  than `@`, which would read as an attribute.
- **E134.** Inside a grid, a signed element is E134 when the element before it in the row or the
  signed operand is not a literal: `[1 -2]` is two elements, `[a -b]` and `[1 -b]` are E134. The
  span starts at the element before the sign.
- **SIGN and SPLPAREN outside grids.** The lexer marks them wherever the innermost bracket is
  `[`. In an index (`xs[i -1]`) they are read as a binary operator and a call; in a list after a
  `,` a SIGN is a unary sign.
- **Time literals.** A standalone time needs seconds (`09:00:00`), so `{10:20}` stays a map.
  Inside a date-time, `T09:00` may omit them.
- **Stage-0 calls.** `#` applies to the whole postfix chain after it: `#deep(0)` is the stage-0
  value of the call.
- **Grammar gaps.** Accepted although 15.2 does not list them: a labelled inout argument
  (`swap(x: &a)`), a bare value name as a type (`Ptr[c_int]`, the `core.ffi` C types), and a
  negative number literal as a pattern (`-1 -> ...`).
- **E112.** Two consecutive capitals are allowed only as a one-letter first word followed by a
  lower-case letter, so the `core.ffi` names `CStr` and `CString` lex, and `HTTPError` is E112.
- **Strict checking.** Wen is elaborated with the lenient reader off (14.4 is Xin's), so
  `s.len()` on a `str` is E504 and recursion without `decreases` gets no N05 measure.
- **`x.len()`.** A call of `len` with no arguments is read as the builtin `len(x)`, which the
  bounds solver and E504 recognise; a user type's own `len` method is not reachable this way yet.
- **Unannotated `self`.** In an impl, `self` has the impl's type; in a trait, `Self`.

## Patch protocol readings (18.3)

Assumed in `yong patch` until the spec says otherwise:

- A JSON patch's `module` names `<module with dots as slashes>.xin` beside the patch.
- A plain file patch names no module. `--module NAME` gives it; without the flag, the patch applies only when exactly one other `.xin` file is beside it. `conformance/patch/plain_file_patch` has `m.xin` and `result.xin` beside it, so it needs `flags = ["--module", "m"]`.
- The item hash in `base` is `b3:` followed by the BLAKE3 hex digest of the item's canonical text, printed alone.
- An edit's search text is matched in the item's canonical text, and the edited text must still parse as that one item; otherwise the patch is P002.
- P006 covers a change to, or the deletion of, an existing `test_` function, and any change to the set of `pre`, `post`, `ex`, `invariant` and `decreases` lines of an existing item. Adding a new test is allowed.
- `--check` refuses with P003 when the patched module has an error whose code and item the original module did not have. The introduced diagnostics follow the P003 record.
- The oplog line is `{"file", "before", "after"}` with the module's whole text.

## MCP readings (21.4)

- `yong mcp` serves the modules under the working directory. A tool's `name` of `a.b` means item `b` of module `a` when `a.xin` exists and `a/b.xin` does not.
- `max_tokens` and `budget` count four bytes per token; lines or items are kept in order while they fit.
- No diagnostic carries a machine fix yet, so `autofix` returns the canonical reprint (the lenient reader's normalizations, 14.4) of each item whose text differs, as a `replace` patch with base hashes.
- `sig` gives each function up to its first body statement (decorators, header, docstring, `pre`/`post`/`ex`/`decreases`), and every other item whole.
- A plain text patch through `check_patch`/`apply_patch` takes the module as a `module` argument. Patches are always checked (P003), and protected targets refuse with P006, since the server has no confirmation token yet.

## FFI readings (12)

- A module is an ffi module when a directory named `ffi` contains it (conformance A9); the CLI decides from the path.
- `core.ffi` names (`c_int` and the rest, `Ptr`, `CStr`, `CString`, `Owned`, `View`, `MutView`) resolve only in ffi modules. The C scalars take LP64 sizes: `c_int` is `i32`, `c_long` and `c_ssize` are `i64`, `c_size` is `u64`, `c_double` is `f64`, `c_void` is `Unit`.
- Yong has no C string literal yet: `c"..."` lowers to a `Str` literal typed `CStr`. Interior NUL (E140) is not checked yet.
- Calling an extern item does not yet require `uses lib` (12.2): the conformance cases call extern items from functions without a `uses` clause.
- A `MutView[T]` extern parameter takes `&list`; a list passes as `View[T]` by value.
- E905 covers the default argument promotions: `f32` to `f64`, and `i8`, `i16`, `u8`, `u16`, `bool` to `c_int`.
- E906 names: `setjmp`, `_setjmp`, `sigsetjmp`, `savectx`, `vfork`, `getcontext`.
- E907: the profile table lists owned `Str` and `List`, and `e907_export_non_profile` rejects `total(xs: List[int])`. Reading: an owned `Str` or `List` crosses only as a `take` parameter or a result; a `let` parameter must be a `View`.
- E911 checks register classes and explicit registers of the host target (x86_64 or aarch64).
- `@forbid(alloc)` (E900) flags list, set and dict literals, comprehensions and f-strings. `@forbid(rc, copy, panic)` is not checked yet and stops elaboration.
- A named function used as a value lowers to a closure that calls it (closures are the only function values in core).

## Stage-0 readings (10.4)

- `let x: T = #e` at the top level is evaluated on the interpreter as a nullary function over the module, and its value becomes the constant. Inside function bodies, `#e` folds only integer and bool constants for now.
- Steps count executed calls; loop iterations and allocations are not counted yet. The evaluator runs on a 1 GiB stack so `@limit(stage0_depth: n)` above the default is reachable.
- A function with one `Nat` parameter is a template: each call `f[k](...)` lowers an instance `f__k` with `k` folded in. `#if` must fold to a bool, and only its taken branch is lowered. `#for` runs over a list literal of constants. Errors inside an instance are E272, except `compile_error` (E270), and each carries a note naming the instantiation. Code that does not depend on the parameter is not yet checked once at the definition (R346 phase one).
- `embed(path)` resolves against the module's directory. The package root is the nearest ancestor with a `yong.toml`, else the module's directory.
- `wen/pass/stage0/limit_raises_depth.wen` and `wen/run/stage0/folded_constant.wen` recursed without `decreases` (E602). Antonio chose to add the lines to the cases (2026-11-01); Wen infers no measures.

## Intrinsic registry readings (R561)

Assumed in `yong-std`'s registry and the interpreter until the spec says otherwise:

- **Generated entries.** The registry is generated at build time from the `@intrinsic` functions and `@capability` operations in `yong-std/lib/<tier>/` (`core`, `alloc`, `std`), plus the scalar `<op>_<ty>` family and `exclusive_check`. An entry's signature is its declaration line; purity is read from it (a capability operation is `Cap`, a `var` parameter makes it `Mutates`, otherwise `Pure`). Only `Cap` entries are restricted to run time (stage 1 and later).
- **Ids.** An id is the entry's index in path order, stable within one build only. Nothing persists ids; the stage-0 query cache, when it exists, will key on paths.
- **Resolution point.** Yong text names std functions by path, and Yong text is the interpreter's input, so the path is resolved to its id when the interpreter compiles a call site, and again by each code generator from `yong-mir`. Elaboration checks only that every std function it may call has an entry (`registry_matches_declarations`). Run time never looks an intrinsic up by name once its module is bound by id.

## Native code readings (M1b, 22.1 to 22.4)

Assumed in `yong-mir` and `yong-cg-cranelift` until the spec says otherwise:

- **Milestone order.** The lead chose on 2026-11-01 to start M1b before the M1 gate passes, against 22.6 rule 1, because code generation depends on checked Yong only. The interpreter stays the oracle; native code must match it on every case it runs.
- **In-process first.** The first backend compiles with Cranelift's JIT and runs in the `yong` process (`yong --backend cranelift run FILE`). Object files and the system linker of 23 (M1b) come next, once std values have a native layout.
- **Coverage.** `yong-mir` lowers integers up to 64 bits, `f64`, `bool`, `Unit`, `Str`, `List[T]`, tuples, non-generic records and non-recursive sums, and closures without `inout` captures or parameters. Generic functions are instantiated per list of type arguments, and only functions reachable from `main` are lowered. It also covers direct and tail calls, `inout` parameters, loops, join points, `match` on literals, ranges, tuples and constructors, `Option` and `Result`, `fail`, `try` and `?` without a conversion, `requires`, `assert` and `par` (in order). The std calls it covers are `std.io.print`, `std.text.concat`, `std.fmt.show` and `std.list.new`, `push`, `append` and `len`, plus list indexing. Anything else is a tool failure (`not supported yet`), not a program result.
- **Layout.** Until 5.2's layout pass exists, a record or tuple is one machine value per scalar field, and a sum is a tag followed by every variant's fields, with no overlap and no niches. Each `inout` parameter comes back as extra results. A function with `fail E` returns `Result[T, E]` slots, and a failing `main` exits with status 1. List elements are 8-byte words, one per scalar. A closure is its code address and a block holding its captures by value; its body is a function that takes that block first. Only these values cross a call, so the reading is not observable.
- **No drops yet.** `Str` and `List` blocks are allocated in `yong-rt` and are not freed: drop elaboration (5.3 pass 2) lands with `yong-opt`. Memory is the only difference this makes. `copy` of a list copies its words, so a copy of a list whose elements hold lists is still unsupported.
- **Unsafe code.** The workspace forbids `unsafe`. `yong-rt` and `yong-cg-cranelift` deny it instead and allow it only in `yong_rt::abi` (the C-ABI entry points), at the call into generated code, and in the lazy-compile callback that stubs call.
- **Lazy compilation.** `yong --backend cranelift run` compiles each function at its first call (`--jit lazy`, the default; `--jit eager` compiles all before `main`). Calls go through one code-address table per program, read as the debug jump table of 18.4: hot reload (M2) writes the same slots. A Cranelift error found at a first call is a tool failure (exit 2) after the program has started.

## Probe-gap readings (14.1, 14.4, 8.1, 8.4)

Assumed in `yong-elab` and the interpreter until the spec says otherwise:

- **Injection into `T | None`.** A `T` where `T | None` is expected (an argument, a return, an assignment, an annotated binding) is lowered as `Some` of it. Std natives that take an optional closure accept it either way.
- **Narrowing.** `x is None` and `x is not None` narrow a local of optional type in the branch they guard, through `and`, `or` and `not`. When one branch of an `if` without `elif` exits, the rest of the block is narrowed by the other branch's test. A `while x is not None:` body is narrowed. An assignment to `x` ends the narrowing; a call that passes `&x` or uses `x` as a `var` receiver does not end it yet. `x is None` on a type that is not optional is E201.
- **N15.** The truth test of a non-`bool` condition (in `if`, `while`, `assert`, `post`, `not`, `and`, `or` and conditional expressions) is `n != 0` for numbers, `s != ""` for `str`, `len(xs) > 0` for collections and `x is not None` for optionals. Other types stay E201. In strict mode every such condition is E201.
- **`s.split()`.** With no separator it splits at runs of the characters CPython's `str.isspace` accepts and drops empty parts. `split(sep: str | None = None)` is the std declaration.
- **`s.chars()`.** Returns `iter[char]`, like iterating a `str` under N21.
- **Slices.** `xs[a:b:c]` on a list lowers to `std.list.slice_clamped`, which clamps as CPython's `slice.indices` does; a step of 0 panics with kind `value`. Slices of a `str` are not supported yet, and in strict mode they are E504. W807 is not reported yet.
- **`bigint` literals.** An integer literal where `bigint` is expected is built from pieces of at most 18 digits.
- **Error unions.** `fail A | B` is an anonymous union of the named types, without repeats. `?` injects a member into a union that contains it with no conversion. Matching on a union's members is not supported yet.
- **`post`.** Each `post` is checked at every return, including falling off the end, with `out` naming the result; a failure panics with kind `assert`, as `pre` does. `old(p)` reads a copy of `p` taken on entry. A parameter named `out` in a function with `post` is E230. `post` is not yet emitted as a Yong `ensures` clause.
- **Integer literal range.** An integer literal outside the range of the integer type expected of it, or outside `int` when no sized type is expected, is E201 with the message "does not fit"; 25 has no dedicated code. A negative literal takes the expected sized type like a positive one.
- **Ordering needs `Ord`.** `<`, `<=`, `>` and `>=` on a type that does not implement `Ord` (a record without `derive(Ord)`, `bytearray`) are E221, as for a bound.

## `yong test` readings (18.4, R300, R309)

Assumed in `yong-cli` until the spec says otherwise:

- **Package and ids.** `yong test` runs in the package root. Its modules are the `.xin` and `.wen` files under `src/`, or under the root when there is no `src/`, without `package.xin`. A test's id is its module path, dots for directories, then the function name: `lib.test_price`.
- **What runs.** Only `test_` functions with no value parameters run so far. `ex` clauses, `bench_` functions and property tests (R301) are not run yet, and no verdict is cached.
- **Budget.** Every test gets the stage-0 step and depth budgets (50 million calls, depth 1,000) on its own thread. I/O tests are not told apart yet, so none gets a wall-clock timeout.
- **Test hash.** It is blake3 of the test function's canonical Yong text, prefixed `blake3:`. References are by path, so an implementation change keeps approvals. Test-support items are not yet hashed in, and neither are callees' interfaces.
- **Gate.** Without `--gate`, any failing test fails the run. With `--gate`, a locked test whose hash differs is E014 at the test, one missing from the module set is E015 at its `tests.lock` line, and a failing approved test fails the run. Proposed tests are reported only. A test counts as approved only when its id and hash both match.
- **Output.** `--format json` prints one array of diagnostic records followed by one `"kind": "test"` record per test. The line format prints one line per failing test, ending in `| repro: yong test --only <id>`, and then a count.
