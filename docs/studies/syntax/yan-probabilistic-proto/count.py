import os, re, sys
doc = open(sys.argv[1]).read()
def blocks(s): return re.findall(r"```\n(.*?)```", s, re.S)
def toks(s):
    s = "\n".join(l.split("--")[0] for l in s.splitlines() if "io.print" not in l)
    return len(re.findall(r"\d+(?:\.\d+)?|\w+|[^\w\s]", s))
secs = re.split(r"\n### ", doc)
for name in ["P1", "P2", "P3", "P4", "P5"]:
    sec = next(x for x in secs if x.startswith(name))
    print(name, toks(blocks(sec)[2]))
mix = doc.split("### 6.4")[1]
print("Mix", toks(blocks(mix)[2]))
for f in ["stan.txt", "pymc.txt"]: print(f, toks(open(os.path.join(os.path.dirname(__file__), f)).read()))
