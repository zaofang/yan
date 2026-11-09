# Website

Static site: home (`index.md`), tour (`tour/`), playground (`../playground/`), status (generated
from the repository) and blog (`blog/YYYY-MM-DD-slug.md`, with an Atom feed). Brand files in
`brand/` are copied unchanged from the brand kit.

```sh
./playground/build.sh                                  # playground/yan.wasm
uv run python -m tools.site build _site                # --no-wasm to skip the playground binary
uv run python -m tools.site check --yan target/release/yan
python3 -m http.server -d _site 8000
```

Pages are a Markdown subset (headings, paragraphs, lists, tables, fences, raw HTML blocks). A
`yag`, `yn` or `yir` fence is a program with an "Open in playground" link; `check` runs it on the
interpreter and native backends and compares stdout with the `output` fence after it. Fence
attributes: `expect=E301` (must be rejected with that code), `exit=101`, `check` (check only, no run).

A `CNAME` file here, once the domain exists (#389), is copied to the site root.
