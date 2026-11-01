# 0009: std.time, exact time first

- Status: draft
- Research: [Date and time](https://claude.ai/artifact/MoSWSGCoXQ4zqGm8RtKvVd) (the design behind 7.5, 11.6 and 17.5)
- Rows: filled in when accepted

## Summary

The spec already fixes the time model in 7.1, 7.5, 11.6, 14.2 and 17.5:
- `Instant`, `MonoInstant` and `Duration` are i128 nanoseconds.
- Civil types never get an implicit zone.
- The tzdb is pinned data.

The compiler has none of it yet. This RFC builds `std.time` in two steps. The first step is exact time
with the `time` capability and its test handlers. The second step is civil time: `PlainDate`,
`PlainTime`, `PlainDateTime`, `ZonedDateTime`, `Period`, `TimeZone`, `TzDb` and strftime. That step
needs the `std.tzdb` data package and the E540 to E542 literal checks, so it gets its own RFC. This RFC
also settles the points the spec leaves open for the first step: the `Duration` API before units
exist, `Show`, parsing, and the error type.

## Decisions

### D1: two steps, exact time first

Step 1 is `Duration`, `Instant`, `MonoInstant`, the `time` capability, and the `os`, `fixed_time` and
`virtual` handlers. Step 2 is the civil types, zones, `Period` and strftime/strptime, in a later RFC.
Step 1 is useful on its own, because it covers timeouts, benchmarks, logs and timestamps on the wire,
and it does not depend on tzdb packaging.

Recommendation: accept.

### D2: `Duration` before units

11.6 makes `Duration` the type `Quantity[i128, ns]`. Units are not implemented yet, so until they are,
`Duration` is an opaque type over i128 nanoseconds. Its API is chosen so that nothing changes when it
becomes the quantity type.

- Constructors: `Duration.nanos(n)`, `.micros(n)`, `.millis(n)`, `.seconds(n)`, `.minutes(n)` and
  `.hours(n)`, each taking an `int`. There are no `days` or `weeks`, because those are `Period`
  units (W817).
- Reads: `d.nanos() -> int128`, `d.seconds() -> int` (truncated toward zero) and
  `d.as_seconds_float() -> float`.
- Arithmetic follows Python's `timedelta`:
  - `+` and `-` between durations, and unary `-`.
  - `d * n` with an `int`.
  - `d // n` and `d % n` with an `int`, flooring.
  - `d / d -> float` and `d // d -> int`.
  - `d / n` with an `int`, rounded half-even to the nanosecond.
- `abs()`, `Eq`, `Ord` and `Hash`.
- Overflow panics with `overflow`, and `checked_add`, `checked_sub` and `checked_mul` return `None`,
  as 11.6 requires.

Recommendation: accept.

### D3: `Instant` and `MonoInstant`

`Instant` has the following API:
- `Instant.from_unix_seconds(n)` and `Instant.from_unix_nanos(n)`.
- `i.unix_seconds()`, which floors, and `i.unix_nanos()`.
- `Instant - Instant -> Duration` and `Instant ± Duration -> Instant`.
- `Eq`, `Ord` and `Hash`.

`MonoInstant` has the same arithmetic and `Eq` and `Ord`, with no Unix conversion and no `Show`
beyond `MonoInstant(<ns>)`. Its epoch is unspecified and only differences mean anything.
`Instant + Period` stays a type error (11.6).

Recommendation: accept.

### D4: `Show` and parsing are RFC 9557, strict

`show(i)` prints UTC in RFC 9557 and RFC 3339 form, `2000-01-01T00:00:00Z`. Fractional seconds are
printed only when they are non-zero, with trailing zeros dropped, as Temporal's
`Instant.prototype.toString` does.

`Instant.parse(s) -> Instant fail TimeError` accepts RFC 3339 with `Z` or a numeric offset, and
ignores an RFC 9557 bracketed suffix only when it is `[UTC]` or `[Etc/UTC]`, or matches the
offset. It rejects:
- a lowercase `t` or `z`;
- a space in place of `T`;
- a missing offset;
- leap second 60;
- more than 9 fractional digits.

`show(d)` prints ISO 8601 durations as Temporal does: `PT1.5S`, `PT0S`, `-PT2H30M`. It uses hours,
minutes and seconds only, because a `Duration` has no calendar units.

Recommendation: accept.

### D5: `TimeError`

`TimeError { at: int, reason: TimeReason }` with `TimeReason = Syntax() | Range()`, by analogy with
`EncodingError`:
- `at` is the char offset of the first bad char.
- `Range` covers a field out of range, such as month 13, and a value outside years -9999 to 9999.
- `show` prints `invalid time at N` for `Syntax` and `time out of range at N` for `Range`.

It is the module's only error type. Step 2 adds reasons to it, not new types.

Recommendation: accept.

### D6: handlers in the interpreter

The `time` capability operations are already in 7.1: `now() -> Instant`, `monotonic() -> MonoInstant`,
`sleep(d)` and `sleep_until(m)`. Step 1 implements them with three handlers:
- `os` reads the host clocks. `monotonic` includes suspended time, as 7.1 requires.
- `fixed_time(i)` (17.3) returns `i` from every `now()`. `monotonic()` starts at 0 and advances only
  through `sleep`.
- `virtual(start)` advances only through `sleep` and `sleep_until`, and under the seeded scheduler only
  at quiescence (R396). This is the test default at `2000-01-01T00:00Z`.

There is no spec change beyond what the rows above add. This item records the order of the work.

Recommendation: accept.

## Spec changes

- 17.5: a "Step 1 API" paragraph with D2 to D5.
- 17.3 time row: unchanged.
- 11.6: one sentence saying that `Duration` is opaque until units land, with the same API (D2).
