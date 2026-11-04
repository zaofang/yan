"""Count lexer tokens of task T3 per proposal in the study, plus Stan and PyMC baselines."""

import re
import sys
from pathlib import Path

HERE = Path(__file__).parent


def blocks(s: str) -> list[str]:
    return re.findall(r"```\n(.*?)```", s, re.S)


def toks(s: str) -> int:
    lines = [line.split("--")[0] for line in s.splitlines() if "io.print" not in line]
    return len(re.findall(r"\d+(?:\.\d+)?|\w+|[^\w\s]", "\n".join(lines)))


doc = Path(sys.argv[1]).read_text()
secs = re.split(r"\n### ", doc)
for name in ["P1", "P2", "P3", "P4", "P5"]:
    sec = next(x for x in secs if x.startswith(name))
    print(name, toks(blocks(sec)[2]))
print("Mix", toks(blocks(doc.split("### 6.4")[1])[2]))
for f in ["stan.txt", "pymc.txt"]:
    print(f, toks((HERE / f).read_text()))
