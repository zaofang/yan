# 0088: Tree Borrows check under `--ub` (item 5)

<<<<<<< HEAD
- Status: accepted
=======
- Status: draft
>>>>>>> origin/main
- Research: code reading for #1661 (no study; the facts are cited below)
- Rows: numbered with the cases of the first slice (CASE WITH ROW), as the final commit after merging main

## Summary

Spec 6.7 item 5 is "a Tree Borrows aliasing violation" and 6.6 says where retags happen: function
entry for `inout` and by-address `let`, and creation of a `View` or `MutView`. The checked machine
(`yan-machine`) has no aliasing state, and today it could not use any: `yan-mir` passes `inout` as
extra results and `let` by value (`lib.rs` header), `View` and `MutView` have no lowering, and a
`Ptr` is two words (address, allocation it derives from; `lower/ptr.rs`) whose only sources are
`Ptr.of` and `as_ptr`, neither a retag. So no MIR program can reach item 5, and no case can fail
under it. This RFC adds the checker so that it is complete when the retag sites appear, and
names the one site that exists now. Stage claimed: working, then right.

## Decisions

### D1: a codeless `Inst::Retag`, the tag in the pointer's second word

`yan-mir` gets `Inst::Retag { ptr: Operand, prov: Local, size: u32, kind: RetagKind }` with
`RetagKind = Unique | Frozen | Cell` and a `protected` bit, emitted at the 6.6 sites only, no
code in any backend (like `Inst::Move` and `Inst::Arm`). The `Ptr` provenance word becomes
allocation id and tag packed (`alloc << 32 | tag`; 0 stays none), so a derived `Ptr` inherits its
place's tag with no work, as 6.6 requires, and `Ptr` equality still compares addresses.

Alternatives weighed:

- *Shadow table keyed by address.* Two pointers to one address with different tags are
  indistinguishable. Rejected: wrong for exactly the case being checked.
- *Tag in the high bits of the address word.* Every address use must strip it, in all backends.
  Rejected: native code would see the tag.
- *Provenance word (this proposal).* The slot exists, native code already ignores it, and
  derivation is a copy. Recommended.

- Decision: (lead)
- Row: numbered in the first slice

### D2: a tree per allocation, tag GC at function exit

`Memory` keeps, per allocation, a tree of tags with per-byte-range permission state (Reserved,
Active, Frozen, Cell, Disabled) and the foreign and child access rules of Tree Borrows. A read
or write through `Memory::read` and `write` with a pointer's tag is an access; a violation is
`Stop::Ub(5, ...)` naming the tag's retag site. Protectors end at function exit. Tag GC runs
there too: a tag with no live pointer word (frame locals, memory words holding a `Ptr`) and no
live child is removed and its parent absorbs it, which bounds the tree by live borrows.

Alternative: never collect. Rejected: a loop that calls a function with an `inout` argument one
million times builds a million-node tree.

- Decision: (lead)
- Row: numbered in the first slice

### D3: the first slice emits the one retag site that exists, and no more

Slice 1 is the machine (tree, access check, GC) and a `Retag` emitted for the root of `Ptr.of`
and for `as_ptr`, which gives each a distinct root tag; no violation is reachable from those
alone, so the slice carries unit tests in `yan-machine` and no conformance case. Each later
site (by-address `let` and `inout` when native passes them by address, `View`, `MutView`) adds
its retag and its `conformance/ub/` case in its own PR, CASE WITH ROW. The alternative of
implementing views first is not this RFC's to decide.

Alternative: add an `unsafe` retag intrinsic so a case can fail today. Rejected: it adds
surface to the spec for a checker-only need (6.6 says there is no retag on `Ptr` operations).

- Decision: (lead)
- Row: numbered in the first slice

## Spec changes

6.6: the tag representation is not spec; 6.9 lists item 5 as checked once the first view site
lands, and says which sites are covered, so the report does not claim more than the machine
checks. No change to 6.7.
