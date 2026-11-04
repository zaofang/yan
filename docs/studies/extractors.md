# Extractors: user-defined match patterns for Yan

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no new diagnostic
codes. Items are labelled D1, D2 and so on; none carries a row number. Everything here is
non-normative until an RFC and accepted rows say otherwise.

Brief: item F8 of the [language forums study](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT).
Study extractors, patterns whose matching logic is a user function, as in Scala `unapply`, Python
`__match_args__` and F# active patterns. Cover prior art, exhaustiveness, determinism, capabilities,
lowering to Yan Core, and a recommendation.

## 1. Answer

Add one attribute, `@pattern`, on an ordinary function of one `let` parameter. Its name may then appear
in pattern position, `case even(k)`, wherever a pattern is allowed (`match`, `is`, `let ... else`).
The function's return type decides what kind of extractor it is:

- **Partial**, `-> Option[T]`: the pattern matches when the result is `Some(v)` and `v` matches the
  sub-pattern. It covers nothing for exhaustiveness, exactly like a guard outside the 8.6 fragment.
- **Total**, any other type: the pattern always runs and matches when the result matches the
  sub-pattern. If the result type is a sum, the arms that apply the same total extractor at the same
  place are checked for exhaustiveness as a match on that sum. This is Wadler's views and F#'s
  complete active patterns, with the case names taken from an ordinary nominal sum.

An extractor uses no capability except `alloc`, the rule contracts already follow (8.4), so it cannot
make arm order observable and the match stays deterministic. It may return owned values or views with
`from` (4.5). It lowers to a call and a match on its result, so Yan Core gains no new form: only the
attribute, which Core already accepts on items, and which the validator checks. Law 1 (one semantics) and
law 7 (deterministic by default) hold by construction.

Not proposed now: extra arguments to an extractor (`case prefix("GET ")(rest)`), extractors as trait
methods, trusted completeness groups, and facts drawn from an extractor's `post`. Each is a D-item.

## 2. The gap today

Confirmed against the spec. Patterns are fixed syntax: literals, ranges, variant and record patterns,
tuples, slices, or-patterns and named fields (3, `Pat`; 13, `pattern`; 15, Patterns). They can only
take apart a value's declared representation. Three things follow.

1. **Abstraction leaks or patterns go.** A type that hides its fields (a `newtype` with a `where`
   predicate, a record whose fields are not `pub`, a `@non_exhaustive` sum) cannot be matched on any
   shape other than its representation. A `Queue` stored as two lists cannot offer `Empty | Cons(h, t)`.
   A `Complex` stored as `(re, im)` cannot also offer polar form. The author either exposes the
   representation, which makes it API (20.6), or callers give up on `match`.
2. **Guards repeat work.** `case s if parse_int(s).is_some()` cannot bind the parsed value, so the arm
   parses again: `let n = parse_int(s).unwrap()` with a panic site the solver must discharge. Every
   regex, prefix and parse dispatch in a request router or a lexer has this shape.
3. **No nesting.** Matching `Request(path: p, ..)` where `p` must start with `/api/` and the remainder
   bound is a guard plus a slice call, and it cannot sit inside a larger pattern or an or-pattern.

Today's workaround is a function returning a sum, then `match f(x)`. It works for one level and loses
nesting, or-patterns, and the ability to mix representation and view arms in one match.

## 3. Prior art

