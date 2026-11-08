# Spec questions

## FFI: `uses lib` (T102)

Calling an extern item does not yet require `uses lib` (12.2): the conformance cases call extern items from functions
without a `uses` clause. Settled by T102, not by RFC 0047.

## Accuracy lint readings (R613)

11.1 leaves the lint threshold, the sample count and the reference precision open. The implementation
(`crates/yan-elab/src/accuracy.rs`) flags a mean error above 4 bits over 256 samples, offers a rewrite only when
it is at least 1 bit better, and takes the reference from exact addition and multiplication with division and
square root at the first precision from 256 bits that halving does not change. Candidates are expressions of
`+ - * /`, unary minus and `math.sqrt` over `float` parameters and literals in functions without `uses`; a
function that reassigns a parameter is skipped. The fix is `choice` (R292), since it changes results.
