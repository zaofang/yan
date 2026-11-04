# Yan Law 言法 (`.ylaw`): a syntax and semantics study

Status: brainstorm study, 2026-11-04. Not normative. It proposes no rows and no diagnostic codes; the
labels D1 to D9 at the end are items an RFC could take up. Where this page and `spec/` differ, the
spec wins. Every code block is a design sketch.

Today the surface is one research row (R549, spec 16.7) and one example (RFC 0002 D14, copied into
`docs/surfaces.md` section 18 with a mock grammar). This study asks what Yan Law should be, not only
what it should look like.

## Contents

1. Methodology and process
2. Findings: the field, what users want, what works, what fails
3. The five shared tasks
4. Five proposals, one axis each
5. Experiment: the default calculus, run
6. Comparison
7. The mix: consolidated proposal
8. Lowering and fit with the core
9. Open questions and items for an RFC
10. Sources

## 1. Methodology and process

**Read in the repo, narrowly.** `docs/surfaces.md` (status table, the shared mock rules, section 18);
RFC 0002 D14 and D15; spec 16.7 (N1 to N9, R549, the R568 boundary rules); spec 11.7 (`dec128`,
`Money`, `quantize`, `RoundingMode`); spec 17 lines on `std.time` (`PlainDate`, `Period`) and
`units` (`pct`, `bp`); spec 15 lines on contracts (`pre`, `post`, `ex`) and soft keywords;
`examples/tally/package/src/fees.yn` for real Yan money code. Nothing else in `spec/` or
`conformance/` was read.

**Searched.** Catala releases and adoption (Catala 1.x, the Cnaf and Inria partnership of June
2026); the IRS Fact Graph; LLM statute-to-code benchmarks (NLLP 2025, ICML 2026, an arXiv study on
the Internal Revenue Code with Prolog); Logical English and s(CASP); L4. The rest of section 2 is
from the established literature, cited in section 10 with a confidence mark: **H** read or well
established, **M** known but not rechecked today, **L** recalled, verify before relying on it.

**Tried.**

1. Fixed five tasks that every proposal must write (section 3), chosen to hit the hard parts of
   statutes: arithmetic with rounding, exceptions to exceptions, amendments in time, obligations
   with deadlines, and terms that need human judgment.
2. Wrote five proposals, each pushed to one extreme: fidelity to the text, brevity, agent token cost,
   safety and explicitness, expressive power. Each writes the same tasks.
3. Built a 80-line prototype of the default calculus in Python
   ([`ylaw/defaults.py`](ylaw/defaults.py)) and ran it on task T2 to see whether gap and conflict
   checks find real drafting bugs (section 5).
4. Measured the samples with a crude size proxy (section 6).
5. Took the best idea from each proposal into one consolidated design (section 7) and checked it
   against the spec's admission rules N1 to N9 (section 8).

**Brief from Antonio.** Elegant, concise, powerful, expressive, opinionated without being
obnoxious; for people who value efficiency and can appreciate craft. Vanguard ideas welcome; the
output is frameworks, not a final grammar.

## 2. Findings

### 2.1 The field

| System | Shape | What it gets right | Where it hurts |
| --- | --- | --- | --- |
| Catala (Inria, 2019 on; ICFP 2021) | Literate scopes; statute text interleaved with code; default calculus for exceptions | Isomorphism with the text; exceptions as first-class; pair programming with lawyers; compiles to OCaml, Python, C, JS; Cnaf chose it for benefits engines (June 2026); 1.x has a test UI in VS Code | Verbose English keywords; scope and context-variable model takes time to learn; labels and exceptions spread across files |
| Mlang / M (French DGFiP, 1990s; CC 2021) | The tax administration's own DSL | Ran the French income tax for decades | Unreadable to lawyers; no exceptions; the reason Catala exists |
| OpenFisca (France, 2011 on), PolicyEngine | Python formulas over entities and periods; dated parameters in YAML | Population microsimulation; parameters versioned by date; large reform modelling community | Python's semantics: no exception structure, floats in places, weak link to the text |
| IRS Fact Graph (Direct File, open sourced May 2025) | Scala graph of derived facts | Knows which facts are still missing, so it drives an interview; incremental | Not a language people write; XML-ish definitions; US-only |
| Logical English (Kowalski), Blawx + s(CASP) | Controlled English or blocks over answer-set Prolog | Reads like law; s(CASP) gives justification trees in English | Controlled English is brittle to write; negation as failure surprises; performance |
| L4 (Singapore, CCLaw at SMU and Legalese) | Spreadsheet and text DSL for contracts and rules | Deontic forms (MUST, MAY, SHANT) with deadlines | Small community; several syntaxes over time |
| DMN and FEEL (OMG) | Decision tables with hit policies | Business analysts read tables; standard | Hit policy `first` makes row order semantic; FEEL is weak for law-scale logic |
| Cedar (AWS, OOPSLA 2024), Rego | Policy languages | Cedar: forbid overrides permit, SMT analysis of policy sets, formal model in Lean | Authorization only; no money, time or rounding |
| Contract DSLs: CSL, Symboleo, Marlowe, Stipula, Lexon | Obligations, deadlines, state | Obligations as state machines; contrary-to-duty (what happens on breach) | Contracts, not statutes; Lexon's English is ambiguous |
| Standards: Akoma Ntoso (LegalDocML), LegalRuleML | XML for legal text and rules | Stable identifiers for every provision; versioning by date | Not for people to write |

