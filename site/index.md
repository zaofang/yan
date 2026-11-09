# Yan 言

Yan is a programming language with one core and many surfaces. Every surface lowers to the same
core, Yan Core, which is fast, concurrent, deterministic and capability-safe.

<p class="cta"><a class="btn btn-run" href="play/">Open the playground</a> <a class="btn" href="tour/">Take the tour</a></p>

## The same program, two surfaces

<div class="pair">

```yag check
type Shape = Circle(r: int) | Rect(w: int, h: int) | Dot()

def area(s: Shape) -> int:
    match s:
        case Circle(r):
            return 3 * r * r
        case Rect(w, h):
            return w * h
        case Dot():
            return 0
```

```yn check
type Shape = Circle(r: int) | Rect(w: int, h: int) | Dot()

area(s: Shape) -> int uses none =
  Circle(r) -> 3 * r * r
  Rect(w, h) -> w * h
  Dot() -> 0
```

</div>

## What every surface gets

- **Capabilities.** A function reaches the terminal, files, network or clock only through
  capabilities it names. Code that names none is pure.
- **Determinism.** `par` blocks give the same result on every run, whatever the scheduling.
- **Checked failure.** Failures are part of a signature, handled with `else` or passed on with `?`.
- **Examples as tests.** Public functions carry `ex` lines the checker runs.
- **Native and interpreted.** The interpreter and the native backend agree, checked by differential tests.

## Surfaces

<div class="surfaces">
<a href="tour/"><img src="brand/file-icon-yag.svg" alt="" width="48" height="48"><b>Yan Agent</b><span>.yag</span><em>Python-shaped and typed, written by language models.</em></a>
<a href="tour/07-surfaces.html"><img src="brand/file-icon-yn.svg" alt="" width="48" height="48"><b>Yan</b><span>.yn</span><em>The syntax written for people.</em></a>
<a href="play/"><img src="brand/file-icon-yir.svg" alt="" width="48" height="48"><b>Yan Core</b><span>.yir</span><em>The core every surface lowers to.</em></a>
<a href="status/"><img src="brand/file-icon-ymt.svg" alt="" width="48" height="48"><b>Yan Meta</b><span>.ymt</span><em>Research surface, planned after M1.</em></a>
</div>

## Install

```sh
curl -fsSL https://github.com/atbraz/yan-lang/releases/latest/download/install.sh | sh
brew install atbraz/yan/yan
```

Releases cover macOS (arm64, x86_64) and Linux (x86_64, arm64).
