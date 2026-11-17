// Tokenization snapshots and theme coverage for the TextMate grammars.
// Update snapshots after an intended grammar change: npm run snapshots
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const vsctm = require("vscode-textmate");
const oniguruma = require("vscode-oniguruma");

const here = dirname(fileURLToPath(import.meta.url));
const tm = join(here, "../../textmate");
const snapDir = join(here, "snapshots");
const update = process.env.UPDATE_SNAPSHOTS === "1";

const grammars = {
  "source.yan": { file: "yan.tmLanguage.json", ext: ".yn" },
  "source.yag": { file: "yag.tmLanguage.json", ext: ".yag" },
  "source.yd": { file: "yd.tmLanguage.json", ext: ".yd" },
};

const wasm = readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm")).buffer;
const onig = oniguruma.loadWASM(wasm).then(() => ({
  createOnigScanner: (p) => new oniguruma.OnigScanner(p),
  createOnigString: (s) => new oniguruma.OnigString(s),
}));
const registry = new vsctm.Registry({
  onigLib: onig,
  loadGrammar: async (scope) => {
    const g = grammars[scope];
    return g ? vsctm.parseRawGrammar(readFileSync(join(tm, g.file), "utf8"), g.file) : null;
  },
});

async function tokenize(scope, text) {
  const grammar = await registry.loadGrammar(scope);
  let stack = vsctm.INITIAL;
  const out = [];
  for (const line of text.split("\n")) {
    const r = grammar.tokenizeLine(line, stack);
    out.push({
      line,
      tokens: r.tokens.map((t) => ({
        text: line.slice(t.startIndex, t.endIndex),
        scopes: t.scopes.slice(1),
      })),
    });
    stack = r.ruleStack;
  }
  return out;
}

function render(lines) {
  return lines
    .map(({ line, tokens }) => {
      const body = tokens
        .filter((t) => t.text.trim() !== "")
        .map((t) => `  ${JSON.stringify(t.text)} ${t.scopes.join(" ") || "-"}`);
      return [`${JSON.stringify(line)}`, ...body].join("\n");
    })
    .join("\n") + "\n";
}

const samples = Object.entries(grammars).flatMap(([scope, g]) =>
  readdirSync(join(tm, "samples"))
    .filter((f) => f.endsWith(g.ext))
    .map((f) => ({ scope, f })),
);

for (const { scope, f } of samples) {
  test(`snapshot ${f}`, async () => {
    const lines = await tokenize(scope, readFileSync(join(tm, "samples", f), "utf8"));
    const got = render(lines);
    const path = join(snapDir, `${f}.snap`);
    if (update) {
      mkdirSync(snapDir, { recursive: true });
      writeFileSync(path, got);
      return;
    }
    assert.ok(existsSync(path), `missing snapshot ${path}; run npm run snapshots`);
    assert.equal(got, readFileSync(path, "utf8"));
  });
}

// Every colored scope must be styled by the theme; roles the theme leaves plain are listed here.
const plain = ["meta.", "punctuation", "variable", "constant.other"];
const themes = ["yan-dark", "yan-light"].map((n) =>
  JSON.parse(readFileSync(join(tm, `../themes/${n}.color-theme.json`), "utf8")),
);

for (const theme of themes) {
  test(`theme ${theme.name} styles every scope in the samples`, async () => {
    const selectors = theme.tokenColors.flatMap((r) => r.scope.split(",").map((s) => s.trim()));
    const missing = new Set();
    for (const { scope, f } of samples) {
      const lines = await tokenize(scope, readFileSync(join(tm, "samples", f), "utf8"));
      for (const { tokens } of lines)
        for (const t of tokens) {
          const specific = t.scopes.filter((s) => !s.startsWith("meta."));
          const leaf = specific[specific.length - 1];
          if (!leaf || plain.some((p) => leaf.startsWith(p))) continue;
          if (!selectors.some((sel) => leaf === sel || leaf.startsWith(sel + "."))) missing.add(leaf);
        }
    }
    assert.deepEqual([...missing], []);
  });
}

test("no sample line is tokenized as invalid except the rejected-keyword line", async () => {
  const bad = [];
  for (const { scope, f } of samples) {
    const lines = await tokenize(scope, readFileSync(join(tm, "samples", f), "utf8"));
    for (const { line, tokens } of lines)
      if (tokens.some((t) => t.scopes.some((s) => s.startsWith("invalid")))) bad.push(`${f}: ${line}`);
  }
  assert.deepEqual(bad, ["literals.yag:     try:"]);
});