### 2.2 What users want

From the rules-as-code literature (OECD OPSI 2020, the Catala papers, the Robodebt report) and the
tools above, in order of how often it comes up:

1. **Trace to source.** Every number points to the provision that made it. Auditors and courts ask
   "under which section?" first.
2. **Explanation for the citizen.** Not a stack trace: "you got 600 euros because of s. 4(3)."
3. **Tests that lawyers write.** Worked examples from guidance notes, run as tests.
4. **Time.** Laws are amended; a claim for March 2026 uses the law in force in March 2026, even when
   computed in 2027. Retroactive amendments exist.
5. **Exact money and stated rounding.** Statutes say where rounding happens; code that rounds
   elsewhere is wrong by a cent and gets sued at scale.
6. **Discretion is not computed.** "Reasonable excuse" and "ordinarily resident" are decided by
   people. A system that guesses them is the Robodebt failure mode.
7. **Maintenance under amendment.** An amending act changes one subsection; the code should change in
   one place.
8. **Interviews.** Ask only the questions whose answers matter (Fact Graph, docassemble).
9. **Simulation.** Run a reform over a population (OpenFisca).

### 2.3 What works

- **Default logic matches how statutes are written.** A general rule, then exceptions, then
  exceptions to those. Lawsky (2018) shows tax law is prioritized default logic; Catala's default
  calculus makes it executable. This is the one semantic idea every serious system converges on. (H)
- **Isomorphism.** One provision, one piece of code (Bench-Capon and Coenen 1992). Catala's
  literate files are the modern form. Maintenance follows. (H)
- **Conflict as an error, not an order.** Catala raises a conflict when two exceptions of equal rank
  both apply; Cedar resolves by a fixed rule (forbid wins) and proves properties with SMT. Both beat
  DMN's `first`, where row order decides silently. (H)
- **Dated parameters.** OpenFisca's parameter files, valued by date, are what reform modellers use
  most. (M)
- **Missing-fact tracking.** Fact Graph and docassemble ask only what the answer needs. (M)

### 2.4 What fails

- **Code that diverges from law.** Robodebt (Australia, Royal Commission 2023) automated an income
  averaging rule the law did not allow. Horizon (UK Post Office) treated system output as truth. The
  lesson for syntax: the source of every rule must be visible and checkable, and discretion must stay
  with people. (H)
- **Controlled natural language as the only syntax.** Lexon and Logical English read well but are
  hard to write without the tool, and parse ambiguities appear at scale. (M)
- **Word operators.** `plus`, `minus`, `times`, `divided by`, as in the current mock, add tokens and
  ambiguity (`a divided by b times c`) and lawyers read `+` and `*` without trouble. Catala itself
  uses symbols for arithmetic. (M)
