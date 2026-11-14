# ruff: noqa: E501, E701, E702, E741, B007, SIM115
"""Generates docs/research/2026-11-14-uncased-row-audit.{md,csv}: uv run python -m tools.uncased_audit"""

import csv
import subprocess

from tools import conform, rows, status


def rng(*parts):
    out = set()
    for p in parts:
        if isinstance(p, tuple):
            out |= {f"R{i}" for i in range(p[0], p[1] + 1)}
        else:
            out.add(f"R{p}")
    return out


owner, _ = rows.allocations(rows.load())
alloc = {r for r in owner if r.startswith("R")}
cased = {r for c in conform.discover(status.CONFORMANCE) for l in c.rows for r in rows.expand(l)}
unc = alloc - cased
G = [  # bucket, group, rows, reason, treatment
    (
        "exempt",
        "Licensing, governance, trademark (26.5)",
        rng((350, 355), (357, 366)),
        "Legal and process rules about the project, not program behaviour. R356 (license_policy enforced by yan check) stays, it is observable.",
        "Exempt: checked by repo files (LICENSE, CODE_OF_CONDUCT, DCO bot), not by conformance.",
    ),
    (
        "exempt",
        "Names, marks, release scope, design bases",
        rng(774, 775, 586, 990, 991, 1001, 566, 567),
        "Naming, trademark, release-scope and probe-timing decisions; no program output depends on them.",
        "Exempt: tools/names_lint.py and the roadmap carry them.",
    ),
    (
        "exempt",
        "Toolchain-internal structure",
        rng(558, 560, 561, 562, 914, 915, 921, 922, 923, 961, 965, 987, 479, 1000),
        "Crate layering, registry generation, suite layout, unsafe confinement, yan-flow placement, egglog backend choice. Checked by repo tooling (cargo, lint scripts, tools/conform.py), never by a Yan program.",
        "Exempt: a CI lint or cargo test is the evidence.",
    ),
    (
        "exempt",
        "Implementation language and self-hosting",
        rng(966, 967, 968, 969, 970),
        "Which language the compiler is written in, S0 to S5 stages and the LLVM choice. Tracked by #851, gated by byte-equality, not by a conformance case.",
        "Exempt: self-host stage gates (#852) are the evidence.",
    ),
    (
        "exempt",
        "Panic payload internals",
        rng(796, 883, 884, 885),
        "Payload fields (RFC 0031, 0042) live in the record-and-replay buffer; a program sees only the terminal message, which omits them (spec 8.3).",
        "Exempt unless the lead wants a replay-trace case (tool-observable, not program-observable).",
    ),
    (
        "merge",
        "Origin and spelling marks (RFC 0034)",
        rng(565, (837, 843), (850, 870)),
        "HIR trivia marks. Not observable alone; their only contract is the raise round trip, already stated by N7 and law 6 (R996).",
        "Merge into R996 as clauses; one round-trip case family cites R996.",
    ),
    (
        "merge",
        "Superseded implementation language",
        rng(215),
        "Replaced by R966 (spec says so).",
        "Retire R215, point at R966.",
    ),
    (
        "defer",
        "Research track and niche surfaces, post-1.0",
        rng((522, 535), (536, 550), (554, 557), 568, 576, (776, 786), 680, 455),
        "Spec marks them not normative for 1.0 or post-1.0.",
        "Defer: count after 1.0, or exclude from the 1.0 gate by lead card.",
    ),
    ("defer", "Yan Meta, M4", rng((516, 521)), "Third surface built at M4 (R516).", "Defer to M4."),
]
cb = {}
rowsout = []
for b, g, s, reason, tr in G:
    s &= unc
    for r in sorted(s, key=lambda x: int(x[1:])):
        rowsout.append((r, b, g, reason))
    cb.setdefault(b, set()).update(s)
N = len(unc)
D = len(alloc)
T = 0.6


def line(x, both):
    n = N - x
    d = D - x if both else D
    return n, d, n / d


