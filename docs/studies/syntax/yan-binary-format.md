# Yan Binary Format 言码 (`.ybf`): a syntax study

Status: brainstorm study, 2026-11-04. Not a spec edit. It proposes no rows and changes nothing in
`spec/`. Names are frozen and used as decided.

The accepted D8 entry of RFC 0002 gives the job (packet, file and register layouts with checked
zero-copy parsers and writers), one IPv4 example and a lowering to view-backed records over
`View[u8]`. `docs/surfaces.md` section 12 adds a mock grammar with tagged unions. Neither settles the
syntax. This study does.

## Contents

1. Methodology and process
2. Findings: peers, what users want, what works and what fails, research
3. The framework: one description, six artifacts, four laws
4. Five proposals, each optimizing one axis, on the same five tasks
5. Experiments and measurements
6. The mix: the consolidated proposal
7. Open questions

## 1. Methodology and process

**What I read in the repo (narrowly).** RFC 0002 D8 (job, lowering, first slice, three diagnostics);
`docs/surfaces.md` sections 3 (Yan `.yn` style), 6 (Yan Schema, the nearest neighbour) and 12 (the
`.ybf` mock grammar); spec 4.5 (views and `from`), 5.2 (`@layout`, including `bits(uN)` for LSB-first
registers), and the `bytes` paragraph of 17 (no byte literal exists today).

**What I searched.** The current state of Kaitai Struct (0.11 release notes, serialization docs),
EverParse 3D (language reference), Vest (USENIX Security 2025), DaeDaLus (PLDI 2024) and the Rust
derive crates binrw and deku. The rest of section 2 is from prior knowledge; each claim carries a
confidence mark: **[H]** high (checked this session or long-established), **[M]** medium (recalled, not
re-checked), **[L]** low (impression).

**What I tried.**

- Wrote the same five tasks in five proposals, each pushed hard toward one axis, so the tradeoffs show
  in real code, not in adjectives.
- Built a 40-line prototype (`diagram.py`, kept in the thread scratchpad) that renders and re-reads
  RFC-style bit diagrams, to test whether proposal P2 can be source code.
- Measured every sample for size (bytes, lines, and a lexical token proxy) with a script over this
  file, to put numbers on the brevity and agent-cost axes.
- Scored each proposal on eight criteria, then took the best part of each into the mix.

**The five tasks.** Chosen to cover the first slice and everything the RFC lists as "later":

| Task | Format | Exercises |
| --- | --- | --- |
| T1 | IPv4 header (RFC 791) | sub-byte fields, big-endian, refinements, checksum over itself, dependent length |
| T2 | PNG file | magic, repetition to a terminator chunk, tagged union on a 4-byte kind, CRC, length-delimited window |
| T3 | TLV stream with LEB128 | variable-length integers, repetition to end of input |
| T4 | A directory archive (zip/ELF style) | random access by offset, offsets the writer must compute, overlap |
| T5 | A 32-bit UART control register | LSB-first bits, reserved bits, access modes (`w1c`) |

T4 is a small invented format so every proposal fits on a screen: a header `magic "YARC"`,
`version u16 = 1`, `count u16`, `dir_off u32`, all little-endian, then at `dir_off` a directory of
`count` entries, each `name_len u16`, a UTF-8 name, `data_off u32`, `size u32`, with the data at
`data_off`.

T5: bit 0 `en`, bits 1 to 2 `parity` (an enum), bit 3 `stop`, bit 4 `tx_done` (write 1 to clear),
bits 5 to 7 reserved, bits 8 to 23 `div` (non-zero), bits 24 to 31 reserved.

## 2. Findings

### 2.1 The field

Binary description languages split into four families. Where a tool sits decides what it can promise.

| Family | Examples | Model | Strength | Weakness |
| --- | --- | --- | --- | --- |
| Declarative descriptions | Kaitai Struct, DFDL, PADS, Nail, EverParse 3D, Vest, DaeDaLus | a schema; tools generate parsers | one source, many targets | writing is weak or absent; expression languages are bolted on |
| Language-embedded derives | binrw, deku, Construct, Zig packed structs, Rust zerocopy | annotate host types | no new language; host types | read and write directives drift apart; attribute soup |
| Safe parsing languages | Wuffs | a whole language whose type checker proves bounds | fastest safe decoders | you write the decoder, not the format |
| Inspection templates | 010 Editor templates, ImHex patterns, Wireshark dissectors | imperative scripts over a buffer | great for reverse engineering | not a contract; no writer; not safe |

Notes on each, with confidence:

- **Kaitai Struct** [H]. YAML schemas (`.ksy`), the widest adoption in the niche, a format gallery of
  hundreds of formats, an IDE that shows the parse tree over the hex view. Version 0.11 (September
  2025) added Rust as a target; serialization exists only for Java and Python, needs `_check()` after
  every edit or `_write()` throws, and value instances need manual invalidation. The user keeps length
  fields consistent by hand. Lesson: bolting writing onto a read-only design is hard, and users feel the
  seams.
- **EverParse and 3D** [H]. C-like `typedef struct` with refinements in braces, `casetype` for tagged
  unions, value parameters, actions on fields, `entrypoint` for exported validators. Generates verified
  C (via F\*) that is memory safe, arithmetic safe and double-fetch free, and validates in place
  without allocation. Shipped in the Hyper-V virtual switch and other Microsoft components [M on the
  exact list]. Lesson: validate-in-place with zero allocation is the right default, and proof-grade
  guarantees fit in a familiar syntax.
- **Vest** [H for the abstract, M for details]. USENIX Security 2025. An "RFC-like DSL" that generates
  Rust parsers and serializers verified in Verus, with round-trip properties, performance on par with
  hand-written parsers, and resistance to basic digital side channels. Case studies: Bitcoin blocks,
  TLS 1.3 handshake messages, WebAssembly binaries. Lesson: parse and serialize from one description,
  with the round-trip as a proved law, is now practical, not a research dream.
- **Narcissus** [M]. ICFP 2019, Coq. Derives correct-by-construction encoders and decoders from one
  format description; formalizes the round-trip. Same lesson, earlier.