| Language | Mechanism | Exhaustiveness | Effects | Notes |
| --- | --- | --- | --- | --- |
| Scala 2 and 3 | `unapply` / `unapplySeq` on an object, returning `Boolean`, `Option[T]`, or (Scala 3) any type with `isEmpty`/`get` or a product | Scala 3 counts an extractor irrefutable when its result type is a product or `Some`; others cover nothing | any; called once per case, unspecified sharing | The general model; the main complaint is that partial extractors silently break exhaustiveness and that call counts are unspecified |
| F# | active patterns: complete `(\|Even\|Odd\|)`, partial `(\|Int\|_\|)`, parameterized `(\|Prefix\|_\|) "GET "` | complete ones are checked total; partial cover nothing | any | Complete patterns define their case names inline; the result is evaluated once per match |
| Haskell | `ViewPatterns` `(f -> p)`; `PatternSynonyms` with `{-# COMPLETE #-}` | view patterns cover nothing; `COMPLETE` sets are trusted, not checked | pure by the language | `COMPLETE` is unsound if wrong: a missing case is a runtime failure |
| OCaml | none in the language; `ppx_view` and proposals | n/a | n/a | Long-standing request; the core team cites the loss of exhaustiveness and the cost model |
| Python | `__match_args__` gives positional class patterns; a custom `__match__` was in PEP 622 and dropped | no exhaustiveness in the language; type checkers treat class patterns as `isinstance` | any | PEP 622 deferred custom matching: hard to specify, and it hid arbitrary code in `case` |
| C# | `Deconstruct` methods give positional patterns; property patterns | `Deconstruct` is total and is used for exhaustiveness of tuples | any | Total, product-only extractors: the most conservative mainstream design |
| Swift | `~=` operator overloads give expression patterns | expression patterns cover nothing | any, may throw | Matching on user `~=` is a frequent source of surprise in forum threads |
| Rust | none; RFC 2687 "deref patterns" for `Box`, `String`, `Vec` behind a std trait | deref patterns are treated as transparent | std types only, trusted | Rust chose to extend built-in patterns through a sealed trait rather than open extractors |
| TC39 | pattern-matching proposal, `Symbol.customMatcher` | none | any | Same Scala shape; the proposal's open issues are evaluation order and caching |

Lessons. Every design that lets an arbitrary function run inside a pattern meets the same three
problems: arm order and call counts become observable through effects (Scala, Swift, TC39), partial
patterns erode exhaustiveness (all of them), and trusted completeness is unsound (Haskell `COMPLETE`).
F# complete active patterns avoid the second without trust, because the case set is declared with the
extractor. Python dropped custom matching rather than solve the first. Yan already has the pieces that
answer all three: a pure fragment, nominal sums, and a solver that treats unknown guards as covering
nothing.

## 4. Semantics

### 4.1 Declaration

```
@pattern
even(n: int) -> int? = if n % 2 == 0 { n / 2 } else { None }

@pattern
polar(c: Complex) -> (f64, f64) = (c.abs(), c.arg())

type QueueView[T] = Empty() | Cons(head: T, rest: Queue[T])

@pattern
front[T](q: Queue[T]) -> QueueView[T] = ...
```

In Yan Agent the same functions are `@pattern def even(n: int) -> int | None:` and so on; in Yan Core,
`@pattern fn even(let n: i64) -> Option[i64]`. Rules:

- Exactly one parameter, passed `let`. `inout` and `sink` are rejected: a pattern must not change or
  consume the value being matched, since later arms still see it.
- Generic parameters are allowed and inferred from the scrutinee.
- The body follows the contract fragment of 8.4: calls only to functions using no capability other
  than `alloc`. It may not `fail`. It may panic, and a panic in an extractor is a panic of the match.
- `@pattern` is API. On a `pub` function it appears on the item page (18.9, R314) as a badge, and
  changing an extractor from total to partial is a major change under 20.6.

### 4.2 Use

The extractor's name, a lowercase value path, takes one sub-pattern for its result: `even(k)`,
`polar((r, theta))`, which may be written `polar(r, theta)` when the result is a tuple, as a variant
pattern is. Use is legal anywhere a pattern is: nested inside variant, record, tuple and slice
patterns, inside or-patterns (each alternative binds the same names, as today), in `e is P`, and in
`let P = e else d`. A `let` with a total extractor whose sub-pattern is irrefutable needs no `else`:
`let polar(r, theta) = c`.

Name resolution is unambiguous in every surface because of the naming rules of 13.1: types and
variants are PascalCase, values lowercase, so `case even(k)` cannot be read as a class or variant
pattern. A lowercase path in pattern position that names a function without `@pattern` is an error
with the fix "add `@pattern`" when the signature fits.

### 4.3 Ownership

