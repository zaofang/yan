# 0097: Canonical MIR text

- Status: accepted
- Research: #1513 (S3a), RFC 0082 D7 (R1048), `docs/research/2026-11-19-self-hosting-sizing.md`
- Rows: pending (numbered with S1)

## Summary

RFC 0082 D7 requires `yan-mir` to have a canonical text form with a printer, a parser and a validator, reached by `yan view --as mir` and `yan build --emit mir`, so that the Yan LLVM code generator (#1514) can read MIR as a file and every semantic decision is visible. None exists. This RFC decides the shape of the text, what it must carry that in-memory MIR does not yet carry, and the order of work. It states the stage it claims: working only, by design (a text form with a fixed-point test); no performance claim. It adds no syntax to any surface. The grammar items below are open for the lead; each gives a recommendation.

Today's MIR (`crates/yan-mir/src/lib.rs`) is a per-function control-flow graph over typed locals (`Int(IntTy)`, `F64`, `Bool`, `Unit`, `Ptr`), with `Inst` (copy, scalar intrinsic, call, indirect call, runtime call, load, store, address-of, atomic, fence, arm events, move), `Term` (jump, branch, switch, return, tail call, panic, plus the pre-task-transform `Par`, `Suspend`, `TaskEnd`, `Unwind`), a string table and `main`. Several of D7's required facts (layouts, niches, per-parameter calling convention, drop glue as visible items) are not in this structure; they are in elaboration, `yan-mir::layout` or the backend. The text cannot show what MIR does not hold, so D6 decides how the gaps are closed.

## Decisions

### D1: one canonical text, a structured line form

Options:

- A. A line-oriented, block-structured text in the spirit of Yan Core: one instruction per line, labeled blocks, locals as `%n`, functions as top-level items. Every construct has exactly one printing (canonical). The grammar is small enough for the Yan code generator to parse in about 300 lines.
- B. S-expressions. Trivial parser, hard to read in diffs and reviews.
- C. Reuse Yan Core's surface grammar for MIR. Rejected: MIR has no expressions, nesting or names; sharing a grammar would force one to carry the other's constructs (one fact, one representation fails the other way).
- D. A binary form with a printer on top. Rejected for this RFC: D7 asks for a text form; a binary cache format is a later, separate decision.

Recommended: A. It is diffable, it keeps the parser a hand-written recursive descent that the Yan port can mirror, and `print . parse . print` fixed-point testing is straightforward.

- Decision: accepted as recommended: A (lead, 2026-11-19)
- Row:

### D2: is the text the interface or a dump

Options:

- A. The text is a normative interface: the spec fixes its grammar (a new section 22.4 subsection), versioned by a header line `mir 1`, and the Yan code generator depends on it. Changes go through RFC.
- B. A debugging dump only, unstable. Rejected: D7 and D8 make the text the boundary between the Yan pipeline and LLVM; an unstable dump cannot be one.

Recommended: A, with the `mir <version>` header so a stale reader fails loudly (practice 13) instead of misparsing.

- Decision: accepted as recommended: A (lead, 2026-11-19)
- Row:

### D3: naming and numbering

Options:

- A. Numeric ids as in memory: `%0` locals, `bb0` blocks, `fn0` functions, with a comment-free trailer of names in a symbol table so function names print once, as `fn0 = "main"`.
- B. Names everywhere: functions by mangled name, blocks by user-visible labels. More readable, but names are not unique across instances and drift under renumbering passes.
- C. Numeric ids with the name inline on each `fn` item (`fn 0 "main"(...)`).

Recommended: C. Ids stay the identity (the in-memory ids, so Cranelift sees identical programs from memory and from text, #1513 acceptance 2), and the name on the item is for readers and for diagnostics; the parser checks ids are dense and in order.

- Decision: accepted as recommended: C (lead, 2026-11-19)
- Row:

### D4: types and layout in the text

Options:

- A. Print the existing `Ty` (`i32`, `u8`, `f64`, `bool`, `unit`, `ptr`) and add a layout table per record type (size, alignment, field offsets, niche description) as top-level `layout` items that instructions reference by id.
- B. Print only `Ty` and leave layout to each backend. Rejected: D7 says layout and niche choice are visible and made above the backend; a second backend would copy the decision.
- C. Print layouts inline on every instruction that uses them. Rejected: duplicates a fact on every use.

Recommended: A. This needs MIR to hold layouts as first-class data; see D6.

- Decision: accepted as recommended: A (lead, 2026-11-19)
- Row:

### D5: instruction vocabulary and intrinsic spelling

Options:

- A. One mnemonic per `Inst` and `Term` variant, intrinsic ids dotted as in the Yan Core study (`i32.add`, `i32.add_wrap`, `u8.checked_as`), runtime calls as `rt.<name>` taken from `RtFn`, operand types explicit where the in-memory form carries them. The mnemonic table lives in one file and is generated into both the printer and parser.
- B. Print `Debug`-style variant names. Rejected: not stable, not canonical.

Recommended: A. The printer and parser share the table so they cannot disagree (practice 12). `Par`, `Suspend`, `TaskEnd` and `Unwind` exist only before the task transform: the printer accepts them, the validator rejects them in the post-transform form that backends read.

- Decision: accepted as recommended: A (lead, 2026-11-19)
- Row:

### D6: closing D7's visibility gaps

D7 lists: layout, niche choice, calling convention per parameter, overflow and bounds checks, panic kinds, drop placement. In-memory MIR already shows checks (checked scalar ops), panic kinds (`Term::Panic`) and drops (as `Inst`/`Rt` and `Move`). Not shown today: layouts, niches, per-parameter calling convention (how an `inout` or `sink` parameter passes), drop glue as named functions.

Options:

- A. Land the printer on today's MIR, then add the missing facts to MIR itself in follow-up slices, each with its text form, until the gap list is empty; the printer's PR files each unshown decision in a gap issue (as #1513 acceptance 4 requires).
- B. Block the printer until every fact is in MIR. Rejected: delays the first useful output and hides the real gap list.
- C. Print the missing facts as comments from backend queries. Rejected: a fact printed but not stored is a second representation.

Recommended: A. The gap issue is the acceptance record; no fact moves into the text without existing in MIR first.

- Decision: accepted as recommended: A (lead, 2026-11-19)
- Row:

### D7: validator and its codes

Options:

- A. A validator over parsed or in-memory MIR checking: dense ids, every local typed and defined before use on all paths, block terminators present, call arity and types match the callee signature, `Switch` targets in range, no pre-transform terminators in the backend form, pinned locals only address-taken. Each rule has an I-code naming it; codes are added with their crate symbol, catalogue entry and one case (practice 3).
- B. Validate only the parser's syntax. Rejected: #1513 acceptance 3 names ill-formed MIR.

Recommended: A, in a new `yan-mir` module (practice 5), run by `yan view --as mir --check` and in CI over the corpus.

- Decision: accepted as recommended: A (lead, 2026-11-19)
- Row:

### D8: acceptance and order of slices

The corpus check is print, parse, print is a fixed point for every runnable conformance and example program, and the Cranelift backend produces identical objects from in-memory and parsed MIR. Options for order:

- A. S1 printer plus `yan view --as mir` and `yan build --emit mir`; S2 parser and the fixed-point check; S3 validator and I-codes; S4 Cranelift-from-text equivalence; S5 layout, niche and calling-convention facts (D6) one per slice. Each slice runs on the interpreter and native outputs where it executes programs.
- B. Parser first. Rejected: nothing to parse before a printer fixes the canonical form.

Recommended: A.

- Decision: accepted as recommended: A (lead, 2026-11-19)
- Row:

## Spec changes

On acceptance, in the writing PR with rows numbered last: 18.1 (`yan view --as mir`, `yan build --emit mir`), 22.4 (MIR text grammar, version header, validator codes), and conformance cases for the fixed point and each I-code. `rfcs/README.md` index row. No change to any surface.
