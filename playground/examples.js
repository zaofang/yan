// Generated from conformance programs; header comments dropped.
const EXAMPLES = [
  {
    "name": "Hello",
    "surface": "yag",
    "source": "pub def main() uses io:\n    io.print(\"hello, world\")\n",
    "from": "conformance/golden/g1_hello.yag"
  },
  {
    "name": "Word count",
    "surface": "yag",
    "source": "pub def count_words(text: str) -> dict[str, int]:\n    \"\"\"Counts space-separated words in first-appearance order.\"\"\"\n    ex count_words(\"a b a\") == {\"a\": 2, \"b\": 1}\n    counts: dict[str, int] = {}\n    for w in text.split(\" \"):\n        counts[w.copy()] = (counts.get(w) else 0) + 1\n    return counts\n\npub def main() -> fail FsError uses io:\n    with fs = fake_fs({\"in.txt\": \"b a b c a b\"}):\n        text = fs.read(\"in.txt\")?\n        for w, n in count_words(text).items():\n            io.print(f\"{w} {n}\")\n",
    "from": "conformance/golden/g4_word_count.yag"
  },
  {
    "name": "VWAP",
    "surface": "yag",
    "source": "class Trade:\n    sym: str\n    px: float\n    qty: int\n\nclass BadLine:\n    line: str\n\ndef parse_line(line: str) -> Trade fail BadLine:\n    p = line.split(\",\")\n    if len(p) != 3:\n        fail BadLine(line=line.copy())\n    px = p[1].parse_float() else fail BadLine(line=line.copy())\n    qty = p[2].parse_int() else fail BadLine(line=line.copy())\n    return Trade(sym=p[0].copy(), px=px, qty=qty)\n\npub def vwap(ts: list[Trade]) -> dict[str, float]:\n    ex vwap([Trade(sym=\"A\", px=10.0, qty=1), Trade(sym=\"A\", px=20.0, qty=3)]) == {\"A\": 17.5}\n    acc: dict[str, tuple[float, int]] = {}\n    for t in ts:\n        n, v = acc.get(t.sym) else (0.0, 0)\n        acc[t.sym.copy()] = (n + t.px * float(t.qty), v + t.qty)\n    return {s.copy(): nv[0] / float(nv[1]) for s, nv in acc.items()}\n\npub def main() -> fail BadLine uses io:\n    lines = [\"A,10.0,1\", \"B,5.0,2\", \"A,20.0,3\"]\n    ts = [parse_line(l)? for l in lines]\n    for s, v in vwap(ts).items():\n        io.print(f\"{s} {v}\")\n",
    "from": "conformance/golden/g2_vwap.yag"
  },
  {
    "name": "Sum types",
    "surface": "yag",
    "source": "type Shape = Circle(r: int) | Rect(w: int, h: int) | Dot()\n\ndef area(s: Shape) -> int:\n    match s:\n        case Circle(r):\n            return 3 * r * r\n        case Rect(w, h):\n            return w * h\n        case Dot():\n            return 0\n\npub def main() uses io:\n    for s in [Circle(r=1), Rect(w=2, h=3), Dot()]:\n        io.print(f\"{area(s)}\")\n",
    "from": "conformance/yag/run/match/sums.yag"
  },
  {
    "name": "par blocks",
    "surface": "yag",
    "source": "def sq(x: int) -> int:\n    return x * x\n\npub def main() uses io:\n    par:\n        a = sq(2)\n        b = sq(3)\n        io.print(\"one\")\n        io.print(\"two\")\n    ys = par [sq(x) for x in range(5)]\n    io.print(f\"{a} {b} {ys}\")\n",
    "from": "conformance/yag/run/concurrency/par_source_order.yag"
  },
  {
    "name": "var parameters",
    "surface": "yag",
    "source": "def bump(var n: int) -> None:\n    n += 1\n\npub def main() uses io:\n    a = 1\n    bump(&a)\n    bump(&a)\n    io.print(f\"{a}\")\n",
    "from": "conformance/yag/run/ownership/inout.yag"
  },
  {
    "name": "Big integers",
    "surface": "yag",
    "source": "pub def main() uses io:\n    x = bigint(9223372036854775807) + bigint(1)\n    io.print(f\"{x}\")\n",
    "from": "conformance/yag/run/numbers/bigint.yag"
  },
  {
    "name": "Hello (Yan)",
    "surface": "yn",
    "source": "main() uses io =\n  io.print(\"hello, world\")\n",
    "from": "conformance/yn/run/hello.yn"
  },
  {
    "name": "Fallbacks (Yan)",
    "surface": "yn",
    "source": "type Bad = { code: int }\n\ncheck(x: int) -> int fail Bad =\n  if x < 0\n    fail Bad(code: x)\n  x\n\nlookup(m: Map[str, int], k: str) -> int =\n  let v = m.get(k) else return -1\n  v\n\nmain() =\n  for x in [3, -2]\n    let v = check(x) else err ->\n      io.print(\"bad \\(err.code)\")\n      continue\n    io.print(\"ok \\(v)\")\n  let m = {\"a\": 1}\n  io.print(\"\\(lookup(m, \"a\")) \\(lookup(m, \"b\"))\")\n",
    "from": "conformance/yn/run/failure/fallback_block.yn"
  },
  {
    "name": "Map defaults (Yan)",
    "surface": "yn",
    "source": "main() =\n  var m: Map[str, int] = {}\n  for w in [\"a\", \"b\", \"a\"]\n    m[w] += 1\n  for (k, v) in m.items()\n    io.print(\"\\(k)=\\(v)\")\n",
    "from": "conformance/yn/run/collections/map_default_insert.yn"
  }
];