The scrutinee is borrowed for the extractor call, as for any `let` argument. The result is owned unless
the extractor returns a view with `from` its parameter, in which case the bindings are views over the
scrutinee and follow 4.5 unchanged: they are second-class and the scrutinee stays borrowed until their
last use. This gives zero-copy patterns such as `case strip_prefix_api(rest)` on a `StrView`. An owned
binding is the extractor's result, so binding it moves nothing out of the scrutinee.

## 5. Exhaustiveness

The checker of 8.6 (R606) extends as follows.

1. **Partial extractor.** The arm's pattern at that place covers nothing, exactly like a guard outside
   the fragment. A match that needs it for coverage still needs a fallback arm (`_` on a scalar; on a
   sum, the remaining variants). W853 applies unchanged to an arm no value can reach.
2. **Total extractor, non-sum result.** The pattern covers the scrutinee's whole space when its
   sub-pattern is irrefutable (`polar(r, theta)`), and nothing otherwise. Coverage does not reach
   through the function: a total extractor with a literal sub-pattern covers nothing.
3. **Total extractor, sum result.** Arms that apply the same extractor at the same place (same path in
   the pattern tree) form a group. The group covers the scrutinee's whole space when its sub-patterns
   are exhaustive over the result sum, checked by the existing sum and solver rules. E503's witness is
   then a variant of the result sum, written through the extractor (`front(Cons(_, _))`), and its fix
   inserts that arm.
4. **Mixing.** A match may mix representation arms and extractor arms. Each group is checked on its
   own space; the match is exhaustive when some group or the representation arms cover the scrutinee.
   The solver does not relate an extractor's result to the representation, so `case Queue(front: [], ..)`
   and `case front(Empty())` are independent facts.

No completeness is trusted. A Haskell-style `COMPLETE` set across partial extractors is out (D7): a
wrong declaration would turn E503 into a runtime panic, the outcome the exhaustiveness check exists to rule out.

## 6. Determinism and evaluation

An extractor is pure, so its result depends only on its argument, and the number of times it runs is
not observable except through cost and panics. Both are pinned so the cost model and replay stay exact:

- Arms are tried top to bottom, sub-patterns left to right, and an arm's extractors run before its
  guard. This is the order a reader assumes and the order Yan Agent already uses.
- An extractor applied to the same place of the scrutinee runs **at most once per match**, and its
  result is shared by every arm that applies it there. A grouped total extractor (5.3) runs once.
- An extractor is not run if an earlier arm matched, or if an earlier sub-pattern of the same arm
  failed.

Because the function uses no capability, it needs no replay record, and `explore` and `seeded`
schedules (6.9) cannot interleave with it. A `comptime` match may call extractors, since a pure
function is comptime-evaluable (10.4).

## 7. Capabilities

The rule is the one contracts and shared initializers already follow (8.4, R393): no capability
except `alloc`. The alternatives were weighed:

- **Any capability, declared with `uses` at the match.** Scala, Swift and TC39 allow this. It makes
  arm order, sharing and short-circuiting part of the program's effects, so a refactor that reorders
  arms changes behaviour, and the at-most-once rule of 6 becomes a semantic promise rather than an
  optimisation. It also hides I/O inside `case`, which Python cited when it dropped `__match__`.
- **`nondet` only.** No use case found: a pattern that reads a clock or a random source is a bug.
- **`alloc` only.** Covers every use case in section 2: parsing, prefix tests, polar form, queue views,
  regex with a precompiled `Regex` value. Recommended.

An extractor that needs an effect is written as a function call before the `match`, which makes the
effect visible where it runs. Extractors add no capability, no new authority and no channel for
untrusted data; a capability value cannot be a scrutinee's sub-value bound through an extractor,
since an extractor cannot return one (a capability is second-class, 7.2).

## 8. Lowering to Yan Core

No new Core pattern form. `yan-elab` lowers an extractor pattern to a call and a match on its result,
reusing join points (3, R764). For

```
match s {
  case even(k) if k > 10 => a(k)
  case even(k) => b(k)
  case _ => c()
}
```

