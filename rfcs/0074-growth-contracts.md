# 0074: Growth contracts: check a function's cost class by fitting deterministic step counts

- Status: draft
- Research: [Yan tooling study: borrow and champion](https://claude.ai/artifact/SAaQdxrqitundhjM7DoBxz),
  champion C9; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

A complexity regression (a sort that went from `n log n` to `n^2`) passes every test and shows up as a
timeout in production. Wall-clock benchmarks catch it only when noisy timings cross a threshold.
CodSpeed counts instructions, which removes the noise, but asserts no growth class. Yan can do better
because its step counts are exact: stage-0 steps and interpreter steps are the same abstract count on
every evaluator (R487, R307). This RFC lets a test state a growth class, run the function on a ladder
of generated sizes, and fit the step counts. A deterministic count on a fixed ladder makes the verdict
a pure function of the source hashes, so it is cacheable by verdict key (R19) and cannot flake.

R610 already has `cost steps <= A`, proven by a linear program. That covers linear bounds only and
proves them for all inputs. This RFC covers the classes the LP cannot express (`log n`, `n log n`,
`n^2`, `n^3`) and offers evidence on generated inputs, not proof. The two claim forms stay separate
and share one step definition. The honest status of a fit is stated in D5: it is a test, not a theorem.

At 4fbc37b (2026-11-11) `yan profile`, `yan test --bench` and the verdict cache for benchmarks have no
CLI (study, baseline table). The first implementation therefore targets the interpreter only, behind
`yan test`, and waits on no new capability.

## Decisions

### D1: where a growth claim is written

Options: (a) a clause in the function header beside R610, for example `cost steps in O(n log n)`;
(b) an attribute on the function, as in the study (`@cost(O(n log n))`); (c) a call in a test: a std
function `bench.growth(f, "n log n", gen)` inside a `test_` function.

(a) puts an empirical claim in the header, where it changes the item hash (7.7) and where the R610
analysis would meet a bound it cannot prove: the LP is linear, so every `n log n` clause would be
W851 and compile to a counter that can never be right. (b) has the same hash cost and reuses the name
`cost` for a second meaning. Both also let an agent patch weaken the claim in source; a test sits
behind the test gate (`tests.lock`, protected-target approval) and an agent cannot edit it. (c) needs no
grammar change in any surface and no change to R610. A claim worth showing in `yan doc` can be
promoted to a header clause later, by its own RFC, once the fit has a track record.

Recommended (c). The spelling `bench.growth` is a placeholder for the lead; no new keyword.

- Decision:
- Row:

### D2: what is counted

The counter is the one R610 and R487 define: calls, loop iterations and `alloc` operations of
checked Yan Core, counted before optimization, so the count does not depend on the optimizer (R385).
`bench.growth` takes a `metric`, `steps` (default) or `alloc`, as R610 does. Wall time, cycles and
hardware counters are never used; native builds are not measured. Hardware counters on native code are
R307's business (`bench_` functions, `--diff`) and gate constants, not classes.

Options: (a) the R487 count; (b) the R307 interpreter instruction count (an abstract step is
attributed to an item, which is finer than the R487 definition). (b) varies with interpreter
internals and would make a verdict change across toolchain versions with no source change. Recommended
(a): it is specified, backend independent and already implemented for stage 0.

- Decision:
- Row:

### D3: sizes and inputs

The ladder is `n0, 2*n0, 4*n0, ... , 2^(k-1) * n0` with `n0 = 16` and `k = 8` by default (16 to 2048),
both overridable. Powers of two keep every class value an exact integer: `log n` is the integer
`log2 n`, `n log n` and `n^2` are integer products. The fit then needs no floating point and gives one
answer on every machine.

For each size, inputs come from the parameter's `Arbitrary` generator (R301) at that size, drawn with
a seed fixed by the verdict key, with `m` draws per size (default 5) and the maximum count kept, so the
claim is about the worst of the draws. The size of a value is `len` for collections and strings and the
integer itself for an `int` parameter, as in R610. A test may pass its own generator `gen(n)` where the
default does not reach the interesting case, which is the usual case for sorts (a reverse-sorted input
is rare in random draws).

Options: (a) the above; (b) one draw per size (cheaper, but a lucky draw hides a worst case);
(c) exhaustive worst-case search (not possible in general). Recommended (a), with the limit stated in D5.

- Decision:
- Row:

### D4: the fit

Let `c_i` be the kept count at rung `i`, and `f` the declared class evaluated at that rung's size.
Define the ratio `r_i = c_i / f(n_i)`. If the function stays inside the class, `r_i` is bounded (it
falls toward a constant while lower-order terms fade); if it is in a larger class, `r_i` keeps growing.
The test: `r_last <= (1 + t) * max(r_{k-3}, r_{k-2}, r_{k-1})` with `t = 1/8`. It uses integer
cross-multiplication, so the answer is exact.

Sensitivity: one doubling of a class that differs by a factor `n` (`n log n` against `n^2`) raises the
ratio by well over `t` at every rung; a class that differs by `n^0.1` raises it by about 7 percent and
passes. The test separates classes, not exponents, and a function in a smaller class than declared also
passes, as for any upper bound. The report prints the ladder, the ratios and the best-matching class
among the supported set, so a pass that is far below its declared class shows as such.

Options: (a) the ratio test; (b) non-negative least squares over a basis of the declared class and
all smaller ones, with a residual threshold; (c) the slope on a log-log plot. (b) and (c) need floats
and a tolerance that is hard to explain; (c) cannot tell `n log n` from `n`. Recommended (a).

Supported classes: `1`, `log n`, `n`, `n log n`, `n^2`, `n^3`. Exponential claims are out of scope:
the ladder cannot reach sizes where they are observable inside the step budget.

- Decision:
- Row:

### D5: what a pass means, and the diagnostics

A pass means: on the generated inputs, at these sizes, the count was consistent with the class. It is
not a proof. R610 remains the proof path for what the LP can express, and a function with both a
`cost` clause and a growth test gets both checks. The report says "observed", never "proven" or
"bounded". A failing fit prints the first rung where `r_i` crossed the threshold and the input at that
rung as a `conform`-style repro (a value the test can paste into an `ex` clause or a snapshot). A ladder
stopped by the step budget before rung `k` is reported as inconclusive and fails the test, never passes
silently (practice 13).

Codes: one diagnostic for a refuted fit and one for an inconclusive ladder, assigned when the lead
accepts (the same-PR rule of CLAUDE.md); each lands with its crate symbol, catalogue entry and one case.

Options: (a) both fail the test; (b) inconclusive is a warning. (b) turns "too slow to measure" into a
pass, which is the regression this feature exists to catch. Recommended (a).

- Decision:
- Row:

### D6: running and caching

A growth test is an ordinary `test_` function. Its verdict key is the usual one (R19: the item hashes
it reaches, the seed, the ladder), so it reruns only when something it reaches changed, and `yan test
--no-cache` forces it. It does not need `--bench`, because it reads no clock. The cost is `k * m`
executions at growing sizes; the default ladder sums to about `2 * 2048 * m` input elements per test,
fine for a nightly or a gate and visible in the cache hit rate. Under `yan test --coverage` the growth
runs do not count toward coverage (the generated inputs are a measurement, not a specification).

Constant factors stay with R307: `bench_` functions and `.yan/bench/` baselines (#759) gate the
count at one size. This RFC adds no second baseline store.

Options: (a) as above; (b) only under `--bench`. (b) makes the check opt-in, and an opt-in check does
not stop a regression. Recommended (a).

- Decision:
- Row:

### D7: conformance cases and agents

Cases, proposed as `conform/*` changes (agents never edit `conformance/`): (1) a function of each
supported class passes its own claim and fails the next-smaller class; (2) a quadratic insertion sort
declared `n log n` is refuted with a repro; (3) the verdict is byte-identical on the interpreter run
twice and under a different thread count; (4) a ladder cut by the step budget is inconclusive, not a
pass. The agent loop (`run_tests`, R616) needs no change: growth tests appear as tests, and an agent
that "fixes" a failing one by editing it hits the test gate.

Options: (a) the four cases; (b) only (1). (3) is the determinism law everything else rests on; (4)
is the fail-loud law. Recommended (a).

- Decision:
- Row:

## Spec changes

- 18 (testing, near R301 and R307): a paragraph on growth tests: ladder, the draws, the fit, the
  reading of a pass (D3 to D6).
- std: the `bench.growth` function and the class set (D1, D4), in the section that holds `bench_`
  support.
- 8.4: one sentence stating that a growth test and a `cost` clause share one step definition and do not
  replace each other (D2, D5). R610 is not otherwise touched.
- 25: the two diagnostics of D5, when accepted.
- 24: D7 cases.

Implementation order: interpreter only. Add a step counter query to the interpreter (the W851 counter
of `checked-all` is the same counter and shares one implementation, practice 12), then `bench.growth`
and the fit over exact integers, then caching by verdict key, then the diagnostics and repro. New code
goes in a new module of the test runner, not in the lowering files of practice 5. The change claims the
Working stage on the interpreter; no native result is needed because no native count is used, which the
PR must say.

## Risks and open points

- Generated inputs may never reach a worst case. D3's custom generator is the mitigation, and D5 words
  the result as "observed". A function whose cost depends on a value, not a size, needs a generator that
  sweeps that value; the RFC does not define a multi-parameter size.
- A function whose class is amortized (`push` in a loop) is measured over a whole sequence of
  operations; the test must wrap that sequence. The `n` of the claim is then the length of the sequence.
- The novelty claim in the summary is from memory of CodSpeed, Hypothesis and cargo-bench, not a fresh
  survey.
