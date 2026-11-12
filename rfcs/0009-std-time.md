# 0009: std.time

- Status: accepted (the lead chose "Accept, civil time in this RFC" on 2026-11-01: D2 to D6 accepted, D1 replaced; then "Accept all (D7 to D13)" the same day)
- Research: Date and time (the design behind 7.5, 11.6 and 17.5)
- Rows: R593 to R604

## Summary

The spec already fixes the time model in 7.1, 7.5, 11.6, 14.2 and 17.5:
- `Instant`, `MonoInstant` and `Duration` are i128 nanoseconds.
- Civil types never get an implicit zone.
- The tzdb is pinned data.

The compiler has none of it yet. This RFC settles the open points for all of `std.time`. D2 to D6 cover
exact time and the `time` capability. D7 to D13 cover civil time, `Period`, zones, `ZonedDateTime`
and strftime. The implementation lands exact time first, then civil time, in separate PRs.

## Decisions

### D1: one RFC, exact time implemented first

The draft proposed two RFCs. The lead chose one. The implementation still lands exact time (D2 to D6)
before civil time (D7 to D13), because the second part needs the tzdb data.

- Decision: replaced by the lead's choice, 2026-11-01

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

- Decision: accepted, 2026-11-01
- Row: R593

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

- Decision: accepted, 2026-11-01
- Row: R594

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

- Decision: accepted, 2026-11-01
- Row: R595

### D5: `TimeError`

`TimeError { at: int, reason: TimeReason }` with `TimeReason = Syntax() | Range()`, by analogy with
`EncodingError`:
- `at` is the char offset of the first bad char.
- `Range` covers a field out of range, such as month 13, and a value outside years -9999 to 9999.
- `show` prints `invalid time at N` for `Syntax` and `time out of range at N` for `Range`.

It is the module's only error type. Step 2 adds reasons to it, not new types.

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R596

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

- Decision: accepted, 2026-11-01
- Row: R597

### D7: `PlainDate`, `PlainTime`, `PlainDateTime`

These are the ISO 8601 proleptic Gregorian types. They have no zone and no offset.
- Constructors are `PlainDate(y, m, d)`, `PlainTime(h, mi, s=0, ns=0)` and `PlainDateTime(date,
  time)`. A literal call is checked at compile time (E540, 14.2). At run time an invalid field panics
  with `value`. `try_new(...)` returns `T | None`, following the `try_` convention.
- Reads: `year()`, `month()`, `day()`, `weekday()` (ISO, 1 is Monday), `day_of_year()`,
  `days_in_month()` and `in_leap_year()`. `PlainTime` has `hour()`, `minute()`, `second()` and
  `nanosecond()`. `PlainDateTime` has all of these plus `date()` and `time()`.
- They implement `Eq`, `Ord` and `Hash`.
- `show` prints ISO 8601 extended form: `2024-02-29`, `09:05:00`, `09:05:00.5` and
  `2024-02-29T09:05:00`. Seconds are always printed, and fractional seconds as in D4.
- `T.parse(s) -> T fail TimeError` accepts exactly that form.
- `PlainTime + Duration` wraps at midnight, as Temporal does.
- `PlainDateTime ± Duration` is exact.
- `PlainDate + Duration` stays a type error (11.6).

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R598

### D8: `Period` arithmetic

`Period(years=0, months=0, weeks=0, days=0)` holds one sign (11.6). A mixed sign panics with `value`.
- `d + p` adds in this order: years, then months, clamping the day to the month's end, then weeks and
  days. So `2024-01-31 + P1M` is `2024-02-29`. This is Temporal's default `constrain`.
  `checked_add` returns `None` outside years -9999 to 9999.
- `a.until(b) -> Period` returns years, months and days, with no weeks, such that `a + p == b`.
- `PlainDateTime.until` returns `(Period, Duration)`, as 11.6 requires.
- `show` prints ISO 8601: `P1Y2M`, `P3W`, `P0D` and `-P1D`.
- `Period.parse` accepts the same form.

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R599

### D9: `TimeZone` and `TzDb`

`TimeZone` is an IANA ID resolved against a `TzDb`.
- `TimeZone(id)` uses the pinned `std.tzdb`. A literal ID is checked at stage 0, and an unknown ID is
  a compile-time error. `TimeZone.parse(id) -> TimeZone fail TimeError` handles IDs read at run time.
- `TimeZone.utc()` returns UTC, and `TimeZone.fixed(offset_seconds)` returns a fixed offset, shown as
  `+05:30`.
- `tz.id()` returns the ID.
- `TzDb.get(id)` is the lookup into a database that a capability returned (7.5).
- `Eq` compares by ID.

The interpreter takes the zone rules from Jiff's bundled tzdb, at the version pinned in `yan.lock`.

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R600

### D10: `ZonedDateTime`

- `i.in_zone(tz)` is total.
- `dt.in_zone(tz, d)` takes a required `Disambiguation`, one of `Compatible()`, `Earlier()` or
  `Later()`, as in Temporal. There is no default.
- `dt.in_zone_exact(tz) -> ZonedDateTime | None` returns `None` in a gap or a fold.
- Fields: `z.instant`, `z.plain`, `z.zone` and `z.offset_seconds()`.
- `z + Duration` is exact.
- `z + Period` adds on the wall clock and resolves again with `Compatible`. A `Duration` in days or
  weeks is W817 (14.2).
- `show` prints RFC 9557 form, for example `2024-03-10T03:00:00-04:00[America/New_York]`.
- `parse` requires the bracketed zone, and fails when the offset does not match the zone.
- Equality is structural and there is no ordering (11.6).

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R601

### D11: strftime and strptime

The directive set is Jiff's (17.3): POSIX plus `%f`, `%.f`, `%:z` and `%Q`.
- `x.strftime(fmt) -> str` is defined on each civil type, on `ZonedDateTime` and on `Instant`.
  `Instant` formats in UTC.
- `T.strptime(s, fmt) -> T fail TimeError` parses.
- A literal `fmt` is checked against the type at stage 0 (E542). For example, `%z` on a
  `PlainDateTime` is an error.
- Names (`%a`, `%B`, `%p`) are English only. Localized formats are `@yan/i18n` (R334).

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R602

### D12: `env.local_zone()` in tests

`env.local_zone()` is the only way to reach the host's zone (17.5).
- The `os` handler reads `TZ`, then the platform setting, and falls back to UTC.
- The test `env` handler returns UTC unless the test binds another zone, so test output never depends
  on the machine's zone.

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R603

### D13: `TimeError` reasons for civil time

`TimeReason` gains two reasons:
- `Zone()` for an unknown ID. `show` prints `unknown time zone at N`.
- `Offset()` when a parsed offset does not match its zone.

A gap or fold is never an error: `in_zone` resolves it, and `in_zone_exact` returns `None`.

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R604

## Spec changes

- 17.5: the API of D2 to D5 and D7 to D13.
- 11.6: one sentence saying that `Duration` is opaque until units land, with the same API (D2).
- 7.1 and 7.5: handler behavior (D6, D12).
