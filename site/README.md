# Website

Static site: home (`index.md`), tour (`tour/`), playground (`../playground/`), status (generated
from CI data, below) and blog (`blog/YYYY-MM-DD-slug.md`, with an Atom feed). Brand files in
`brand/` are copied unchanged from the brand kit: `brand/icons/` holds the UI icons (24 px grid, `currentColor` stroke) and `brand/file-doc-*.svg` the document-shape file icons.

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

The status page renders `status.json` from `tools/status.py`, which CI's `status` job builds on
main pushes and the nightly run (artifact `status`): conformance by milestone, the differential
run, spec debt and bench figures, each stamped with commit and date. Pass it with
`build --status status.json`; without it the page says it carries no CI data. `site.yml` deploys
after each green `ci` run on main and passes that run's artifact.

A `CNAME` file here, once the domain exists (#389), is copied to the site root.
