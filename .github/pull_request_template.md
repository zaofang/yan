## Issue

Closes #

## Stage claimed

Pick one. A stage is claimed only when the previous stage is proven (DL1).

- [ ] working
- [ ] right
- [ ] fast

## Definition of done (DL9)

### Working

- [ ] Runs on the interpreter; output attached
- [ ] Runs on native; output attached

### Right

- [ ] Conformance case proposed via `conform/*` (link)
- [ ] Differential run clean (`uv run python -m tools.difftest`); divergences allowlisted with an issue
- [ ] Spec rows and code agree

### Fast

- [ ] Callgrind instruction count against the committed baseline, figure recorded below
- [ ] Linked green differential run

## Evidence

Outputs, differential result, callgrind figure (with commit and date).
