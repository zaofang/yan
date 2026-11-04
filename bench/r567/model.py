"""Model adapter for the R567 harness.

As a program it is the `cmd:` adapter of `yan agent`: one JSON line per turn on stdin,
{"system", "messages", "max_tokens"}, and one line on stdout:
{"text", "usage", "model"} or {"error"}.
As a module it gives the harness the same call for the typed-Python arm.

Models (the SPEC argument):
  claude:MODEL        the `claude -p` CLI, billed to the session's Anthropic credits; no key needed
  anthropic:MODEL     the Messages API with ANTHROPIC_API_KEY; usage is the provider's own count
  compat:URL#MODEL    an OpenAI-compatible server (llama.cpp, vLLM). The only vendor that can apply
                      the GBNF grammar in $R567_GRAMMAR, which the harness sets for constrained runs
  oracle:DIR[?wrong]  scripted replies from DIR/NAME.yag and DIR/NAME.py, for dry runs (no network)

usage: python model.py SPEC
"""

import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import urllib.request

Usage = dict[str, int]
SEP = "    # ---\n"  # in an oracle file: the spec part above, the body part below


def transcript(msgs: list[dict]) -> str:
    if len(msgs) == 1:
        return msgs[0]["content"]
    turns = [f"<{m['role']}>\n{m['content']}\n</{m['role']}>" for m in msgs]
    return "Conversation so far:\n\n" + "\n\n".join(turns) + "\n\nReply to the last user turn."


class ClaudeCode:
    """One conversation through the Claude Code CLI. Tools, MCP, settings and CLAUDE.md are off;
    turns after the first resume the CLI session and send only the newest user message."""

    def __init__(self, model: str):
        self.model, self.sid, self.sent = model, None, 0
        self.cwd = tempfile.mkdtemp(prefix="r567-claude-")

    def __call__(self, system: str, msgs: list[dict], max_tokens: int) -> dict:
        if self.sid and len(msgs) == self.sent + 2:
            prompt, resume = msgs[-1]["content"], ["--resume", self.sid]
        else:
            prompt, resume = transcript(msgs), []
        cmd = [
            "claude", "-p", "--model", self.model, "--system-prompt", system, "--tools", "",
            "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands",
            "--output-format", "json", *resume, prompt,
        ]  # fmt: skip
        env = {**os.environ, "CLAUDE_CODE_MAX_OUTPUT_TOKENS": str(max_tokens)}
        p = subprocess.run(
            cmd, cwd=self.cwd, env=env, capture_output=True, text=True, timeout=900, check=False
        )
        if not p.stdout.strip():
            raise RuntimeError(f"claude exited {p.returncode}: {p.stderr.strip()[:300]}")
        r = json.loads(p.stdout)
        if r.get("is_error"):
            raise RuntimeError(r.get("result") or p.stderr)
        self.sid, self.sent = r["session_id"], len(msgs)
        u = r["usage"]
        tin = u["input_tokens"] + u.get("cache_creation_input_tokens", 0)
        tin += u.get("cache_read_input_tokens", 0)
        think = (u.get("output_tokens_details") or {}).get("thinking_tokens", 0)
        usage = {"in": tin, "out": u["output_tokens"], "reasoning": think}
        return {"text": r["result"], "usage": usage, "model": self.model}


def post(url: str, body: dict, headers: dict[str, str]) -> dict:
    req = urllib.request.Request(
        url, json.dumps(body).encode(), {"content-type": "application/json", **headers}
    )
    with urllib.request.urlopen(req, timeout=900) as r:
        return json.load(r)


class Anthropic:
    def __init__(self, model: str):
        self.model, self.key = model, os.environ["ANTHROPIC_API_KEY"]

    def __call__(self, system: str, msgs: list[dict], max_tokens: int) -> dict:
        r = post(
            "https://api.anthropic.com/v1/messages",
            {"model": self.model, "max_tokens": max_tokens, "system": system, "messages": msgs},
            {"x-api-key": self.key, "anthropic-version": "2023-06-01"},
        )
        text = "".join(b["text"] for b in r["content"] if b["type"] == "text")
        u = r["usage"]
        tin = u["input_tokens"] + u.get("cache_creation_input_tokens", 0)
        tin += u.get("cache_read_input_tokens", 0)
        return {"text": text, "usage": {"in": tin, "out": u["output_tokens"], "reasoning": 0},
                "model": r.get("model", self.model)}  # fmt: skip


class Compat:
    """Chat completions on a local server. The grammar, when set, constrains the whole reply."""

    def __init__(self, spec: str, grammar_file: str | None = None):
        self.url, _, self.model = spec.partition("#")
        grammar_file = grammar_file or os.environ.get("R567_GRAMMAR")
        self.grammar = pathlib.Path(grammar_file).read_text() if grammar_file else None

    def __call__(self, system: str, msgs: list[dict], max_tokens: int) -> dict:
        body: dict = {
            "model": self.model, "max_tokens": max_tokens, "temperature": 0,
            "messages": [{"role": "system", "content": system}, *msgs],
        }  # fmt: skip
        if self.grammar:
            body["grammar"] = self.grammar
        r = post(self.url.rstrip("/") + "/v1/chat/completions", body, {})
        u = r["usage"]
        reasoning = (u.get("completion_tokens_details") or {}).get("reasoning_tokens", 0)
        return {
            "text": r["choices"][0]["message"]["content"] or "",
            "usage": {
                "in": u["prompt_tokens"],
                "out": u["completion_tokens"],
                "reasoning": reasoning,
            },
            "model": self.model,
        }


class Oracle:
    """Replays a stored solution. `wrong` fails every body with an off-by-one result."""

    def __init__(self, spec: str):
        d, _, flag = spec.partition("?")
        self.dir, self.wrong = pathlib.Path(d), flag == "wrong"

    def __call__(self, system: str, msgs: list[dict], max_tokens: int) -> dict:
        first = msgs[0]["content"]
        name = re.search(r"Function: (\w+)\(", first)
        if name is None:
            raise RuntimeError("no `Function:` line in the task")
        py = system.startswith("You write typed Python")
        path = self.dir / f"{name.group(1)}.{'py' if py else 'yag'}"
        if not path.exists():
            raise RuntimeError(f"no oracle file {path}")
        parts = path.read_text().split(SEP)  # yag: spec, body. py: def and docstring, body, tests
        spec_phase = not any("Phase 2" in m["content"] for m in msgs)
        body = "    ...\n" if spec_phase else parts[1]
        if self.wrong and not spec_phase:
            body = re.sub(r"return (.+)", r"return \1 + 1", body, count=1)
        fence = "python" if py else "yag"
        reply = parts[0] + body + ("\n\n" + parts[2] if py else "")
        words = len(reply.split())
        usage = {"in": len(system.split()) + sum(len(m["content"].split()) for m in msgs),
                 "out": words, "reasoning": 0}  # fmt: skip
        return {"text": f"```{fence}\n{reply}```", "usage": usage, "model": "oracle"}


def make(spec: str, grammar_file: str | None = None):
    vendor, _, rest = spec.partition(":")
    if vendor == "compat":
        return Compat(rest, grammar_file)
    return {"claude": ClaudeCode, "anthropic": Anthropic, "oracle": Oracle}[vendor](rest)


def main() -> None:
    chat = make(sys.argv[1])
    for line in sys.stdin:
        req = json.loads(line)
        try:
            out = chat(req["system"], req["messages"], req["max_tokens"])
        except Exception as e:  # reported to the loop, which fails the run
            out = {"error": repr(e)}
        print(json.dumps(out), flush=True)


if __name__ == "__main__":
    main()
