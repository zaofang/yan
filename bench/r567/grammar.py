"""Export the Yan Agent grammar (spec 13.2) for constrained decoding (spec 21.3, R567).

Reads the EBNF block of spec/13-agent-lexical-rules-and-grammar.md and writes
  yag.gbnf  for llama.cpp (and vLLM's guided_grammar)
  yag.lark  for llguidance and XGrammar's Lark front end
Both start with the code fence the harness asks the model for, so a constrained reply is exactly
one fenced block.

The masks are supersets of the language, never subsets, so they cannot block a valid program:
- Layout. INDENT and DEDENT match nothing and NEWLINE accepts any following indentation, so
  block structure is not enforced.
- Whitespace. Any run of spaces between tokens is allowed, so `defmain` is accepted.
- Keywords. NAME does not exclude reserved words.
The reference check is the toolchain; this grammar only narrows the sampler.

usage: uv run python bench/r567/grammar.py [OUT_DIR]
"""

import pathlib
import re
import sys
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = ROOT / "spec" / "13-agent-lexical-rules-and-grammar.md"
FENCE = "```yag\n"

# Terminals: GBNF text, Lark text.
TERMINALS: dict[str, tuple[str, str]] = {
    "NAME": (r"""[a-z] [a-z0-9]* ("_" [a-z0-9]+)*""", r"/[a-z][a-z0-9]*(_[a-z0-9]+)*/"),
    "TYPENAME": (r"[A-Z] [a-z0-9]* ([A-Z] [a-z0-9]*)*", r"/[A-Z][a-z0-9]*([A-Z][a-z0-9]*)*/"),
    "INT": (
        r"""[0-9] [0-9_]* | "0x" [0-9a-fA-F_]+ | "0o" [0-7_]+ | "0b" [01_]+""",
        r"/0x[0-9a-fA-F_]+|0o[0-7_]+|0b[01_]+|[0-9][0-9_]*/",
    ),
    "FLOAT": (
        r"""[0-9]+ "." [0-9]+ ([eE] [+-]? [0-9]+)? | [0-9]+ [eE] [+-]? [0-9]+""",
        r"/[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+/",
    ),
    "STRING": (
        r"""tq | dq | "r" dq""",
        r'''/"""([^"]|"[^"]|""[^"])*"""|"([^"\\\n]|\\.)*"|r"[^"\n]*"/''',
    ),
    "BYTES": (r""""b" dq""", r"""/b"([^"\\\n]|\\.)*"/"""),
    "FSTRING": (r""""f" dq""", r"""/f"([^"\\\n]|\\.)*"/"""),
    "TSTRING": (r""""t" dq""", r"""/t"([^"\\\n]|\\.)*"/"""),
    "CHAR": (r'''"'" ([^'\\\n] | "\\" [^\n]) "'"''', r"/'([^'\\\n]|\\.)'/"),
    "NEWLINE": (
        r"""sp ("#" [^\n]*)? "\n" (sp ("#" [^\n]*)? "\n")*""",
        r"/[ \t]*(#[^\n]*)?\n([ \t]*(#[^\n]*)?\n)*/",
    ),
}
GBNF_HELPERS = (
    'sp ::= [ \\t]*\n'
    'dq ::= "\\"" ([^"\\\\\\n] | "\\\\" [^\\n])* "\\""\n'
    'tq ::= "\\"\\"\\"" ([^"] | "\\"" [^"] | "\\"\\"" [^"])* "\\"\\"\\""\n'
)  # fmt: skip


# Where 13.2 is stricter than 13.1 or the lenient reader, widen it so the mask stays a superset:
# a docstring may open a body that has contract lines, and a module may open with comments.
OVERRIDES = {
    "fbody": "NEWLINE [STRING NEWLINE] {contract} {stmt}",
    "module": "[NEWLINE] {modattr} [docstring] {item}",
}


@dataclass
class Node:
    kind: str  # lit, ref, seq, alt, opt, rep
    text: str = ""
    kids: tuple[Node, ...] = ()


def strip_comment(line: str) -> str:
    out, quoted = "", False
    for ch in line:
        if ch == '"':
            quoted = not quoted
        if ch == "#" and not quoted:
            break
        out += ch
    return out.rstrip()


def productions(text: str) -> dict[str, str]:
    block = re.search(r"### 13\.2 Grammar.*?```\n(.*?)```", text, re.S)
    if block is None:
        raise SystemExit("13.2 grammar block not found")
    rules: dict[str, str] = {}
    name = ""
    for raw in block.group(1).splitlines():
        line = strip_comment(raw)
        if not line.strip():
            continue
        m = re.match(r"^(\w+)\s*=\s*(.*)$", line)
        if m and not raw[0].isspace():
            name = m.group(1)
            rules[name] = rules.get(name, "") + " " + m.group(2) if name in rules else m.group(2)
        else:
            rules[name] += " " + line.strip()
    return rules