commit = subprocess.check_output(["git", "rev-parse", "--short", "HEAD"], text=True).strip()
print(commit, N, D, {k: len(v) for k, v in cb.items()})
w = csv.writer(open("docs/research/2026-11-14-uncased-row-audit.csv", "w", newline=""))
w.writerow(["row", "bucket", "group", "reason"])
w.writerows(rowsout)
ex = len(cb["exempt"])
me = len(cb["merge"])
de = len(cb["defer"])
need = D * T


def pr(label, x, both):
    n, d, p = line(x, both)
    return f"| {label} | {n} of {d} | {p * 100:.1f}% | {'yes' if n <= int(d * T) else 'no'} |"


tab = [
    "| Treatment | Uncased | Percent | At or below 60% |",
    "|---|---|---|---|",
    f"| Today | {N} of {D} | {N / D * 100:.1f}% | no |",
    pr("Exempt, rows leave numerator only", ex, False),
    pr("Exempt, rows leave both", ex, True),
    pr("Exempt + merge, numerator only", ex + me, False),
    pr("Exempt + merge, both", ex + me, True),
    pr("Exempt + merge + defer, numerator only", ex + me + de, False),
    pr("Exempt + merge + defer, both", ex + me + de, True),
]
groups = []
for b, g, s, reason, tr in G:
    s &= unc
    ids = sorted(s, key=lambda x: int(x[1:]))
    groups.append(f"| {g} | {len(ids)} | {reason} | {tr} |")
md = (
    f"""# Uncased spec row audit, 2026-11-14

Issue #1025 (spec freeze gate), tracker #1136 (P3). Evidence at `{commit}`, 2026-11-14, from `tools.status` spec_debt logic: {N} of {D} R rows have no conformance case. The gate needs 527 or fewer (60 percent of {D}). Antonio chose "audit rows first, then sweep". This study changes no spec row and no threshold; it proposes treatments for a lead card.

Regenerate the figures and the per-row CSV (`2026-11-14-uncased-row-audit.csv`) with `uv run python -m tools.uncased_audit`; do not edit them by hand.

## Method

Each uncased row was judged by one question: can a Yan program, run through `yan`, ever produce an output that a conformance case could assert? Rows were grouped by the topic text in `spec/rows.toml` and, for the groups below, by the row's body text in `spec/`. Only groups where the answer is clearly no (exempt), clearly a duplicate or part of another row (merge), or not normative for 1.0 (defer) are listed. Everything else stays uncased and needs a real case (about {N - ex - me - de} rows); that includes bundle rows R898 to R907, which are observable but each need several cases, and mixed entries such as R467 to R473 and R474 to R476.

## Candidates

| Group | Rows | Reason | Proposed treatment |
|---|---|---|---|
"""
    + "\n".join(groups)
    + f"""

Totals: exempt {ex}, merge {me}, defer {de}.

## Projected uncased count

Two readings of "exempt" give different ratios. Removing a row from the denominator as well as the numerator moves the ratio less than counting it as cased, so both are shown.

"""
    + "\n".join(tab)
    + f"""

Reading: exemption and merge alone ({ex + me} rows) do not reach the gate under either reading. Reaching 527 needs {N - 527} fewer uncased rows if exempt rows count as cased. The remainder must come from cases (the sweep) or from deferring the post-1.0 rows by lead card.

## Caveats

- Panic payload rows (R796, R883 to R885) are tool-observable through `yan replay`; exempting them is a judgement call.
- R5 and R8 to R22 (old process and tooling rows) were not judged; their bodies are not in the spec text and need a read before any decision.
- Exempting rows changes what "spec debt" means; practice 16 forbids growth across reports, so the progress report must state the rule used.
- Merging rows renumbers nothing; it removes ids, which needs a spec PR the lead merges, after the freeze rules allow it (contradiction fix or lead card).
"""
)
open("docs/research/2026-11-14-uncased-row-audit.md", "w").write(md)