- **Order-dependent rules.** First-hit tables and Prolog clause order hide priority. (H)
- **Floats.** Many rules engines compute money in binary floats. Yan already rules this out (11.7).
- **LLM translation without checks.** NLLP 2025 (Catala benchmark) finds generation feasible but
  error-prone; the arXiv IRC s. 121 study found GPT-4o detected a planted inconsistency in one of
  three strategies, while the Prolog encoding made results deterministic and repeatable. ICML 2026
  generated Catala for part of the German KStG with scenario tests. Conclusion: agents will write
  this code, and the language must make their mistakes visible (gaps, conflicts, missing citations,
  missing rounding). (H for the papers' claims, L for generalizing)

### 2.5 What Yan already gives a law surface

`Money` as exact `dec128` quantities per currency (`12000eur`), `pct` and `bp` units, `quantize`
with an explicit `RoundingMode`, civil dates and `Period` with defined month-end clamping, `uses none`
purity, determinism, `pre`/`post`/`ex` contracts, item hashes (3.6) and round-trip laws. Most systems
in 2.1 build these from scratch. Yan Law's job is narrower: the default calculus, provenance, time,
judgment, and the syntax for them.

## 3. The five shared tasks

All examples use a fictional "Example Family Act 2026" and "Example Income Tax Act 2026".

- **T1 Income tax with rounding.** Allowance 12,000 euros plus 2,000 per dependant. Taxable income is
  income less the allowance, not below zero. Tax is 20% up to 50,000 and 40% above, rounded down to
  the euro. Worked example: income 70,000, two dependants, tax 11,600.
- **T2 Exceptions to exceptions.** (1) 250 euros a month per child. (2) If income exceeds 80,000,
  half of (1), rounded half-even to the euro. (3) Notwithstanding (1) and (2), a parent of a disabled
  child gets (1) plus 100 euros. (4) No children, no benefit.
- **T3 Time.** From 2027-01-01 an amending act raises 250 to 270. A child counts if under 18 on the
  first day of the month, or under 25 and in full-time education.
- **T4 Obligation and judgment.** A return is due on 31 May of the following year. Late filing costs
  10 euros per started week, at most 500, unless the taxpayer has a reasonable excuse, which a case
  officer decides.
- **T5 Explanation.** For T2 with two children, income 90,000, a disabled child: the result and why.

## 4. Five proposals

### 4.1 Proposal A, "Gloss": fidelity to the text

**Axis.** The file reads as the statute. Quoted statute text (`>` lines) is the skeleton; code sits
under the paragraph it implements. A paragraph's number is its rule's label. Priority uses the
statute's own word, `notwithstanding`.

```
law IncomeTax "Example Income Tax Act 2026"
  in income: Money
  in dependents: int
  out tax: Money

s. 2
  > (1) The allowance is 12,000 euros plus 2,000 euros for each dependant.
  allowance = 12000eur + 2000eur * dependents

  > (2) Taxable income is income less the allowance, but not below zero.
  taxable = max(income - allowance, 0eur)

  > (3) Tax is 20 per cent of taxable income up to 50,000 euros and 40 per cent
  > of the rest, rounded down to the whole euro.
  tax = (20pct * min(taxable, 50000eur) + 40pct * max(taxable - 50000eur, 0eur))
    rounded down to 1eur

  > Example 1. Income 70,000 euros, two dependants: tax 11,600 euros.
  example income = 70000eur, dependents = 2 gives tax = 11600eur
```

```
law ChildBenefit "Example Family Act 2026"
  in children: List[Child]
  in income: Money
  out benefit: Money

s. 4
  > (1) A parent receives 250 euros a month for each eligible child.
  base = 250eur * count(children, eligible)
  benefit = base

  > (2) If the parent's income exceeds 80,000 euros, the benefit is half
  > the amount in (1), rounded to the euro.
  benefit = base / 2 rounded to 1eur
    when income > 80000eur
    notwithstanding (1)

  > (3) Notwithstanding (1) and (2), a parent of a disabled child receives
  > the amount in (1) plus 100 euros.
  benefit = base + 100eur
    when any(children, c => c.disabled and eligible(c))
    notwithstanding (1), (2)

  > (4) No benefit is paid for a household without an eligible child.
  benefit = 0eur when base == 0eur notwithstanding (1), (2), (3)

s. 5
  > (1) A child is eligible if under 18 on the first day of the month, or under 25
  > and in full-time education.
  eligible(c: Child) = age(c) < 18 or (age(c) < 25 and c.in_education)
  age(c: Child) = years(c.born, month_start)

  > (1) [amended by Example Family (Amendment) Act 2027, s. 2, from 2027-01-01]
  > ... 270 euros ...
  from 2027-01-01 base = 270eur * count(children, eligible)
```

```
law LateFiling "Example Income Tax Act 2026"
  in year: int
  in filed: PlainDate
  judged reasonable_excuse: bool
  out penalty: Money

s. 9
  > (1) A return is due on 31 May of the year after the tax year.
  due = PlainDate(year + 1, 5, 31)

  > (2) A late return incurs 10 euros for each started week, at most 500 euros.
  penalty = 0eur
  penalty = min(10eur * started_weeks(due, filed), 500eur)
    when filed > due notwithstanding the above

  > (3) Subsection (2) does not apply where the taxpayer has a reasonable excuse.
  penalty = 0eur when reasonable_excuse notwithstanding (2)
```

T5: `yan law explain` prints the paragraphs that fired, quoting the `>` text: "benefit = 600 eur
by s. 4(3): a parent of a disabled child receives the amount in (1) plus 100 euros."

**Strengths.** Audit and review are reading. The citation is the label, so it can't be omitted.
Amendments land beside the text they amend. `notwithstanding` is the drafter's own word.
**Costs.** Long files; the statute text dominates token cost for agents; quoting copyrighted or
non-official text is a licensing question; numbering clashes when the code needs helpers the
statute doesn't name.

### 4.2 Proposal B, "Tabula": brevity

**Axis.** Fewest characters for an expert. Scopes are function headers; brackets and decision tables
are built-in forms; math is symbolic; time is a column.

```
scope income_tax(income: Money, dependents: int) -> tax: Money
  taxable = max(income - 12000eur - 2000eur * dependents, 0eur)
  tax = brackets(taxable) floor 1eur
    to 50000eur  20pct
    rest         40pct
  ex (70000eur, 2) -> 11600eur
```

```
scope child_benefit(kids: List[Child], income: Money) -> benefit: Money
  n = count(kids, eligible)
  base = n * rate
  benefit = cases
    n == 0                        0eur              -- s4(4)
    any(kids, .disabled)          base + 100eur     -- s4(3)
    income > 80000eur             base / 2 even 1eur -- s4(2)
    _                             base              -- s4(1)

  rate = dated
    2026-01-01  250eur
    2027-01-01  270eur

  eligible(c) = age(c) < 18 or age(c) < 25 and c.study
```

```
scope late_filing(year: int, filed: PlainDate, excuse: judged bool) -> penalty: Money
  due = date(year + 1, 5, 31)
  penalty = cases
    excuse or filed <= due   0eur
    _                        min(10eur * weeks_up(filed - due), 500eur)
```

T5: `--why` prints the `cases` row and its trailing comment.

**Strengths.** Tiny. `brackets` and `dated` capture the two most common statute shapes in two
lines each. **Costs.** `cases` is first-match, so row order is priority (the DMN failure in 2.4);
citations are comments, so nothing forces them; conflicts can't be detected because there are none
by construction, which also means drafting errors are silently resolved.

### 4.3 Proposal C, "Ledger": agent token cost and regularity

**Axis.** One statement form, flat, no nesting, canonical order, trivially constrained by a grammar
(GBNF or llguidance, as in the R567 harness). Every rule is one line:
`rule LABEL OUT = EXPR [when COND] [over LABELS] by CITE`.

```
law IncomeTax
in income: Money
in dependents: int
out tax: Money
let allowance = 12000eur + 2000eur * dependents by "s2(1)"
let taxable = max(income - allowance, 0eur) by "s2(2)"
rule t = floor(20pct * min(taxable, 50000eur) + 40pct * max(taxable - 50000eur, 0eur), 1eur) by "s2(3)"
ex income = 70000eur, dependents = 2 => tax = 11600eur
```

```
law ChildBenefit
in children: List[Child]
in income: Money
out benefit: Money
let n = count(children, eligible) by "s5(1)"
let rate = 250eur until 2027-01-01 else 270eur by "s4(1); Amendment Act 2027 s2"
let base = rate * n by "s4(1)"
rule b1 benefit = base by "s4(1)"
rule b2 benefit = half_even(base / 2, 1eur) when income > 80000eur over b1 by "s4(2)"
rule b3 benefit = base + 100eur when any(children, disabled) over b1, b2 by "s4(3)"
rule b4 benefit = 0eur when n == 0 over b1, b2, b3 by "s4(4)"
fn eligible(c: Child) -> bool = age(c) < 18 or (age(c) < 25 and c.in_education) by "s5(1)"
```

```
law LateFiling
in year: int
in filed: PlainDate
judged excuse: bool
out penalty: Money
let due = PlainDate(year + 1, 5, 31) by "s9(1)"
rule p0 penalty = 0eur by "s9(2)"
rule p1 penalty = min(10eur * started_weeks(due, filed), 500eur) when filed > due over p0 by "s9(2)"
rule p2 penalty = 0eur when excuse over p1 by "s9(3)"
```

T5: `explain` returns the label chain `b3 over b1, b2` with cites, as JSON for agents.

**Strengths.** An agent can't forget a citation or a priority; the grammar is about twenty
productions; every line diffs alone; repair messages point to one line. **Costs.** Monotonous to
read; `over b1, b2, b3` lists grow; labels are invented names, a second numbering beside the
statute's; no structure for sections.

### 4.4 Proposal D, "Writ": safety and explicitness

**Axis.** Nothing implicit. Every law states its sources, period in force, input domains, output
rounding, and which checks it must pass. Exceptions name what they defeat. Discretion is a typed
input with provenance. The checker proves totality and conflict-freedom over the stated domain.

```
law IncomeTax
  source s2 = "Example Income Tax Act 2026, s. 2"
  in force 2026-01-01 ..
  input income: Money[eur] where income >= 0eur
  input dependents: int where 0 <= dependents <= 30
  output tax: Money[eur] rounded Floor to 1eur
  uses none
  fail Conflict | Gap
  checks total, conflict_free, monotone(tax, income)

  define allowance by s2(1) = 12000eur + 2000eur * dependents
  define taxable by s2(2) = max(income - allowance, 0eur)
  rule tax by s2(3) = 20pct * min(taxable, 50000eur) + 40pct * max(taxable - 50000eur, 0eur)

  example by "Guidance note 1" income = 70000eur, dependents = 2 gives tax = 11600eur
```

```
law ChildBenefit
  source s4 = "Example Family Act 2026, s. 4"
  source s5 = "Example Family Act 2026, s. 5"
  source a2 = "Example Family (Amendment) Act 2027, s. 2"
  input children: List[Child] where all(children, c => c.born <= month_start)
  input income: Money[eur] where income >= 0eur
  output benefit: Money[eur] rounded HalfEven to 1eur
  checks total, conflict_free

  define rate by s4(1) =
    in force 2026-01-01 .. 2026-12-31: 250eur
  define rate by a2 amending s4(1) =
    in force 2027-01-01 ..: 270eur
  define base by s4(1) = rate * count(children, eligible)

  rule benefit by s4(1) = base
  rule benefit by s4(2) defeats s4(1) when income > 80000eur = base / 2
  rule benefit by s4(3) defeats s4(1), s4(2)
    when any(children, c => c.disabled and eligible(c)) = base + 100eur
  rule benefit by s4(4) defeats s4(1), s4(2), s4(3) when base == 0eur = 0eur

  define eligible(c: Child) by s5(1) =
    years(c.born, month_start) < 18
    or (years(c.born, month_start) < 25 and c.in_education)
```

```
law LateFiling
  source s9 = "Example Income Tax Act 2026, s. 9"
  input year: int
  input filed: PlainDate
  judgment reasonable_excuse: bool by s9(3) decided_by CaseOfficer
  output penalty: Money[eur] rounded HalfEven to 1eur
  checks total, conflict_free

  define due by s9(1) = PlainDate(year + 1, 5, 31)
  rule penalty by s9(2) = 0eur
  rule penalty by s9(2) defeats s9(2) when filed > due =
    min(10eur * started_weeks(due, filed), 500eur)
  rule penalty by s9(3) defeats s9(2) when reasonable_excuse = 0eur
```

T5: the output's provenance is part of its type, `Money by Rule`, so a caller can't drop the why.

**Strengths.** The checker catches gaps, conflicts and impossible inputs before anything runs (the
prototype in section 5 shows these are real). Rounding at outputs is a type fact, not a habit.
Judgment is visible and attributable. **Costs.** The headers are heavy for small laws; two rules
under one label (`s9(2) defeats s9(2)`) shows label-as-citation strains when one paragraph holds a
default and its exception.

### 4.5 Proposal E, "Forum": expressive power

**Axis.** Law is more than functions. Norms (obligations, permissions, prohibitions) have states;
rules can be queried backwards; hypotheticals and counterfactuals are first-class; arguments, not
only values, are results.

```
law IncomeTax
  facts income: Money, dependents: int
  derive allowance = 12000eur + 2000eur * dependents         § 2(1)
  derive taxable   = max(income - allowance, 0eur)           § 2(2)
  derive tax       = progressive(taxable,
                       [0eur: 20pct, 50000eur: 40pct]) floor 1eur  § 2(3)
```

```
law ChildBenefit
  facts household: Household
  derive benefit
    default base                                             § 4(1)
    unless income > 80000eur then base / 2 even 1eur         § 4(2)
    unless any disabled child then base + 100eur             § 4(3) beats § 4(2)
    unless no eligible child then 0eur                       § 4(4) beats all

  -- backward chaining: the questions an interview must ask, in order
  ask benefit
  -- counterfactual: smallest input change that changes the answer
  why not benefit >= 600eur
  -- hypothetical, for reform modelling
  suppose § 4(1) reads 300eur
```

```
law LateFiling
  party taxpayer, authority
  norm file: taxpayer must file return by PlainDate(year + 1, 5, 31)   § 9(1)
    on breach: authority may charge penalty                            § 9(2)
  derive penalty = min(10eur * started_weeks(file.due, file.done), 500eur)
    when file is breached
    unless judged reasonable_excuse                                    § 9(3)
```

T5: the result is an argument tree (Dung-style): `600 eur` supported by § 4(3), which attacks
§ 4(2), which attacks § 4(1). The tree prints as prose, JSON or a diagram.

**Strengths.** `ask`, `why not` and `suppose` cover the interview, the citizen's question and the
reform modeller in three words. Norms with breach handlers model contrary-to-duty obligations, which
plain functions cannot. **Costs.** Several semantics in one file (functions, norms, queries);
counterfactual search is not cheap or always decidable; harder to lower to Yan Core cleanly.

## 5. Experiment: the default calculus, run

[`ylaw/defaults.py`](ylaw/defaults.py) implements the calculus common to C, D and E: among the rules
that apply, drop each rule that another applying rule defeats; one survivor is the value, none is
`Gap`, two survivors with different values are `Conflict`. It sweeps a small grid of inputs
(children 0, 1, 3; income 40,000 to 120,000; disabled yes or no) for T2.

| Encoding | Gaps | Conflicts | What was wrong |
| --- | --- | --- | --- |
| Naive: (2) and (3) each defeat (1) only | 4 | 4 | No rule for zero children; (2) and (3) both apply to a high-income parent of a disabled child |
| Fixed: (3) defeats (1) and (2); (4) added | 0 | 4 | A disabled child with zero children: an impossible input, but the checker can't know that |
| Fixed plus input constraint (disabled implies a child) | 0 | 0 | Clean |
| T5 query: 2 children, 90,000, disabled | | | 600 eur by s4(3) |

Findings. The naive encoding is what a drafter, or an agent, writes first, and both bugs are
invisible to example tests that don't hit the corner. The second row matters most: without input
constraints, a conflict checker reports noise, and people learn to ignore it. So input domains
(`where`, or a `given` clause) are not optional decoration; they are what makes the checker usable.
A grid sweep is a toy; the real checker would hand conditions to an SMT solver, as Catala's
verification work and Cedar's analyzer do (M).

## 6. Comparison

Size proxy: characters and lexical tokens (identifiers, numbers and punctuation runs) of the T1 to
T4 samples above, statute quotations included for A. Not model tokens; relative only.

| Proposal | Chars | Tokens (proxy) | Citations forced | Conflicts detectable | Time | Judgment | Lowers to Yan Core |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A Gloss | 2519 (1358 without quotes) | 614 (311) | yes (structure) | yes | per paragraph | `judged` input | yes |
| B Tabula | 980 | 218 | no (comments) | no (first match) | `dated` | `judged` type | yes |
| C Ledger | 1254 | 362 | yes (`by`) | yes | inline `until` | `judged` input | yes |
| D Writ | 2155 | 547 | yes (`by`) | yes, plus totality | `in force` | `judgment`, attributed | yes |
| E Forum | 1291 | 240 | partly (trailing §) | yes | not shown | `judged` | partly: queries are tooling, norms need a state encoding |
| Mix (7.2) | 1864 | 450 | yes (line label) | yes, plus domains | `dated`, `in force` | `judged ... by` | yes |

Reading the table. B is smallest because it gives up the two things section 2 says matter most:
forced citations and detected conflicts. A costs the most only because it carries the statute text;
without the `>` lines it is second smallest, because citations as structure replace labels and
`by` clauses. E is small partly because it leaves T3 out. D's extra size is almost all headers, which a small law pays
once. The interesting comparison is C against D: D's per-rule cost is close to C's, and D's headers
buy the checks. The mix lands between them: A's citation-as-label removes C's and D's `by` clauses,
which pays for D's domains and rounding.

## 7. The mix: consolidated proposal

### 7.1 Principles

1. **The statute's structure is the program's structure** (A). Sections and subsections are the
   items; their citation is their label.
2. **Defaults, defeated by name** (C, D). A rule applies by default; a later rule says what it
   defeats, in the statute's own word. Equal-rank rules that both apply are a `Conflict`; none is a
   `Gap`. Order never decides.
3. **Every value has a why** (D, E). Provenance is carried, not reconstructed.
4. **Time is an axis, not a branch** (A, B, D). Any definition or rule may carry an `in force`
   period; the law is evaluated "as of" a date.
5. **Judgment is an input, never a guess** (D). Open-texture terms are declared, typed and
   attributed to who decides.
6. **Rounding is stated where the law states it** (D). Money outputs declare their rounding; an
   unrounded amount can't reach one.
7. **Shapes the law uses often get a form** (B). Brackets and dated tables, nothing else.
8. **Queries are tooling over pure code** (E). `ask`, `why`, `why not` and `suppose` are commands
   on the lowered functions, not semantics in the file.

### 7.2 The tasks in the consolidated syntax

```
--- Example Income Tax Act 2026, s. 2.
law IncomeTax uses none fail Gap | Conflict
  in income: Money where income >= 0eur
  in dependents: int where dependents >= 0
  out tax: Money rounded Floor to 1eur

  s2(1) allowance = 12000eur + 2000eur * dependents
  s2(2) taxable = max(income - allowance, 0eur)
  s2(3) tax = brackets taxable
    upto 50000eur  20pct
    rest           40pct

  ex income: 70000eur, dependents: 2 gives tax: 11600eur  -- Guidance note 1
```

```
--- Example Family Act 2026, ss. 4 and 5.
law ChildBenefit uses none fail Gap | Conflict
  in children: List[Child] where all(children, .born <= month)
  in income: Money where income >= 0eur
  in month: PlainDate where month.day() == 1
  out benefit: Money rounded HalfEven to 1eur

  s4(1) rate = dated
    2026-01-01  250eur
    2027-01-01  270eur  -- Example Family (Amendment) Act 2027, s. 2
  s4(1) base = rate * count(children, eligible)
  s4(1) benefit = base
  s4(2) benefit = base / 2  when income > 80000eur  notwithstanding s4(1)
  s4(3) benefit = base + 100eur  when any(children, c => c.disabled and eligible(c))
    notwithstanding s4(1), s4(2)
  s4(4) benefit = 0eur  when base == 0eur  notwithstanding s4

  s5(1) eligible(c: Child) -> bool =
    let age = c.born.until(month).years
    age < 18 or (age < 25 and c.in_education)

  ex children: [disabled_child, child], income: 90000eur, month: 2026-03-01
    gives benefit: 600eur by s4(3)
```

```
--- Example Income Tax Act 2026, s. 9.
law LateFiling uses none fail Gap | Conflict
  in year: int
  in filed: PlainDate
  judged reasonable_excuse: bool  by CaseOfficer
  out penalty: Money

  s9(1) due = PlainDate(year + 1, 5, 31)
  s9(2) penalty = 0eur
  s9(2) penalty = min(10eur * ceil(due.until(filed).days / 7), 500eur)  when filed > due
    notwithstanding s9(2)
  s9(3) penalty = 0eur  when reasonable_excuse  notwithstanding s9(2)
```

T5, from the command line:

```
yan law why ChildBenefit children=[disabled_child, child] income=90000eur month=2026-03-01
  benefit = 600 eur  by s4(3)
    defeats s4(2) (applied: income 90000 eur > 80000 eur)
    defeats s4(1)
    base = 500 eur  by s4(1): rate 250 eur (in force 2026-01-01) * 2 eligible
```

### 7.3 What each piece is

- **`law Name`** is the item. Its header ends in Yan's signature clauses (`uses none`, `fail`), as the
  shared mock rules require, so the five facts stay visible (N5). `uses none` is written, not
  implied: a law that reads a clock or a network is not a law.
- **`in`, `out`, `judged`** declare the boundary. `where` gives the input domain the checker
  assumes and the runtime enforces (`pre`). `rounded Mode to Step` on an `out` of type `Money` is
  where `quantize` happens; the checker rejects a law whose output path would need a second
  rounding the text doesn't state.
- **Citation-labelled lines.** A line starts with its citation (`s4(2)`), the label. Several lines
  may share a citation; `notwithstanding s4(2)` then means the other lines of s4(2). A bare prefix
  (`notwithstanding s4`) means every rule under s4 so far. The citation format is a small grammar
  (section, subsection, paragraph, `art.`, `§`), so `§ 4 Abs. 2` and `art. 4(2)` are also labels.
- **`notwithstanding`** is the one priority word. It matches drafting usage and reads in one pass. A
  shorter alias was considered and rejected: one word, one meaning.
- **`dated`** is a table of values by start date. **`brackets`** is the progressive schedule. Both
  lower to ordinary Yan. No other built-in shapes until a second statute needs one.
- **`ex ... gives ... by s4(3)`** is a test that also pins the winning rule, so a change that keeps
  the number but changes the reason fails.
- **Time.** The law takes the evaluation date from its inputs (`month` here). `dated` and an
  `in force a .. b` suffix on any line pick by that date. A retroactive amendment is a new `dated`
  row; the item hash changes and `yan audit` shows which past results it touches.
- **Expressions** are plain Yan: symbols, not words. Lawyers read `+` and `*`.

### 7.4 What the mix leaves out, and why

- Statute quotations in the file (A). They stay in `---` doc comments when wanted, or are linked by
  citation to an Akoma Ntoso source. Embedding them makes every agent edit pay for the text.
- `cases` first-match tables (B). Order as priority is the failure mode in 2.4.
- Invented labels (C). Citations are better labels and already exist.
- Norms with breach handlers (E). Valuable, but they are Yan Workflow's or Yan Protocol's job
  (durable state over time). Yan Law computes what is owed on a date; a norm machine calls it.
- `ask`, `why not`, `suppose` as syntax (E). Kept as `yan law` commands over the lowered code.

## 8. Lowering and fit with the core

- **Route.** Lowering, not an engine (N1, R568 point 3): every form above is expressible in Yan Core
  as `uses none` functions, so the surface keeps N4, N7 and the one back end. No engine crate.
- **Shape.** `law L` lowers to `L.In` and `L.Out` records and `L.eval(input: L.In) -> L.Out fail Gap
  | Conflict uses none`. Each output is computed by the default term of Merigoux et al.: a list of
  exceptions, a justification and a consequence, lowered to nested `match`. `judged` inputs become
  fields of type `Judged[T]` carrying the decider; `where` becomes `pre`; `ex` becomes `ex`.
- **Provenance.** `L.Out` fields carry the winning citation alongside the value (a `Why` record);
  `L.value(input)` is the projection without it, for callers that don't want it. The citation
  strings are stage-0 metadata, so N3 one printing holds.
- **Checks.** Gap and conflict checking run at compile time when the conditions are linear arithmetic
  and booleans (most statute conditions are), through the solver the verification work (section 8
  of the spec) already plans; otherwise the runtime `fail` remains.
- **Round trip (N7).** The subset predicate is "functions produced by this lowering", recognized by
  their metadata. `yan view --as ylaw` on hand-written Yan outside it reports the first construct
  that doesn't fit, as 16.4 does.
- **Laws touched.** Law 4 marks (N5): failure `fail Gap | Conflict` in the header, capability
  `uses none` in the header, mutation none, concurrency none, compile-time evaluation through Yan's
  stage-0 literals. No new capability is needed.

## 9. Open questions and items for an RFC

Open questions.

1. **Statute text in or out?** The mix keeps it out (doc comments or Akoma Ntoso links). Catala's
   users value it in. Worth a learnability probe with lawyers, not only agents.
2. **Keyword language.** Catala ships French, English and Polish keyword sets. Yan's lexis is one
   (N9). Recommendation: English keywords, any-language citations, doc comments and identifiers.
3. **Evaluation date.** Explicit input (as in T3) or a standard `as_of` parameter on every law?
4. **Judgment type.** Is `Judged[T]` a std type with a decider and a time, or just an annotated
   input? Should a `judged` input be required to be non-defaultable, so no code path guesses it?
5. **Population runs.** OpenFisca-style microsimulation wants `par for` over households and
   vectorized evaluation. The lowering permits it; is a `simulate` command enough?
6. **Norms.** Confirm that obligations belong to Yan Workflow or Yan Protocol and not here.

Items an RFC could take up (no rows yet):

- **D1.** Default calculus semantics (7.1 point 2) with `Gap` and `Conflict`, lowering to nested
  `match`.
- **D2.** Citation labels: grammar, prefixes, `notwithstanding`.
- **D3.** `in`, `out` with `rounded`, `judged`, `where` domains.
- **D4.** `dated` and `in force` time selection.
- **D5.** `brackets`.
- **D6.** `ex ... gives ... by` tests that pin the winning rule.
- **D7.** Provenance in outputs (`Why`) and `yan law why`.
- **D8.** Static gap and conflict checks over stated domains.
- **D9.** Replace the word operators in the RFC 0002 D14 example with Yan symbols; fix its money
  literals to `12000eur` (11.7).

## 10. Sources

Confidence: **H** read today or well established; **M** known, not rechecked today; **L** recalled,
verify first.

- Merigoux, Chataing, Protzenko, "Catala: A Programming Language for the Law", ICFP 2021 (PACMPL 5).
  H.
- Merigoux, Monat, Protzenko, "A Modern Compiler for the French Tax Code", CC 2021. H.
- Huttner, Merigoux, "Catala: Moving Towards the Future of Legal Expert Systems", AI and Law, 2022.
  M.
- Catala on opam (1.x; latest listed 1.3.0), https://ocaml.org/p/catala/1.0.0/README.html ; 1.1.0
  release notes, https://newreleases.io/project/github/CatalaLang/catala/release/1.1.0 . H.
- Cnaf press release, Cnaf and Inria partnership on Catala, 8 June 2026,
  https://caf.fr/professionnels/presse/publications/la-cnaf-et-inria-s-engagent-ensemble-pour-developper-catala-une-solution-souveraine-de-calcul-des
  . H.
- Lawsky, "A Logic for Statutes", Florida Tax Review 21, 2018. H.
- Reiter, "A Logic for Default Reasoning", Artificial Intelligence 13, 1980. H.
- Sergot et al., "The British Nationality Act as a Logic Program", CACM 29(5), 1986. H.
- Bench-Capon, Coenen, "Isomorphism and Legal Knowledge Based Systems", AI and Law 1, 1992. H.
- Dung, "On the Acceptability of Arguments...", Artificial Intelligence 77, 1995. H.
- Prakken, Sartor, "Argument-based extended logic programming with defeasible priorities", 1997. M.
- Governatori, "Representing business contracts in RuleML" and Formal Contract Logic, 2005. M.
- Hart, The Concept of Law, 1961 (open texture). H.
- OECD OPSI, "Cracking the Code: Rulemaking for humans and machines", 2020. H.
- Royal Commission into the Robodebt Scheme, report, July 2023. H.
- Holzenberger, Blair-Stanek, Van Durme, SARA statutory reasoning dataset, NLLP 2020. H.
- Lorenzo, Pietromatera, Holzenberger, "Translating Tax Law to Code with LLMs: A Benchmark and
  Evaluation Framework", NLLP 2025, https://aclanthology.org/2025.nllp-1.4/ . H.
- "Law to Code: An End-to-End Pipeline for Catala Code Generation and Evaluation from German Statutory
  Text", ICML 2026, https://icml.cc/virtual/2026/80583 . H (abstract only).
- Yadamsuren, Platt, Diaz, "LLM-Assisted Formalization Enables Deterministic Detection of Statutory
  Inconsistency in the Internal Revenue Code", arXiv 2511.11954. H (abstract only).
- IRS Direct File and Fact Graph, https://github.com/IRS-Public/direct-file , open sourced May 2025.
  M.
- Kowalski, Datoo, "Logical English meets legal English for swaps and derivatives", AI and Law, 2022.
  M.
- Sartor et al., "Explainable Rules as Code with Logical English and s(CASP)", CEUR Vol-3193, 2022,
  https://ceur-ws.org/Vol-3193/paper5GDE.pdf . M.
- Cutler et al., "Cedar: A New Language for Expressive, Fast, Safe, and Analyzable Authorization",
  OOPSLA 2024. H.
- OMG DMN 1.x and FEEL, hit policies. H.
- OpenFisca, https://openfisca.org . M.
- L4, Legalese and CCLaw, Singapore Management University. M.
- Hvitved, "Contract Formalisation and Modular Implementation of Domain-Specific Languages" (CSL),
  PhD thesis, 2012. M. Sharifi et al., Symboleo, 2020. M. Lamela Seijas et al., Marlowe, 2018 and
  2020. M. Diedrich, Lexon, 2019. L.
- OASIS LegalDocML (Akoma Ntoso) 1.0, 2018; OASIS LegalRuleML Core 1.0, 2021. M.