the elaborated Core is, in outline:

```
jp out(r: R) { ... }
let e0: Option[i64] = even(s)
match e0 {
  Some(k) => {
    if gt_i64(k, 10) { jump out(a(k)) }
    jump out(b(k))
  }
  None => { jump out(c()) }
}
```

The shared `e0` is the at-most-once rule of 6. Where arms interleave representation and extractor
patterns, the decision tree splits at the first extractor: arms above it compile to the usual tree,
the extractor call is bound once, and the remaining arms compile against both. Call sites are
ordinary calls, so `yan-opt` inlines small extractors and the result is the same code as the manual
`match f(x)` form. Validation in checked Core: the `@pattern` item has one `let` parameter and a body
inside the contract fragment; the elaborated matches are checked as any other.

The Core printer keeps the call form, so a decompiled or projected program shows extractors as calls.
Surface round-trip (16) restores the pattern form from a provenance mark on the elaborated match.

## 9. Surfaces and agents

- **Yan Agent.** Python's `__match_args__` is already covered: class patterns take positional
  sub-patterns in field order (13, `parg`). `@pattern def` is the extractor form; it reads as Python
  and lints like it. The lenient reader maps `__match_args__` declarations to nothing (field order is
  fixed) with a note.
- **Yan.** `@pattern` on a function as in 4.1; patterns as in 4.2.
- **Agent loop.** An E503 witness through an extractor is a normal fix-it (18.2), inserting
  `case front(Cons(_, _)):`. An agent writing `case s if parse_int(s) is not None` followed by a
  re-parse gets a lint suggesting the extractor form when one exists in scope; that lint is optional
  (D10).

## 10. Decisions

| Item | Question | Recommendation |
| --- | --- | --- |
| D1 | Add user-defined patterns at all? | Yes, as `@pattern` functions; they close the abstraction and guard-repetition gaps of section 2 |
| D2 | Declared marker or Haskell view patterns `(f -> p)` with any pure function? | Marker: it is API, gets a doc badge and stability rules, and keeps pattern syntax free of expressions |
| D3 | Which result shapes? | `Option[T]` is partial; any other type is total; a sum result gives grouped exhaustiveness (5.3). No `bool` form: `even(_)` over `Option[unit]` covers it |
| D4 | Capabilities in an extractor? | None but `alloc`, the contract fragment of 8.4; no `fail` |
| D5 | Parameter convention? | Exactly one `let` parameter; result owned or a view `from` it |
| D6 | Evaluation order and count? | Top to bottom, left to right, extractors before guards; at most once per scrutinee place per match, guaranteed |
| D7 | Trusted completeness sets across partial extractors (Haskell `COMPLETE`)? | No: unsound; use a total extractor returning a sum |
| D8 | Extra arguments (F# parameterized patterns, `prefix("GET ")(rest)`)? | Defer; revisit with a use count from the R567 corpus. Constant arguments only if adopted |
| D9 | Extractors as trait methods (generic `Seq` patterns)? | Defer; monomorphization makes it possible, but it couples patterns to trait resolution |
| D10 | Use an extractor's `post` as solver facts (`even(k)` gives `n == 2 * k`)? | Defer; first cut treats extractor results as opaque, and a lint suggesting extractors over guard-then-reparse is optional |
| D11 | Rust-style deref patterns for `Box`, `Rc`, `Str`? | Separate item; they are representation patterns through std types, not user extractors |

## 11. Cost

Parser: none in Yan Agent and Yan (the pattern grammar already accepts `path(args)`); resolution
distinguishes value paths. Checker: the `@pattern` validity rule, the fragment check reused from
contracts, and extractor groups in the exhaustiveness pass (5.3). Elaboration: the call-and-match
lowering with shared results. Docs: the item-page badge. Conformance would need cases for partial
coverage, grouped totals with an E503 witness through the extractor, the at-most-once rule (observed
through a panic or a cost bound), a view-returning extractor, an `inout` parameter rejected and a
capability use rejected; those go in a separate PR by the lead.