TOKEN = re.compile(r'\s*("(?:[^"\\]|\\.)*"|\w+|[()\[\]{}|])')


def tokenize(rhs: str) -> list[str]:
    toks, pos = [], 0
    rhs = rhs.strip()
    while pos < len(rhs):
        m = TOKEN.match(rhs, pos)
        if m is None:
            raise SystemExit(f"cannot tokenize at {rhs[pos : pos + 20]!r}")
        if m.group(1) not in ("INDENT", "DEDENT"):
            toks.append(m.group(1))
        pos = m.end()
        while pos < len(rhs) and rhs[pos].isspace():
            pos += 1
    return toks


class Parser:
    def __init__(self, toks: list[str]):
        self.toks, self.i = toks, 0

    def peek(self) -> str | None:
        return self.toks[self.i] if self.i < len(self.toks) else None

    def take(self) -> str:
        t = self.toks[self.i]
        self.i += 1
        return t

    def alt(self) -> Node:
        alts = [self.seq()]
        while self.peek() == "|":
            self.take()
            alts.append(self.seq())
        return alts[0] if len(alts) == 1 else Node("alt", kids=tuple(alts))

    def seq(self) -> Node:
        items = []
        while self.peek() not in (None, "|", ")", "]", "}"):
            items.append(self.atom())
        return items[0] if len(items) == 1 else Node("seq", kids=tuple(items))

    def atom(self) -> Node:
        t = self.take()
        if t == "(":
            n = self.alt()
            self.take()
            return n
        if t == "[":
            n = self.alt()
            self.take()
            return Node("opt", kids=(n,))
        if t == "{":
            n = self.alt()
            self.take()
            return Node("rep", kids=(n,))
        if t.startswith('"'):
            return Node("lit", t[1:-1])
        return Node("ref", t)


def gbnf_name(n: str) -> str:
    return n.replace("_", "-")


def gbnf(n: Node, top: bool = False) -> str:
    match n.kind:
        case "lit":
            return '"' + n.text.replace('"', '\\"') + '"'
        case "ref":
            return gbnf_name(n.text.lower()) if n.text not in TERMINALS else "t-" + n.text.lower()
        case "seq":
            return " sp ".join(gbnf(k) for k in n.kids)
        case "alt":
            text = " | ".join(gbnf(k) for k in n.kids)
            return text if top else f"({text})"
        case "opt":
            return f"({gbnf(n.kids[0], True)})?"
        case _:
            return f"({gbnf(n.kids[0], True)})*"


def lark(n: Node, top: bool = False) -> str:
    match n.kind:
        case "lit":
            return '"' + n.text.replace('"', '\\"') + '"'
        case "ref":
            return n.text if n.text in TERMINALS else n.text.lower()
        case "seq":
            return " ".join(lark(k) for k in n.kids)
        case "alt":
            text = " | ".join(lark(k) for k in n.kids)
            return text if top else f"({text})"
        case "opt":
            return f"({lark(n.kids[0], True)})?"
        case _:
            return f"({lark(n.kids[0], True)})*"


def refs(n: Node) -> set[str]:
    return ({n.text} if n.kind == "ref" else set()) | {r for k in n.kids for r in refs(k)}


def export(text: str) -> tuple[str, str]:
    rules = {k: Parser(tokenize(v)).alt() for k, v in {**productions(text), **OVERRIDES}.items()}
    undefined = {r for n in rules.values() for r in refs(n)} - rules.keys() - TERMINALS.keys()
    if undefined:
        raise SystemExit(f"undefined symbols: {sorted(undefined)}")
    g = [f'root ::= "{FENCE.strip()}" "\\n" module sp "```"', GBNF_HELPERS.rstrip()]
    lk = [f'start: "{FENCE.strip()}" "\\n" module "```"', "%ignore /[ \\t]+/"]
    for name, node in rules.items():
        g.append(f"{gbnf_name(name)} ::= {gbnf(node, True)}")
        lk.append(f"{name}: {lark(node, True)}")
    for name, (gb, lr) in TERMINALS.items():
        g.append(f"t-{name.lower()} ::= {gb}")
        lk.append(f"{name}: {lr}")
    return "\n".join(g) + "\n", "\n".join(lk) + "\n"


def main() -> None:
    out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else pathlib.Path(__file__).parent / "out")
    out.mkdir(parents=True, exist_ok=True)
    g, lk = export(SPEC.read_text())
    (out / "yag.gbnf").write_text(g)
    (out / "yag.lark").write_text(lk)
    print(f"wrote {out}/yag.gbnf ({len(g.splitlines())} rules) and yag.lark")


if __name__ == "__main__":
    main()
