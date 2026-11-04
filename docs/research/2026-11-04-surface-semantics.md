# Should surfaces change semantics?

Study, 2026-11-04. Question (Antonio): what if surfaces were allowed to change semantics and add
features, instead of being syntax over one core? Read against main `10c8c14`: law 1 (2), 16.5 to
16.7, R568, R768, RFC 0029, `docs/architecture.md`, and the K1/N1 memo of 2026-11-03.

## 1. Where semantics can live

Yan has one checker and one evaluator, both on checked Yan Core (interpreter as oracle, MIR and
Cranelift below it). A surface's meaning can therefore sit in only three places:

| Route | What it is | Who may use it today |
| --- | --- | --- |
| **Lowering** | The surface elaborates a form into explicit Yan Core. Implicit widening, `None`, `defer`, for-else, try/catch over typed failures are all "semantics" in the surface and plain core below. | Every surface (law 1, K1, N1). |
| **Core extension** | A new core construct (26.6 research rows: effects with resume, staging, `Any`, `recover`, STM, `eval`), then spelled by whichever surfaces want it. | Every surface once the extension lands; Yan Meta tries it first behind `@lab` (R777, R778). |
| **Engine** | The surface runs its items on its own interpreter or compiler, linked through a boundary signature like an extern (R568). | Niche surfaces only. |

"Let surfaces change semantics" means one of two things. Either heavier lowering, which law 1 already
allows as long as the Yan Core text stays explicit (law 5). Or the engine route for Yan Agent, Yan
and Yan Meta, which is the real change. The rest of this note is about the second.

## 2. What it would break

1. **Shared conformance.** One `yir` case today tests the meaning of every surface at once; the `yag`
   and `yn` columns test syntax and elaboration. An engine needs its own semantic column, including
   determinism under two hash keys and `yan test --ub` (R568 rule 5). Those cases are Antonio's to
   write, because agents never edit `conformance/`. For Yan Agent that is a second copy of most of
   the 528-case suite (estimate), on the one person who owns the gates.
2. **Law 1 and the round-trip laws.** Round-trip law 3 (a Yan module in the Yan Agent subset
   elaborates as its projection) and N7 stop holding: `yan view --as yn` of an engine item prints the
   signature and "semantics (R568)", not the body. Projection becomes translation.
3. **Determinism and capability safety.** Under lowering, laws 2, 4, 6, 7 and the capability ceiling
   hold by construction, because the core checker sees every body. Under an engine they hold only if
   the engine is correct; the engine is trusted code, like FFI. Yan Agent is where untrusted model
   output enters. Moving it off the checker puts the weakest code on the least-checked path, against
   the security model (RFC 0026), which audits in the Yan view.
4. **Tooling.** `yan-db`, `yan-ide`, `yan mcp`, the patch protocol (18) and item hashing all work on
   HIR and checked core. Engine items are opaque to them: no inlay for the five facts, no patch below
   the signature, a hash of surface text plus engine identity instead of the core tree.
5. **Formal model.** The Lean model (RFC 0022) covers Yan-K. Each engine would be an unverified
   semantics beside it.
6. **Learnability.** Today a reader learns one meaning and several spellings. With divergence, the
   same-looking code in `.yag` and `.yn` can mean different things, and the Yan view stops being a
   faithful reading of what the model wrote.

## 3. What it would gain

1. **Agent token cost.** The real prize. Where Yan's meaning differs from Python's (exceptions,
   truthiness, implicit conversion, `None`), models pay repair rounds. But almost all of these can be
   taken by lowering: Yan Agent can accept the Python-shaped form and elaborate it into the explicit
   core, which the Yan view then shows. That keeps every guarantee. Divergent semantics are needed
   only where Python's behavior cannot be expressed in core at all (for example unchecked exceptions
   crossing items), and those are exactly the behaviors law 5 exists to exclude.
2. **Domain fit.** Already granted where it pays, to niche surfaces (R568: three-valued logic,
   inference as evaluation, clocked time).
3. **Experiment speed.** A semantics change could be tried in one surface without core work. This is
   the job Yan Meta already has (R776, RFC 0029): its lifecycle funnels forms from `experimental` to
   graduated, and forms that need a core change wait on 26.6. Letting every surface add semantics
   would duplicate Yan Meta and remove the funnel that makes it a canary.

## 4. Options

- **A. Keep today's rule.** Lowering for all, core extensions via 26.6, engines for niches only. No
  cost.
- **B. Extension gates in every surface.** A core extension under trial is enabled per module by the
  same `@lab(...)` line in any surface, not only in Yan Meta. All guarantees hold (it is core). Cost:
  one row amending 16.6 and the Yan Agent and Yan grammars as each extension needs them; a few
  conformance cases per extension. Value only once the first 26.6 row lands.
- **C. Elaboration profiles.** Each surface states its elaboration defaults (literal types, implicit
  widening, `None`, conversions) in one table, all lowering to explicit core. This is a clarification
  of law 1, not a change. Cost: one row, no code.
- **D. Lab engines for Yan Meta.** Yan Meta may run an `@lab` form on an engine to prototype a 26.6
  row before core work. Cost: an engine crate, a conformance column, opaque items in the lab. Weak
  value: the lab's evidence rule (R777) needs `yan view --as yn` to read the form, which an engine
  item cannot give.
- **E. Any surface may diverge.** Breaks everything in section 2. Rough cost: a semantic conformance
  column per surface, a second evaluator per surface, the formal model covering only part of the
  language, and the review and security story rebuilt. For one part-time developer this would stall
  M1 for months (estimate).

## 5. Recommendation

Keep law 1 for Yan Agent, Yan and Yan Meta (option A), and get the gain through lowering:

1. **Now, inside Plan A, no spec change.** In the R567 runs (Plan A step 3), tag each failure or
   repair round caused by a Python-semantics expectation. Each tag becomes either a Yan Agent lowering
   proposal (legal under law 1) or evidence for a 26.6 extension. This is where the token gain is,
   and it gets measured instead of assumed. Cost: a field in the run report.
2. **With the first evidence, option C.** If two or more lowering proposals come out of the runs,
   write the elaboration-profile table in one row so each surface's defaults are stated in one place.
3. **When the first 26.6 extension lands, option B.** Open its `@lab` gate to every surface.
4. **Decline D and E.** Revisit E only if a run shows a measured agent gain that lowering cannot
   reach, and then as a niche surface under R568, not as a change to Yan Agent.

Rollout cost of the recommendation: zero before M1; one or two rows after it, each with a handful of
conformance cases for Antonio.
