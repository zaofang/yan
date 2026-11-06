# 0042: Panic operand fields that the spec names but does not define

- Status: accepted
- Research: none; the gap is spec section 8.3 (operand fields) against the interpreter after the numeric operand work
- Rows: R874 to R877

## Summary

Section 8.3 says a panic of kind overflow, div_zero, bounds, shape, contract or assert carries named operand
fields, including "key hash with key type" and "the contract clause id" plus the numeric free variables of
the predicate. Index, length, divisor, dividend, overflow type and operands, and assert sides are built. Three
things are not defined: the clause id, the key hash, and any panic of kind `shape`. This RFC fixes each, so
the interpreter, the Cranelift backend and the record-and-replay buffer agree on one payload.

## Decisions

### D1: contract clause id

Options: (a) the zero-based position of the clause among the item's `requires` and `ensures` clauses in
source order, together with the item hash, so the payload names the clause without text; (b) the hash of the
clause's canonical text; (c) the source span.

Recommendation: (a). It is stable across a rename of locals (positional locals already make the item hash
stable), cheap on the panic path, and needs no text in the payload. A span would leak source layout into a
buffer that is replayed on other machines; a clause hash is larger and adds nothing the item hash plus
position does not.

- Decision: Item hash plus clause position (recommended option); Antonio, 2026-11-06
- Row: R874

### D2: free variables of a contract or assert predicate

Options: (a) the numeric (integer, float, bool, unit-carrying) parameters and locals the predicate reads,
named by their written name, in first-use order; (b) the same, by positional name (`%0`), which keeps the
payload hash-neutral; (c) only the operands the failing comparison reads.

Recommendation: (a) for the payload, since a reader of a panic needs the names that the program text shows;
the name table already exists per item (positional locals), so the lookup costs nothing off the panic path.
A variable of a non-numeric type is omitted, as for every other field. The interpreter collects the names at
compile time and reads them only when the check fails.

- Decision: Numeric variables, written names (recommended option); Antonio, 2026-11-06
- Row: R875

### D3: key hash with key type for a missing dict key

Options: (a) the first 64 bits of BLAKE3 over the key's canonical encoding (the encoding that item hashing and
`Wire` already use), as an unsigned integer, plus the key type's name; (b) the key itself when the key type is
numeric, else only the type name, which is what the numeric-only rule already gives for text keys; (c) a
per-run seeded hash.

Recommendation: (a). A hash lets two panics be compared and bucketed without exposing a text or `Secret` key,
and the canonical encoding makes it identical on every target and run (deterministic by default). (c) breaks
replay. For a numeric key the hash is still given, and the key itself is already available as the `index`-like
operand where the dict is indexed by an integer.

- Decision: 64-bit BLAKE3 hash plus key type (recommended option); Antonio, 2026-11-06
- Row: R876

### D4: the `shape` panic kind

Options: (a) remove `shape` from the list of kinds that carry operand fields, since no runtime check raises
it today (shape mismatches are compile errors); (b) reserve it for a runtime tensor or array shape check and
define its fields now as the expected and actual dimension lists (integers); (c) leave it listed and
undefined.

Recommendation: (a), and add it back with its fields in the RFC that introduces a runtime shape check.
Listing a kind with no source and no fields invites a divergent payload.

- Decision: Remove `shape` until a runtime check exists (recommended option); Antonio, 2026-11-06
- Row: R877

## Spec changes

Section 8.3 (operand fields): name the clause id, the free-variable rule, the key hash, and the kinds that
carry fields. Appendix A and the section 14 reference to the payload need no text change beyond the row
topic.