- **Nail** [M]. OSDI 2014. A grammar that yields both parser and generator, with "dependent fields"
  (lengths, counts) that the generator computes and the user never sets, and "transformations" for
  offsets and checksums. Lesson: derived fields are the single biggest ergonomic win for writing, and
  Kaitai still lacks them.
- **DaeDaLus** [H for existence, M for details]. Galois, PLDI 2024 ("Safer Document Parsing"). A
  data-dependent parsing language for PDF and NITF scale formats with semantic values, choice and
  backtracking. Lesson: real formats (PDF) need grammar power; a fixed-layout DSL is not enough.
- **Interval Parsing Grammars** [M]. PLDI 2023. Each nonterminal parses inside an explicit byte
  interval, which gives random access (ELF section tables, ZIP central directories) a clean grammar
  meaning. Lesson: windows (`within len`) and offsets (`at off`) are the two primitives that make
  random-access formats declarative.
- **PADS and the Data Description Calculus** [M]. PLDI 2005, POPL 2006. Dependent types as the
  semantics of data description languages. Lesson: the type theory exists; refinements and dependent
  lengths are the standard model.
- **DFDL** [H]. An Open Grid Forum standard that annotates XML Schema. Powerful and complete;
  universally described as verbose. Lesson: do not make people write the description in a format
  meant for something else (the same complaint applies, more mildly, to Kaitai's YAML).
- **binrw and deku** [H for existence, M for the critique]. `#[br(count = len)]` for reading and
  `#[bw(calc = data.len() as u32)]` for writing are separate attributes, so read and write can
  disagree and nothing checks it. Lesson: one clause must drive both directions.
- **Construct** (Python) [H]. Beloved for its compact combinators (`Struct`, `Switch`, `Prefixed`,
  `this.len`); slow, and dynamic errors only. Lesson: combinator vocabulary (prefixed, switch, repeat
  until, pointer) is the right set of primitives; it needs static checking.
- **Erlang bit syntax** [H]. Segments such as `Version:4, Ihl:4, Rest/binary` between double angle brackets, in patterns and constructors, the
  most loved bit-level notation in a general language. The same notation parses (pattern) and builds
  (expression). Lesson: bidirectional by symmetry of syntax; sub-byte widths written as numbers.
- **Wuffs** [H]. Google's memory-safe language for decoders (PNG, GIF, JPEG and others) used in
  Chromium; its checker proves every index and arithmetic operation in bounds, and its PNG decoder is
  reported faster than libpng [M]. Lesson: safety and speed are not in tension when bounds are
  static facts; Yan's solver (8.6) plays this role.
- **010 Editor templates and FormatFuzzer** [M]. ICSE 2022 turned 010 binary templates into
  format-aware fuzzers and generators. Lesson: a description should also generate valid inputs; that
  is free if the description is bidirectional.
- **Zip overlap bombs** [H]. Fifield, WOOT 2019: overlapping file entries in the central directory
  give a 42 kB file that expands to 4.5 PB without nested archives. Lesson: offset-based formats need a
  default "regions do not overlap" check, with overlap as an explicit opt-in.
- **LangSec** [H]. Sassaman, Patterson, Bratus and others: "shotgun parsing" (validation scattered
  through processing code) is a root cause of exploitable bugs; recognize the full input before acting
  on it. Lesson: the generated `parse` is a full recognizer; nothing hands half-checked data onward.
- **Spicy** (Zeek) [M]. ACSAC 2016. Incremental parsing of network streams that arrive in pieces.
  Lesson: streaming is a real need for network monitors and belongs in the design, even if after the
  first slice.
- **P4 header types and Zig packed structs** [H]. Bit-exact fixed layouts as plain types. Lesson: for
  fixed layouts, a struct-like declaration with widths in the type is all people want.

### 2.2 What users want (synthesized from issues, talks and docs; [M] overall)

1. **Write as well as read**, from the same source, with lengths, counts, offsets and checksums filled
   in by the writer.
2. **Zero-copy and zero-allocation** validation for untrusted input, with views into the buffer.
3. **Errors that name the field path and the byte offset**, not "unexpected EOF".
4. **Tagged unions and repetition** (until a terminator, to end of input, by count) without escaping
   to code.
5. **Random access** by offset, for container formats.
6. **Variable-length integers** (LEB128, VLQ, QUIC varints, ASN.1 lengths) as first-class types.
7. **A visual**: the hex view with the parse tree, or the RFC box diagram.
8. **An escape hatch** to real code for the one weird field, without losing the guarantees for the
   rest.
9. **Fuzzing and test generation** from the description.

### 2.3 What works and what fails

| Works | Fails |
| --- | --- |
| One declaration per field, in wire order | Read and write directives written separately (binrw/deku) |
| Widths in the type (`u4`, `u13`) | Byte order set implicitly by platform (C bitfields) |
| Refinements next to the field they constrain | Validation code apart from the layout (shotgun parsing) |
| Derived fields computed on write, checked on read | User keeps lengths consistent by hand (Kaitai serialization) |
| Validate in place, no allocation | Parse into a heap tree first, validate later |
| Diagrams as generated documentation | Diagrams as the only source (see experiment 5.2) |
| A small fixed vocabulary of combinators | A host format pressed into service (DFDL's XML, Kaitai's YAML) |
| Round-trip as a checked law | Malleability nobody chose (free padding, overlong varints, unchecked unknown tags) |

### 2.4 What Yan already gives the surface

- **Views and `from`** (4.5): a parsed record can borrow the buffer with a lifetime the checker tracks,
  so zero-copy is a type, not a convention.
- **Refinements and the solver** (8.6): dependent lengths are slices whose bounds the solver proves;
  integer overflow in length arithmetic is a solver fact, not a runtime surprise.
- **`fail` and `uses none`**: parsing is pure and its failure is in the signature. Deterministic by
  construction.
- **`@layout(bits(uN))`** (5.2): LSB-first registers already have a core lowering.
- **Stage-0 metadata**: recovery (core back to `.ybf`) is exact, so the formatter can always print the
  source back.

The surface's whole job is to say, compactly, things the core can already check.

## 3. The framework: one description, six artifacts, four laws

Before syntax, the shape every proposal must fill. This is the framework the study recommends
regardless of the syntax chosen.

**One description yields six artifacts:**

| Artifact | Signature (Yan) | Promise |
| --- | --- | --- |
| `parse` | `T.parse(b: View[u8]) -> T from b fail ParseError` | full recognition; zero-copy record of views |
| `check` | `T.check(b: View[u8]) -> int fail ParseError` | validate only: no allocation, returns bytes consumed |
| `write` | `T.write(x: T, out: inout Bytes) fail WriteError` | fills every derived field; fails only on unrepresentable values |
| `size` | `T.size(x: T) -> int` | exact encoded size, for preallocation |
| `gen` | `T.gen(seed: u64) -> T` | valid values for fuzzing and property tests |
| `show` | `T.show(b: View[u8]) -> Str` | the dissector: field path, offset, width, value |

`yan doc` adds a seventh, the box diagram, generated and never hand-written.

**Every field has one of three roles:**

| Role | Meaning on parse | Meaning on write | Typical |
| --- | --- | --- | --- |
| fixed | must equal the constant | writes the constant | magic, version, reserved bits |
| derived | must equal the expression | computes the expression | lengths, counts, offsets, checksums |
| free | any value that meets its refinement | the record's value | payload fields |

**Bytes live in two spaces:** the *sequence* (fields in order, from the current position) and *regions*
(sub-layouts at an offset or inside a window). Windows and offsets are the two primitives from
interval parsing; every random-access container is a composition of them.

**Four laws every layout obeys (checked by `gen`-driven property tests in conformance, and by the
solver where it can):**

1. **Round-trip.** `T.parse(T.write(x)) == Ok(x)` for every `x` that `write` accepts.
2. **Canonical.** For a canonical layout, `T.write(T.parse(b)) == b` for every `b` that `parse`
   accepts. A layout is canonical unless it says `lax` somewhere; the formatter shows where.
3. **Total and linear.** `parse` terminates on every input, reads each byte a bounded number of times,
   and runs in time linear in the input (repetitions must consume at least one byte).
4. **Contained.** Regions do not overlap each other or the sequence unless a field says `shared`.

Laws 2 and 4 are the opinionated part: malleability and overlap are allowed, but they are visible.

## 4. Five proposals

Each proposal pushes one axis as far as it goes. All five write the same five tasks. The samples are
complete for the task as defined in section 1.

### P1: Bit syntax (axis: brevity)

Erlang's bit syntax grown into declarations. A layout is a braced field list; widths are bare numbers
(bits); `/T` reads the field as type `T`; `?e` is a refinement; `=e` is fixed or derived; `[n]` is
bytes; `*` repeats; `^e` ends a repetition after the element where `e` holds; `#x` is the size of
`x`; `&x` is the offset of `x`; `@e` places a region at offset `e`; `>k{...}` switches on `k`.

#### P1 / T1

```
Ipv4Header = be{ version:4 =4, ihl:4 ?ihl>=5, dscp:6, ecn:2,
  total_len:16 ?total_len>=ihl*4, id:16, flags:3, frag_off:13, ttl:8,
  proto:8/IpProto, checksum:16 =inet(@), src:32/Ipv4Addr, dst:32/Ipv4Addr,
  options:[(ihl-5)*4] }
```

#### P1 / T2

```
Png = be{ "\x89PNG\r\n\x1a\n", chunks:Chunk* ^kind=="IEND" }
Chunk = be{ len:32 =#data, kind:[4], data:[len]>kind{ "IHDR":Ihdr, _:[..] },
  crc:32 =crc32(kind,data) }
Ihdr = be{ width:32 ?width>0, height:32 ?height>0, depth:8, color:8,
  "\0\0", interlace:8 ?interlace<=1 }
```

#### P1 / T3

```
Tlv = { tag:leb, len:leb =#value, value:[len] }
TlvStream = Tlv*
```

#### P1 / T4

```
Archive = le{ "YARC", version:16 =1, count:16 =#dir, dir_off:32 =&dir,
  dir:@dir_off Entry[count] }
Entry = le{ name_len:16 =#name, name:utf8[name_len], data_off:32 =&data,
  size:32 =#data, data:@data_off [size] }
```

#### P1 / T5

```
UartCtrl = reg32{ en:1, parity:2/Parity, stop:1, tx_done:1!w1c, _:3, div:16 ?div>0, _:8 }
```

**Verdict.** The shortest in bytes, though only slightly cheaper in tokens than the mix (section 5). Expert-pleasing for a week, then a wall of sigils: `@`
means "this layout's bytes" in T1 and "at offset" in T4, `[n]` means bytes but `Entry[count]` means
an array, and `#`, `&`, `^`, `>` each carry a meaning a newcomer must memorize. Diffs are poor (one
line holds many fields). Keep: widths as numbers in the type, the `size of` and `offset of` ideas as
derived-field expressions, the dense switch.

### P2: Diagram (axis: fidelity to the source document)

Protocol specifications are drawn as box diagrams (RFC 791, RFC 9293, datasheets). P2 makes the
diagram the source: column width gives bit width (two columns per bit, the RFC convention), and a
`where` block carries everything a picture cannot say.

#### P2 / T1

```
layout Ipv4Header be
  +-------+-------+-----------+---+-------------------------------+
  |version|  ihl  |    dscp   |ecn|           total_len           |
  +-------------------------------+-----+-------------------------+
  |               id              |flags|         frag_off        |
  +---------------+---------------+-------------------------------+
  |      ttl      |     proto     |            checksum           |
  +---------------------------------------------------------------+
  |                              src                              |
  +---------------------------------------------------------------+
  |                              dst                              |
  +---------------------------------------------------------------+
  /                            options                            /
  +---------------------------------------------------------------+
  where
    version = 4
    ihl >= 5
    total_len >= ihl * 4
    proto as IpProto
    checksum = inet_sum(self)
    src as Ipv4Addr
    dst as Ipv4Addr
    options: bytes[(ihl - 5) * 4]
```

#### P2 / T2

```
layout Png be
  magic = "\x89PNG\r\n\x1a\n"
  chunks: Chunk[..] through .kind == "IEND"

layout Chunk be
  +---------------------------------------------------------------+
  |                              len                              |
  +---------------------------------------------------------------+
  |                              kind                             |
  +---------------------------------------------------------------+
  /                              data                             /
  +---------------------------------------------------------------+
  |                              crc                              |
  +---------------------------------------------------------------+
  where
    len = data.size
    kind: bytes[4]
    data: in len match kind
      "IHDR" -> Ihdr
      _ -> bytes[..]
    crc = crc32(kind, data)
```

#### P2 / T3

```
layout Tlv
  tag: leb128
  len: leb128 = value.size
  value: bytes[len]

layout TlvStream
  items: Tlv[..]
```

(No diagram: variable-length integers have no box.)

#### P2 / T4

```
layout Archive le
  +---------------------------------------------------------------+
  |                             magic                             |
  +-------------------------------+-------------------------------+
  |            version            |             count             |
  +-------------------------------+-------------------------------+
  |                            dir_off                            |
  +---------------------------------------------------------------+
  where
    magic = "YARC"
    version = 1
    count = dir.len
    dir_off = offset(dir)
    dir: at dir_off Entry[count]

layout Entry le
  name_len: u16 = name.size
  name: str[name_len]
  data_off: u32 = offset(data)
  size: u32 = data.size
  data: at data_off bytes[size]
```

#### P2 / T5

```
register UartCtrl u32
  +---------------+-------------------------------+-----+-+-+---+-+
  |       _       |              div              |  _  |t|s|par|e|
  +---------------+-------------------------------+-----+-+-+---+-+
  where
    e: en
    par: parity as Parity
    s: stop
    t: tx_done w1c
    div > 0
```

**Verdict.** Beautiful where the format is a fixed grid, and that is where people already read it.
It breaks in three places: variable parts (`/ options /` carries no width; the `where` block does the
work), sub-byte fields too narrow for their names (one-bit fields need aliases and a legend, as TCP's
flag letters do in RFC 9293), and editing (renaming `ihl` to `header_len` changes its width unless the
art is redrawn). Experiment 5.2 shows the last one is silent. Keep: the diagram as a generated and
checked artifact, never as source.

### P3: Contract (axis: safety and explicitness)

EverParse and Vest taken to the surface. Nothing is implicit: byte and bit order are stated, every
field states its role (`fixed`, `derived`, `free`, `require`), every repetition and length states a
bound, and the layout states how much it consumes and whether it is canonical. Words, not sigils.

#### P3 / T1

```
layout Ipv4Header
  byte order big
  bit order msb first
  size between 20 and 60
  field version: u4, fixed 4
  field ihl: u4, require ihl >= 5
  field dscp: u6, free
  field ecn: u2, free
  field total_len: u16, require total_len >= ihl * 4
  field id: u16, free
  field flags: u3, free
  field frag_off: u13, free
  field ttl: u8, free
  field proto: u8, decode as IpProto, unknown fails
  field checksum: u16, derived inet_sum(self with checksum = 0)
  field src: u32, decode as Ipv4Addr
  field dst: u32, decode as Ipv4Addr
  field options: bytes[(ihl - 5) * 4], bound 40
  consumes exactly ihl * 4
  canonical
```

#### P3 / T2

```
layout Png
  byte order big
  field signature: bytes[8], fixed "\x89PNG\r\n\x1a\n"
  field chunks: Chunk[..], through last.kind == "IEND", bound 2^20 items
  consumes all
  canonical

layout Chunk
  byte order big
  field len: u32, derived data.size, bound 2^31 - 1
  field kind: bytes[4], require kind.all(is_ascii_alpha)
  field data: window len of choice on kind
    "IHDR": Ihdr
    otherwise: bytes[..]
  field crc: u32, derived crc32(kind, data)
  canonical
```

#### P3 / T3

```
layout Tlv
  field tag: leb128, canonical encoding
  field len: leb128, derived value.size, bound 2^32
  field value: bytes[len]
  canonical

layout TlvStream
  field items: Tlv[..], to end
  consumes all
```

#### P3 / T4

```
layout Archive
  byte order little
  field magic: bytes[4], fixed "YARC"
  field version: u16, fixed 1
  field count: u16, derived dir.len, bound 4096
  field dir_off: u32, derived offset of dir
  region dir: Entry[count] at dir_off, disjoint
  canonical

layout Entry
  byte order little
  field name_len: u16, derived name.size, bound 255
  field name: utf8[name_len]
  field data_off: u32, derived offset of data
  field size: u32, derived data.size
  region data: bytes[size] at data_off, disjoint
  canonical
```

#### P3 / T5

```
register UartCtrl
  width 32
  bit order lsb first
  field en: u1, access rw
  field parity: u2, decode as Parity, access rw
  field stop: u1, access rw
  field tx_done: u1, access w1c
  reserved u3, write 0, read ignored
  field div: u16, require div > 0, access rw
  reserved u8, write 0, read ignored
```

**Verdict.** Auditable and impossible to misread, and about 35 percent more bytes and 20 percent more tokens than the mix. Most of the
words restate defaults that a good design can make safe: `free` is the absence of a clause, `bound` is
derivable from the field width in nearly every case (a `u16` count is bounded by 65535), and
`canonical` should be the default with `lax` as the marked exception. Keep: the three roles, `disjoint`
regions as the default, `unknown fails` for closed enums, `consumes all` at the top level, and
reserved bits that are fixed on write and ignored on read.

### P4: Grammar (axis: expressive power)

DaeDaLus and interval grammars with a light syntax. A layout is a production; productions take
parameters, sequence by juxtaposition, choose with `match`, repeat with `*`, place with `@`, and
window with `[n](G)`. Productions can return semantic values, so varints and other codecs are
definable in the language itself.

#### P4 / T1

```
Ipv4Header := be
  version:u4 = 4
  ihl:u4 { ihl >= 5 }
  dscp:u6 ecn:u2
  total_len:u16 { total_len >= ihl * 4 }
  id:u16 flags:u3 frag_off:u13 ttl:u8
  proto:u8 => IpProto
  checksum:u16 = inet_sum($)
  src:u32 => Ipv4Addr  dst:u32 => Ipv4Addr
  options:bytes((ihl - 5) * 4)
```

#### P4 / T2

```
Png := be "\x89PNG\r\n\x1a\n" chunks:Chunk*! (.kind == "IEND")
Chunk := be len:u32 = #data kind:bytes(4) data:[len](Body(kind)) crc:u32 = crc32(kind, data)
Body(k: bytes) := match k
  "IHDR" -> Ihdr
  "tEXt" -> key:cstr text:rest
  _ -> rest
```

#### P4 / T3

```
leb -> u64 := b:u8 (if b < 0x80 then b else (b & 0x7f) | leb << 7)
  write x = if x < 0x80 then u8(x) else u8(x & 0x7f | 0x80) leb(x >> 7)
Tlv := tag:leb len:leb = #value value:bytes(len)
TlvStream := Tlv* $end
```

#### P4 / T4

```
Archive := le "YARC" version:u16 = 1 count:u16 = #dir dir_off:u32 = &dir
  dir:@dir_off Entry{count}
Entry := le name_len:u16 = #name name:utf8(name_len)
  data_off:u32 = &data size:u32 = #data data:@data_off bytes(size)
```

#### P4 / T5

```
UartCtrl := bits32 lsb en:b1 parity:u2 => Parity stop:b1 tx_done:b1 !w1c _:u3 = 0 div:u16 { div > 0 } _:u8 = 0
```

**Verdict.** The only proposal that can describe PDF-class formats and define its own codecs. The
price shows in T3: once a production returns a computed value, bidirectionality is no longer automatic
and the author must write the inverse (`write x = ...`), which the round-trip law then has to test.
Free-form juxtaposition also loses the one-field-per-line rhythm that makes diffs and reviews easy.
Keep: parameters, `match` on a field, windows, offsets, and a codec escape hatch, but with the inverse
mandatory and the law checked, and with ordinary Yan functions instead of grammar-level semantic
actions.

### P5: Table (axis: regularity for agents and tools)

The layout is a Markdown table with fixed columns, so it renders as documentation on any forge, has
one way to write every field, and has a grammar small enough for constrained decoding. Unions are
separate tables, since tables do not nest.

#### P5 / T1

```
layout Ipv4Header be

| field     | type     | as       | where                | value              |
| --------- | -------- | -------- | -------------------- | ------------------ |
| version   | u4       |          |                      | 4                  |
| ihl       | u4       |          | ihl >= 5             |                    |
| dscp      | u6       |          |                      |                    |
| ecn       | u2       |          |                      |                    |
| total_len | u16      |          | total_len >= ihl * 4 |                    |
| id        | u16      |          |                      |                    |
| flags     | u3       |          |                      |                    |
| frag_off  | u13      |          |                      |                    |
| ttl       | u8       |          |                      |                    |
| proto     | u8       | IpProto  |                      |                    |
| checksum  | u16      |          |                      | inet_sum(self)     |
| src       | u32      | Ipv4Addr |                      |                    |
| dst       | u32      | Ipv4Addr |                      |                    |
| options   | bytes[(ihl - 5) * 4] | |                     |                    |
```

#### P5 / T2

```
layout Png be

| field  | type      | as | where                  | value                |
| ------ | --------- | -- | ---------------------- | -------------------- |
| magic  | bytes[8]  |    |                        | "\x89PNG\r\n\x1a\n"  |
| chunks | Chunk[..] |    | through .kind == "IEND" |                     |

layout Chunk be

| field | type                | as | where | value            |
| ----- | ------------------- | -- | ----- | ---------------- |
| len   | u32                 |    |       | data.size        |
| kind  | bytes[4]            |    |       |                  |
| data  | in len Body(kind)   |    |       |                  |
| crc   | u32                 |    |       | crc32(kind, data) |

union Body on kind

| tag    | type      |
| ------ | --------- |
| "IHDR" | Ihdr      |
| _      | bytes[..] |
```

#### P5 / T3

```
layout Tlv

| field | type      | as | where | value      |
| ----- | --------- | -- | ----- | ---------- |
| tag   | leb128    |    |       |            |
| len   | leb128    |    |       | value.size |
| value | bytes[len] |   |       |            |

layout TlvStream

| field | type    | as | where | value |
| ----- | ------- | -- | ----- | ----- |
| items | Tlv[..] |    |       |       |
```

#### P5 / T4

```
layout Archive le

| field   | type                    | as | where | value        |
| ------- | ----------------------- | -- | ----- | ------------ |
| magic   | bytes[4]                |    |       | "YARC"       |
| version | u16                     |    |       | 1            |
| count   | u16                     |    |       | dir.len      |
| dir_off | u32                     |    |       | offset(dir)  |
| dir     | at dir_off Entry[count] |    |       |              |

layout Entry le

| field    | type                    | as | where | value        |
| -------- | ----------------------- | -- | ----- | ------------ |
| name_len | u16                     |    |       | name.size    |
| name     | str[name_len]           |    |       |              |
| data_off | u32                     |    |       | offset(data) |
| size     | u32                     |    |       | data.size    |
| data     | at data_off bytes[size] |    |       |              |
```

#### P5 / T5

```
register UartCtrl u32

| field   | type | as     | where   | value | access |
| ------- | ---- | ------ | ------- | ----- | ------ |
| en      | bit  |        |         |       | rw     |
| parity  | u2   | Parity |         |       | rw     |
| stop    | bit  |        |         |       | rw     |
| tx_done | bit  |        |         |       | w1c    |
| _       | u3   |        |         | 0     |        |
| div     | u16  |        | div > 0 |       | rw     |
| _       | u8   |        |         | 0     |        |
```

**Verdict.** Perfectly regular and self-documenting, and the most expensive in tokens (section 5):
column padding and pipes cost more than the content in sparse tables. Alignment makes every edit a
multi-line diff unless the formatter stops aligning, at which point the readability is gone. Unions
split into separate tables, which hides the structure. Keep: the fixed clause order (type, `as`,
`where`, `=`), so every field has exactly one spelling; the idea that the source renders as a table in
docs, done by `yan doc` instead; and the small grammar.

### Scorecard

Scores 1 (poor) to 5 (best), my judgment after writing the samples; the size row is measured.

| Criterion | P1 Bit | P2 Diagram | P3 Contract | P4 Grammar | P5 Table |
| --- | --- | --- | --- | --- | --- |
| Brevity (measured, 5.1) | 5 | 2 | 3 | 5 | 1 |
| Read by a newcomer | 2 | 5 | 4 | 2 | 4 |
| Safety visible in source | 2 | 2 | 5 | 3 | 3 |
| Expressive power | 3 | 2 | 3 | 5 | 2 |
| Bidirectional without extra code | 4 | 4 | 5 | 2 | 4 |
| Diff and review friendliness | 1 | 1 | 5 | 2 | 2 |
| Constrained decoding (small grammar) | 3 | 1 | 4 | 2 | 5 |
| Fits Yan's look (`.yn`) | 2 | 2 | 3 | 2 | 2 |

No proposal wins. P3 is the safest, P4 the strongest, P1 the shortest. The mix aims for P3's
guarantees at close to P1's token cost, by making the safe choice the default instead of a word.

## 5. Experiments and measurements

### 5.1 Size of each proposal

Method: a script (`count.py`, thread scratchpad) extracts the first sample block under each
`#### Pn / Tm` heading of this file and each `#### Mix / Tm` heading of section 6, and counts bytes,
non-blank lines and two lexical token proxies: A counts each word and each punctuation character; B
counts each word and each run of one repeated punctuation character (so a `-----` rule is one token).
No real tokenizer was available offline. BPE tokenizers sit between A and B on punctuation-heavy text
(they merge short runs of dashes and spaces, not long ones), so the ranking is what matters.

Caveat on fairness: P1 and the mix write out the `Ihdr` layout in T2 (and the mix adds `Rgb` and a
`PLTE` arm); the others only reference it. That adds about 12 lines and 90 tokens to the mix.

| Proposal | Bytes | Lines | Tokens A | A vs mix | Tokens B | B vs mix |
| --- | --- | --- | --- | --- | --- | --- |
| P1 Bit syntax | 870 | 16 | 357 | 0.85 | 355 | 0.88 |
| P2 Diagram | 3,139 | 79 | 1,480 | 3.54 | 404 | 1.00 |
| P3 Contract | 2,098 | 69 | 490 | 1.17 | 486 | 1.21 |
| P4 Grammar | 1,057 | 25 | 372 | 0.89 | 367 | 0.91 |
| P5 Table | 4,014 | 70 | 1,026 | 2.45 | 692 | 1.72 |
| Mix | 1,538 | 69 | 418 | 1.00 | 403 | 1.00 |

Reading:

- **The sigil proposals save little.** P1 and P4 pack several fields per line, yet cost only 10 to 15
  percent fewer tokens than the mix, and less than that once the mix's extra `Ihdr` and `Rgb` are
  discounted. Words like `where` are one token; `?` plus the repeated field name is two or more.
  Brevity in characters is not brevity in tokens, and the mix gives up almost nothing for
  readability and one-field-per-line diffs.
- **Explicitness costs about 20 percent.** P3 is the mix plus the words for its defaults.
- **Tables and diagrams pay for layout, not content.** P5 costs 70 to 145 percent more than the mix,
  nearly all of it padding and pipes. P2 costs anywhere from the same as the mix to three and a half
  times as much, depending on how the tokenizer treats rule lines; for an agent writing a diagram,
  every column must also be counted right, which is the expensive part (experiment 5.2).

### 5.2 Can a diagram be source? (prototype)

`diagram.py` renders a field list as an RFC box diagram (two columns per bit) and reads one back.
IPv4's 13 fixed fields round-trip exactly (160 bits). Two failures showed up at once:

- **One-bit fields do not fit their names.** A 1-bit field is one column wide. The register in T5
  needs aliases `e`, `s`, `t` and a legend, as RFC 9293 draws TCP's flags as vertical letters.
- **Renames change widths silently.** Reading the cell `|tx_done|` gives a 4-bit field: the parser
  cannot tell a wide name from a wide field. A rename in a diagram-as-source file is a layout change
  that compiles.

Conclusion: the diagram is documentation that must be generated from the layout (and may be checked
against a hand-drawn one in a doc comment), never the source of widths.

### 5.3 Where the bytes go in a real header

In the mix's T1, every line after the header is one field, and the only clauses are those that say
something: two refinements, one fixed value, one derived checksum, one enum mapping, one dependent
length. Every clause in P3 that the mix drops (`free`, `bound`, `canonical`, `byte order` twice) is a
default the mix makes safe. This is the core argument of the mix: the defaults carry the safety.

### 5.4 Script output

```
P1 bytes=870 lines=16 tokens=357 ratio=0.85 runs=355 rratio=0.88
P2 bytes=3139 lines=79 tokens=1480 ratio=3.54 runs=404 rratio=1.00
P3 bytes=2098 lines=69 tokens=490 ratio=1.17 runs=486 rratio=1.21
P4 bytes=1057 lines=25 tokens=372 ratio=0.89 runs=367 rratio=0.91
P5 bytes=4014 lines=70 tokens=1026 ratio=2.45 runs=692 rratio=1.72
Mix bytes=1538 lines=69 tokens=418 ratio=1.00 runs=403 rratio=1.00
```

## 6. The mix: the consolidated proposal

### 6.1 Principles

1. **Looks like Yan.** Two-space indentation, `--` comments, `--- ` doc comments, `name: type` fields,
   `match` arms with `->`, Yan expressions everywhere an expression goes. A `.yn` reader can read a
   `.ybf` file on first sight.
2. **One field per line, one spelling per field.** Clauses in a fixed order: `name: wire [as T]
   [where e] [= e] [flags]`. The formatter enforces it. (From P5 and P3.)
3. **Safe by default, loose by word.** Canonical, contained, closed enums, reserved bits fixed, full
   consumption at the top level. `lax`, `shared`, `open` and `any` are the visible opt-outs. (From
   P3, with the words inverted.)
4. **Derived, not maintained.** Lengths, counts, offsets and checksums are expressions; the writer
   computes them and the parser checks them. The user never sets one. (From Nail and P1.)
5. **Two primitives for random access.** `in n` (a window of `n` bytes the sub-layout must fill) and
   `at off` (a region at an offset). (From interval grammars and P4.)
6. **Power through Yan, not through the grammar.** No semantic actions in layouts. A codec that the
   layout vocabulary cannot express is a pair of Yan functions with a round-trip law. (From P4, with
   the inverse mandatory.)
7. **The picture is generated.** `yan doc` draws the box diagram and the field table; a hand-drawn
   diagram in a doc comment is checked against the layout. (From P2 and P5, inverted.)

### 6.2 The five tasks in the mix

#### Mix / T1

```
--- IPv4 header, RFC 791.
layout Ipv4Header be
  version: u4 = 4
  ihl: u4 where ihl >= 5
  dscp: u6
  ecn: u2
  total_len: u16 where total_len >= ihl * 4
  id: u16
  flags: u3
  frag_off: u13
  ttl: u8
  proto: u8 as IpProto open
  checksum: u16 = inet_sum(self)
  src: u32 as Ipv4Addr
  dst: u32 as Ipv4Addr
  options: bytes[(ihl - 5) * 4]
```

`proto` is `open` because IANA assigns new protocol numbers and a router must not reject them;
without `open`, an unknown value fails the parse. `inet_sum(self)` reads the layout's own bytes with
the field being computed read as zero, the usual checksum convention, so it is not a cycle.

#### Mix / T2

```
--- PNG, ISO/IEC 15948.
layout Png be
  magic = "\x89PNG\r\n\x1a\n"
  chunks: Chunk[..] through .kind == "IEND"

layout Chunk be
  len: u32 = data.size
  kind: bytes[4]
  data: in len match kind
    "IHDR" -> Ihdr
    "PLTE" -> Rgb[..]
    _ -> bytes[..]
  crc: u32 = crc32(kind, data)

layout Ihdr be
  width: u32 where width > 0
  height: u32 where height > 0
  depth: u8
  color: u8 as ColorType
  compression = "\0"
  filter = "\0"
  interlace: u8 where interlace <= 1

layout Rgb
  r: u8
  g: u8
  b: u8
```

A field with no type and a literal (`magic = "..."`) is a fixed byte string; its width is the
literal's. `through` repeats until the element where the condition holds, that element included.
`in len` parses `data` inside exactly `len` bytes: the arm must fill the window, and `bytes[..]` or
`Rgb[..]` fill it by construction. On write, `len` is computed from the encoded size of `data`.

#### Mix / T3

```
--- Tag-length-value records, lengths as unsigned LEB128.
layout Tlv
  tag: leb128
  len: leb128 = value.size
  value: bytes[len]

layout TlvStream
  items: Tlv[..]
```

`leb128` is a standard codec: parse rejects overlong encodings, so the layout stays canonical.
No byte order is needed: the layout has no multi-byte fixed-width integer, and the checker requires
`be` or `le` exactly when one appears. A top-level layout must consume all of its input;
`Tlv[..]` repeats to the end.

A user codec, for a varint the standard set lacks, is ordinary Yan with a law:

```
codec zigzag32 -> i32
  parse(b: View[u8]) -> (i32, int) fail ParseError = ...
  write(x: i32, out: inout Bytes) = ...
```

The compiler generates the round-trip property test from the pair; conformance runs it with `gen`.

#### Mix / T4

```
--- A directory archive: header, then a directory and data at offsets.
layout Archive le
  magic = "YARC"
  version: u16 = 1
  count: u16 = dir.len
  dir_off: u32 = offset(dir)
  dir: at dir_off Entry[count]

layout Entry le
  name_len: u16 = name.size
  name: str[name_len]
  data_off: u32 = offset(data)
  size: u32 = data.size
  data: at data_off bytes[size]
```

`at` regions must not overlap each other or the sequence (law 4); a field that may alias another
says `shared`, which is how a format that deduplicates data says so on purpose. On write, regions are
placed after the sequence in declaration order, depth first, so the output is deterministic and
`offset(...)` is known before the bytes are emitted. `str[n]` is `n` bytes of checked UTF-8.

#### Mix / T5

```
--- UART control register, LSB first.
register UartCtrl u32
  en: bit
  parity: u2 as Parity
  stop: bit
  tx_done: bit w1c
  _: u3
  div: u16 where div > 0
  _: u8
```

`register` is the LSB-first word form; it lowers to `@layout(bits(u32))` (5.2), and the widths must
sum to 32. Reserved fields (`_`) are written as zero and ignored on read, the datasheet convention;
`_: u3 any` would make them free, and `_: u3 = 0b101` fixes another value. Access flags are `ro`,
`wo`, `w1c` and `w1s`; the default is read-write. `bit` is `u1` read as `bool`.

### 6.3 Grammar sketch

```
file        = {use} {item}
item        = [doc] (layout | register | codec | enum | yan-item)
layout      = "layout" NAME [params] [order] NEWLINE INDENT field {field} DEDENT
register    = "register" NAME uint NEWLINE INDENT rfield {rfield} DEDENT
order       = "be" | "le"
params      = "(" NAME ":" type {"," NAME ":" type} ")"
field       = fname ":" wire ["as" type ["open"]] ["where" expr] ["=" expr] {fflag} NEWLINE
            | fname "=" literal NEWLINE                                    -- fixed byte string
            | fname ":" place NEWLINE
            | fname ":" "in" expr "match" expr NEWLINE INDENT arm {arm} DEDENT
fname       = NAME | "_"
wire        = uint | sint | "bit" | "f32" | "f64" | "bytes" "[" len "]" | "str" "[" len "]"
            | NAME [args] | wire "[" len "]" [repeat]
len         = expr | ".."
repeat      = "through" expr
place       = "at" expr wire ["shared"] | "in" expr wire
arm         = (literal | "_") "->" wire NEWLINE
fflag       = "lax" | "any" | "ro" | "wo" | "w1c" | "w1s" | "shared"
rfield      = fname ":" ("bit" | uint) ["as" type ["open"]] ["where" expr] ["=" expr] {fflag} NEWLINE
uint        = "u" INT
sint        = "i" INT
codec       = "codec" NAME "->" type NEWLINE INDENT yan-fn yan-fn DEDENT
```

About 25 productions on top of Yan expressions and types, small enough to export as a GBNF or
llguidance grammar for constrained decoding, as the agent harness plan already intends for the main
surfaces.

### 6.4 Semantics in one table

| Form | Parse | Write |
| --- | --- | --- |
| `f: u16` | read 2 bytes in layout order | write `x.f` |
| `f: u4` | read 4 bits, MSB first; a byte group must end on a byte boundary | pack bits |
| `f: u8 as E` | read, map to closed enum `E`; unknown fails | write the enum's value |
| `f: u8 as E open` | as above; unknown kept as `E.Unknown(n)` | write `n` back |
| `f: T where e` | read, then check `e` | check `e`, then write |
| `f: T = e` | read, then check `f == e` (fixed or derived) | compute `e`, write it |
| `f = "lit"` | check the bytes equal the literal | write the literal |
| `_: uN` | read and ignore (layout); ignore (register) | write zero |
| `f: bytes[n]` | a view of `n` bytes, no copy | copy the view's bytes |
| `f: T[n]`, `T[..]`, `T[..] through e` | count, to the end of the window, to a terminator | each element in order |
| `f: in n G` | parse `G` within exactly `n` bytes | encode `G`; its size feeds any `= f.size` |
| `f: at off G` | parse `G` at `off`; check containment | place after the sequence; fill `offset(f)` |
| `f: in n match k` | choose the arm by `k` within `n` bytes | encode the arm the value holds |

Expressions in `where` and `=` see earlier fields, layout parameters, `self` (the layout's own bytes,
with the field being computed read as zero), `x.size`, `x.len` and `offset(x)`. Derived expressions may
also see later fields, which the writer resolves in dependency order; a cycle is an error. The parser
never needs a later field, because a derived field is checked once the bytes it depends on are read.

### 6.5 What the mix chose not to do

- **No sigils for clauses** (P1, P4). `where`, `as`, `at`, `in`, `through` are words a reviewer can
  search for.
- **No diagram or table as source** (P2, P5). Both are generated; experiment 5.2 shows why.
- **No mandatory `bound`** (P3). Widths bound counts; a size cap on the whole input is a parse
  option (`T.parse(b, max = 1 << 20)`), set at the call site where the threat model lives.
- **No grammar-level semantic actions** (P4). Codecs are Yan functions with a checked law.
- **No implicit byte order.** Required exactly when a multi-byte integer appears, never defaulted.

### 6.6 How the mix meets the brief

- **Concise:** each line is one field and holds only what is not a default.
- **Powerful:** windows, offsets, unions, repetition, parameters, codecs; and six artifacts from one
  source.
- **Opinionated without being obnoxious:** the safe choice is free and the loose choice costs one
  visible word; nothing is forbidden.
- **Crafted:** the same field line means the same thing in both directions; the laws make that a
  promise the conformance suite checks, not a hope.

## 7. Open questions

1. **Byte-string literals.** `.ybf` uses `"\x89PNG..."` in byte positions. Spec 17 says `bytes` has
   no literal syntax. Either the surface defines string-in-byte-position as UTF-8 bytes with `\x`
   escapes (proposed), or the core gains `b"..."`.
2. **`through` versus `until`.** Inclusive termination is what PNG, and most chunked formats, need.
   An exclusive form (stop before an element, peeked) is rarer; propose leaving it out until a format
   needs it.
3. **Streaming.** `parse` takes a whole `View[u8]`. Network monitors (Spicy's niche) need resumable
   parsing over pieces. The laws hold for a resumable parser too; the open part is the API
   (`T.parser()` with `feed`) and whether `at` regions are allowed in streaming layouts (probably not).
4. **Checksums over spans other than `self`.** PNG's CRC covers `kind` and `data`; the mix passes
   fields. TCP's checksum covers a pseudo-header from the enclosing IP layer: that needs layout
   parameters (`layout TcpSegment(src: u32, dst: u32) be`), already in the grammar, and a std helper.
5. **Side channels.** Vest claims resistance to basic digital side channels. Whether `.ybf` should
   offer a `constant_time` layout flag (no data-dependent branches on secret fields) for TLS-like
   uses is open; it is a lowering concern, not syntax.
6. **Bit order inside multi-byte bit groups.** `be` layouts pack sub-byte fields MSB first, and
   `register` packs LSB first. A few formats (some radio and video headers) are little-endian with
   MSB-first bits; whether `le msb` needs a spelling or a codec is open.
7. **Relation to Yan Schema.** `.ysd` describes types whose encoding the toolchain chooses; `.ybf`
   describes encodings someone else chose. A Schema type could name a `.ybf` layout as its wire form;
   the boundary deserves a note when both surfaces are built.
8. **Diagram check in doc comments.** Is a checked hand-drawn diagram worth the parser, or is the
   generated diagram enough?

## Sources

- Kaitai Struct 0.11 release notes: https://kaitai.io/news/2025/09/07/kaitai-struct-v0.11-released.html
- Kaitai Struct serialization guide: https://doc.kaitai.io/serialization.html
- EverParse and 3D: https://project-everest.github.io/everparse/ and https://project-everest.github.io/everparse/3d-lang.html
- Vest, USENIX Security 2025: https://www.usenix.org/conference/usenixsecurity25/presentation/cai-yi and https://github.com/secure-foundations/vest
- DaeDaLus, "Daedalus: Safer Document Parsing", PLDI 2024: https://doi.org/10.1145/3656410
- binrw: https://binrw.rs
- From prior knowledge, not re-fetched: Nail (Bangert and Zeldovich, OSDI 2014); Narcissus (Delaware
  and others, ICFP 2019); Interval Parsing Grammars (PLDI 2023); PADS (Fisher and Gruber, PLDI 2005)
  and the Data Description Calculus (Fisher, Mandelbaum and Walker, POPL 2006); DFDL (Open Grid
  Forum); Construct (Python); Erlang bit syntax; Wuffs (Google); FormatFuzzer (ICSE 2022); Fifield, "A
  better zip bomb" (WOOT 2019); LangSec (Sassaman, Patterson, Bratus and others); Spicy (Sommer and
  others, ACSAC 2016); P4; Zig packed structs.
