# Spec questions

## FFI: `uses lib` (T102)

Calling an extern item does not yet require `uses lib` (12.2): the conformance cases call extern items from functions
without a `uses` clause. Settled by T102, not by RFC 0047.
