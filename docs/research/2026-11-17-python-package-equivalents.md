# Python's popular packages: which domains Yan should replicate, and in what shape

Study and proposal, 2026-11-17, at main `72fc820`. Docs only: no spec rows, no conformance edits, no
diagnostic codes. Items are labelled D1, D2 and so on; none carries a row number. Everything here is
non-normative until an RFC and accepted rows say otherwise. Spec is cited by section number.

Brief (lead, 2026-11-17): take the most popular Python packages, grouped by domain with competitors counted
as one set; say whether Yan can and should replicate each domain; sketch the Yan equivalent (capabilities,
API, improvements, deficiencies); decide whether the API is Python-shaped or improved, with the polars and
pandas lesson in mind; end with a prioritized shortlist.

Evidence tags: **[V]** verified by fetch or by reading the repository on the date given; **[I]** inferred
from verified facts; **[B]** background knowledge about a project's history or design, not re-checked here.

## 1. Answer

1. Download ranking is mostly plumbing. Of the top 20 PyPI projects by 30-day downloads, 11 are
   transitive infrastructure (packaging, typing-extensions, urllib3, setuptools, certifi, idna,
   charset-normalizer, six, cffi, pycparser, pydantic-core) that nobody chooses directly [V, section 2]. The packages people choose sit below them:
   numpy, pandas, pydantic, pytest, requests, httpx, click, fastapi, sqlalchemy, rich.
2. Most domains need no new package. Fourteen of 26 domains below are already std, a language feature, the
   toolchain or an existing first-party package in the spec (validation, dates, logging, CLI parsing,
   async, mocks, packaging, crypto, property testing, arrays, dataframes, formats). Yan's capabilities, `derive(Wire)` and `with` handlers make whole Python
   library categories (mock, freezegun, responses, python-dotenv, dateutil, retry-with-sleep fakes)
   unnecessary [I].
3. The gaps worth building are the layer people call on top of that core: an ergonomic HTTP client, the
   dataframe front end, statistics and plots, a typed web router, an LLM client, terminal output,
   typed templates and spreadsheets (section 6).
4. Do not make the library API Python-shaped. Make the canonical API Yan-native and put Python's shape
   in the lenient reader and `yan import --python`, the place the spec already uses for Python habits
   (14.4, 17.8). Mirror a Python name only when the behavior on every input a correct Python program
   gives it is identical; otherwise take a different name and an alias note (section 5).

## 2. What is popular (verified data)

Source: top-pypi-packages (ClickHouse-backed), 30-day downloads, last update 2026-11-01 06:41 UTC, fetched
2026-11-17 [V]. Libraries.io dependents and stars, fetched 2026-11-17 [V]. Libraries.io had rate limits
and mis-mapped some repositories (duckdb 207 stars, beautifulsoup4 and numba 0 stars), so stars are
indicative only and the dependents count is the better direct-choice signal [I]. Downloads overcount: CI
reinstalls, mirrors and transitive pulls inflate every row, most for transitive projects [I]. GitHub
stars could not be fetched directly (API not enabled for this session).

Rank is among the top 15,000 projects. `dep` is libraries.io dependent packages, thousands.

| Domain set | Members, rank and 30-day downloads | Direct-use signal |
| --- | --- | --- |
| Cloud SDK | boto3 #1 2672M, botocore #22 998M, s3fs #113 302M, fsspec #47 609M | not fetched (rate limit) |
| Numeric arrays | numpy #21 1026M, scipy #95 365M, jax #1153 12M, torch #397 63M, numba #387 66M | numpy 140k dep, scipy 51k |
| Dataframes | pandas #38 669M, pyarrow #78 429M, duckdb #366 71M, polars #396 64M, dask #642 31M | pandas 90k dep, 50k stars; polars 6k dep, 40k stars |
| Validation, records | pydantic #18 1071M (pydantic-core #19 1046M), attrs #29 844M, pydantic-settings #74 464M | pydantic 77k dep, attrs 7k |
| HTTP clients | requests #8 1442M, urllib3 #6 1563M, httpx #31 753M, aiohttp #58 545M, websockets #64 507M | requests 112k dep, httpx 50k, aiohttp 23k |
| Web servers, frameworks | starlette #40 654M, uvicorn #44 617M, fastapi #65 506M, flask #162 195M, gunicorn #263 118M, django #466 49M | fastapi 29k dep, 105k stars; flask 12k dep, 75k stars; django 16k dep |
| Testing | pytest #25 926M, pytest-cov #139 242M, coverage #108 329M, pytest-asyncio #126 271M, hypothesis #384 66M, faker #353 75M, freezegun #499 45M, responses #391 64M, mock #809 21M | pytest 161k dep, hypothesis 6k |
| CLI and terminal | click #20 1028M, rich #57 546M, tqdm #69 481M, typer #115 293M, colorama #118 289M, termcolor #336 79M | click 50k dep, rich 29k, tqdm 38k, typer 26k |
| Databases | sqlalchemy #86 395M, psycopg2-binary #146 232M, psycopg #170 188M, redis #140 237M, alembic #150 219M, pymongo #362 72M, pymysql #331 82M | sqlalchemy 16k dep |
| Serialization, formats | pyyaml #12 1150M, orjson #152 218M, msgpack #145 233M, protobuf #34 726M, toml #230 145M, tomli #125 274M, ujson #656 30M | pyyaml 35k dep |
| Templates, markup | markupsafe #30 816M, jinja2 #37 676M, markdown #265 117M, beautifulsoup4 #110 316M, lxml #81 413M | jinja2 16k dep, bs4 19k |
| Dates and zones | python-dateutil #15 1126M, pytz #67 495M, tzdata #51 575M, arrow #370 71M, pendulum #422 57M | arrow 1.5k dep, pendulum 1.2k |
| Crypto, SSH | cryptography #9 1317M, paramiko #271 114M | not fetched |
| Images, office files | pillow #43 643M, openpyxl #103 338M, xlsxwriter #206 160M, xlrd #339 78M | pillow 28k dep |
| ML | scikit-learn #155 211M, torch #397 63M, tensorflow #1040 14M, transformers #269 115M, joblib #169 188M | scikit-learn 31k dep, 68k stars |
| LLM SDKs | openai #97 359M, anthropic #164 194M | not fetched |
| Plotting | matplotlib #172 186M, plotly #413 59M, seaborn #635 32M, altair #519 43M, bokeh #1610 7M | matplotlib 59k dep, plotly 10k |
| Stats, symbolic, graphs | statsmodels #570 37M, sympy #218 153M, networkx #138 244M | sympy 5k dep |
| Dev tooling | ruff #85 396M, black #252 126M, mypy #160 199M, uv #215 156M, pip #62 517M, isort #296 100M, flake8 #514 44M, pylint #534 41M, poetry #500 45M, virtualenv #134 254M, pre-commit #211 157M, tox #908 17M, nox #1969 5M | ruff 66k dep, 51k stars; uv 92k stars; mypy 54k dep |
| Concurrency, jobs | anyio #23 964M, trio #530 42M, celery #452 51M, ray #978 16M | not fetched |
| Logging, observability | structlog #273 114M, loguru #337 79M | loguru 11k dep |
| Config, retry, caching | python-dotenv #35 720M, tenacity #82 407M, cachetools #127 269M, more-itertools #133 256M, toolz #377 70M | not fetched |
| Browser and scraping automation | playwright #253 125M, selenium #634 32M | not fetched |
| System | psutil #101 352M, watchdog #289 105M, docker #228 147M, kubernetes #322 86M, pexpect #243 134M | not fetched |
| Packaging plumbing | packaging #2 1805M, typing-extensions #3 1704M, urllib3 #6, setuptools #17 1090M, certifi #4, idna #5, charset-normalizer #7, six #16, cffi #10, pycparser #13, markupsafe #30, h11 #28, httpcore #32 | not chosen directly |

Notes that change the picture [I]:

- The adoption lead of a challenger is real but small in absolute terms: polars is 64M against pandas 669M
  (about 10 percent) after four years of "better pandas". httpx 753M is the exception, helped by FastAPI
  and the OpenAI and Anthropic SDKs depending on it. Polars-style quality does not by itself displace the
  incumbent; ergonomics must also meet migration.
- Rust-core Python packages already dominate their niches: pydantic-core 1046M, orjson 218M, ruff 396M,
  uv 156M, polars 64M. The market has accepted native compiled cores behind a Python-shaped surface.
- Pydantic, attrs, fastapi and typer are all about type-driven data shapes, and all are in the top 120. Yan has types and `derive` in the language, so this class shrinks to language features.

## 3. Per-domain verdict

Placement uses the spec's own test (17.1): a module is std only if it is shared vocabulary, a capability
handler or needed by the evaluation; its target is frozen; and one implementation beats many. Otherwise it
is a first-party `@yan/*` package. Anything else is community. "Language" means a language or toolchain
feature, not a library. "Exists" means the spec already names it, with the section.

| # | Domain set | Verdict | Yan home | Why |
| --- | --- | --- | --- | --- |
| 1 | Packaging plumbing | Not at all | toolchain (19, RFC 0084) | the manager, lock and registry replace pip, setuptools, packaging, virtualenv; the module system replaces six and typing-extensions |
| 2 | Dev tooling (ruff, black, mypy, uv) | Not at all | toolchain (`yan check`, `fmt`, `test`, `pkg`) | one binary is the Yan design; Python needed many tools because it lacked one |
| 3 | Numeric arrays (numpy, jax, numba) | Exists | `array`, `Array`, `@yan/blas`, `grad`, kernels (11.2 to 11.5) | see scientific-computing study 2026-11-05; compiler does numba's job |
| 4 | Dataframes (pandas, polars, pyarrow, duckdb) | Exists, finish | `Table` in std, `@yan/frame`, `std.arrow`, `@yan/parquet` (11.8, 11.9) | the highest-value domain that is also the most designed; the remaining work is the lazy front end and aliases |
| 5 | Validation, records (pydantic, attrs) | Not at all | `class`, `derive(Wire)`, `pre`/`post`, `std.data` (17.3) | pydantic exists because Python lacks typed boundaries; Wire is the same job in the language |
| 6 | Settings, dotenv | Not at all | `derive(Config)` in `env` (17.3) | layering env, file and flags, secrets typed `Secret[str]` |
| 7 | Dates and zones (dateutil, pytz, arrow, pendulum) | Not at all | `std.time`, bundled tzdb (17.5) | one strict implementation beats four; the spec already adopts Jiff-class semantics |
| 8 | Logging (structlog, loguru) | Not at all | `log` capability (7, 17.3) | structured, spans, handler-pluggable; nothing left to add |
| 9 | CLI parsing (click, typer) | Not at all | `derive(Cli)` in `std.cli` (17.3) | typer's idea already is the std design |
| 10 | Crypto, TLS (cryptography, certifi) | Not at all | `std.crypto`, `std.tls` | std by the one-implementation test; certifi is a bundled root store |
| 11 | Testing core (pytest, mock, freezegun, responses, faker) | Not at all | `test` module, `fake_*` handlers, `virtual` time, inline `ex` cases (17.3, 24) | capability handlers are mocks; `with fixed_time` replaces freezegun; `mem_net` replaces responses |
| 12 | Property testing (hypothesis) | Not at all | contracts with shrinking, `explore(bound)` (24) | already in the contract tier |
| 13 | Async and parallelism (anyio, trio, asyncio, joblib) | Not at all | `par`, actors, `chan`, `sched` handlers (9, 17.3) | a language feature, which is Yan's thesis |
| 14 | Jobs and queues (celery) | Language, then package | `@durable`, actors; broker adapters later | durable execution is built in; queue brokers are protocol clients, community |
| 15 | HTTP client (requests, httpx, aiohttp, urllib3, websockets) | Std completion | `std.http`, `std.tls`, `@yan/websocket`, `@yan/http3` (17.3) | std has a sans-I/O core and `get_text`; the ergonomic client is the missing surface |
| 16 | Web servers and frameworks (fastapi, flask, starlette, uvicorn, django) | First-party package | new `@yan/web` over `std.http` server | huge demand; Yan's type and capability model makes the typed router nearly free; a full framework (ORM, admin, auth) is community |
| 17 | Databases (sqlalchemy, psycopg, redis, pymongo) | Partly exists | `@yan/sqlite`, `@yan/postgres`, `@yan/sql`, `@yan/adbc` (17.1) | no ORM: a typed query builder over records; Redis and Mongo clients are community protocol libraries |
| 18 | Serialization and formats (pyyaml, orjson, msgpack, protobuf, toml) | Exists | `json`, `std.csv`, `std.toml`, `@yan/yaml`, `@yan/msgpack`, `@yan/cbor`, `@yan/protobuf` | one `Wire` derive for all codecs removes the per-library API zoo |
| 19 | Templates and markup (jinja2, markupsafe, markdown, bs4, lxml) | First-party package | new `@yan/template`; `@yan/markdown`, `@yan/html` exist | typed templates checked at stage 0 and escaping by type are a clear win; scraping is `@yan/html` |
| 20 | Terminal UI (rich, tqdm, colorama, termcolor) | First-party package | new `@yan/term` | every CLI and agent tool prints; styled tables, progress and trace output over `io` |
| 21 | Cloud SDK (boto3, fsspec, s3fs, docker, kubernetes) | Generator, then packages | `@yan/object_store` exists; service clients generated | boto3 is #1 by downloads and is generated from service models; Yan can generate typed clients the same way |
| 22 | LLM SDKs (openai, anthropic, transformers) | First-party package | new `@yan/llm` | an agent-oriented language needs a first-class typed client with record and replay |
| 23 | Statistics, optimization, ODE, FFT (scipy, statsmodels) | First-party packages | `std.stats`, `@yan/stats`, `@yan/linalg`, `@yan/optim`, `@yan/ode`, `@yan/fft` | decided in the 2026-11-05 study; keep its order |
| 24 | Plotting (matplotlib, plotly, seaborn, altair) | First-party package | `@yan/plot`, charts as `Wire` records, Vega-Lite export | decided in the 2026-11-05 study |
| 25 | ML (scikit-learn, torch, transformers) | Community above `@yan/learn`; bridge below | `py` bridge now (17.8) | deep learning frameworks are out of reach for one maintainer; classical ML later |
| 26 | Images, office files, symbolic, graphs, SSH, browser automation, system info | Community unless a demand signal appears | `@yan/xlsx` is the one exception worth building | pillow, sympy, networkx, paramiko, playwright, psutil are each real work with weak capability leverage; xlsx combines three high-download packages and is pure parsing |

Count: verdict "Not at all" or "Exists" is 14 of 26 rows [I].

## 4. What the Yan equivalents look like

Each entry: capabilities, API sketch, improvements, deficiencies. Sketches are for design discussion, not
syntax commitments, and use the Yan Agent form in `examples/`. Only the packages worth building get a sketch;
domains already in the spec point at their section.

### 4.1 HTTP client (std completion; requests, httpx)

Capabilities: `net` (attenuated with `net.only(connect=...)`, 17.6), `time` for deadlines and backoff, `rand`
for jitter, `secret` values for auth headers.

```
def fetch_user(c: http.Client, id: int) -> User fail HttpError uses net, time:
    r = c.get(f"/users/{id}", timeout=5s)?
    return r.json[User]()?          # Wire-typed decode, same derive as json.parse

def main() -> None uses net, time, rand:
    c = http.client(base_url="https://api.example.com", auth=bearer(env.secret("TOKEN")),
                    retry=http.retry(max=3, on=[503], backoff=exp(100ms)))
    print(fetch_user(c, 7)?)
```

Improvements [I]: no global state (`requests.get` uses a hidden session and no default timeout; here a
client value is always passed and a timeout is mandatory by default); retries, redirects and cookies are
explicit policy values; the same code runs under `mem_net` with a fake server and virtual time, so retry and
timeout tests are instant and deterministic; the attenuated `net` capability bounds which hosts a library
may reach, which supply-chain-hardens every package that uses the client; HTTP/2 and 3 are `Transport`
choices behind one API (17.3).

Deficiencies: no connection-pool tuning surface on day one; proxy and mTLS corner cases lag urllib3; server
push and exotic auth schemes are community.

### 4.2 Dataframes (pandas, polars): finish what exists

Capabilities: none for in-memory work; `fs` for file I/O only (11.9). The design exists: `Table[R]` eager,
`LazyFrame[R]` with typed expressions, schema-generated record types, deterministic results.

```
type Sale = ...                       # record generated by `yan schema gen sales.parquet`
q = frame.scan_parquet[Sale]("sales.parquet")
      .filter(.amount > 0)
      .group_by(.region)
      .agg(total=sum(.amount), n=count())
      .sort(.total, desc=True)
rows = q.collect()?
```

The open work is lenient-reader coverage for `df.groupby("a").agg(...)`, `df["x"]`, `df.sort_values`,
`pd.read_csv` and the index (study 2026-11-05, D12), plus a pass@1 evaluation (section 5.3). Deficiency:
no index, no `inplace` and no mutable chained assignment, which is a deliberate break that costs pandas
migrants a rewrite; the alias layer handles the common 80 percent and fails loud on the rest.

### 4.3 Typed web router (`@yan/web`; fastapi, flask, starlette)

Capabilities: `net` to listen; each route handler declares its own `uses`, so a handler that only reads the
database cannot open a socket; the router holds no ambient state.

```
@route("GET", "/users/{id}")
def get_user(id: int, db: Db) -> User fail NotFound uses sql:
    return db.find[User](id) else fail NotFound(id)

app = web.app([get_user, create_user]).openapi(title="users")
```

Improvements [I]: path, query and body decode through `Wire` so validation, OpenAPI and client generation
come from one record type, which is FastAPI's best idea moved into the compiler; handler `uses` is
reported by `yan audit`, which FastAPI cannot offer; actor-per-session state instead of module globals;
deterministic tests via `mem_net`. Deficiencies: no ORM, admin, auth or form ecosystem; middleware
ecosystem starts empty; WebSocket and SSE depend on `@yan/websocket` and the evented scheduler (17.3
preview). Scope guard: a router plus OpenAPI, not Django.

### 4.4 LLM client (`@yan/llm`; openai, anthropic)

Capabilities: `net` attenuated to the provider host, `secret` for keys, `time`, `log`.

```
def classify(m: llm.Model, text: str) -> Label fail LlmError uses net, secret:
    r = m.complete[Label](system="Label the text.", user=text)?   # structured output decoded via Wire
    return r.value
```

Improvements [I]: structured output and tool calls are typed with `Wire`, so a tool is a function and its
schema is derived, not hand-written; `test.replay(trace)` makes model calls recordable and replayable,
which gives deterministic tests for agents; provider host allow-listing is a capability, not a
convention; the key is `Secret[str]` and cannot reach `log`. Deficiencies: provider APIs change monthly,
so this is a first-party package with fast semver and no std promise; streaming needs the evented
scheduler to be pleasant; provider-specific features lag.

### 4.5 Terminal output (`@yan/term`; rich, tqdm, colorama)

Capabilities: `io` only. Width and color detection come from the `io` handler (`env` for `NO_COLOR`), so a
test handler pins them.

```
term.table(rows, columns=["name", "n"]).print()
for item in term.progress(items, label="index"):
    ...
```

Improvements [I]: renderables are values with a deterministic plain-text fallback (snapshot tests work);
progress output goes through `io`, so it is captured in logs the same way in CI and on a tty; one
styling vocabulary replaces colorama and termcolor. Deficiencies: no full TUI toolkit (textual-class); no
Windows legacy console quirks.

### 4.6 Typed templates (`@yan/template`; jinja2, markupsafe)

Capabilities: none; rendering is pure. Templates are parsed at stage 0 as a comptime string DSL (10.4),
the same mechanism `@yan/rel` uses.

```
page = template.parse[Page]("""<h1>{{ title }}</h1>{% for x in items %}<li>{{ x.name }}</li>{% end %}""")
html = page.render(Page(title="hi", items=xs))      # Html type; text is escaped by construction
```

Improvements [I]: a missing variable or wrong field is a compile error, not a runtime `UndefinedError`;
escaping is part of the type (`Html` versus `str`), so markupsafe's discipline is enforced; no sandbox
escape surface because templates cannot call arbitrary functions, only fields. Deficiencies: less
dynamic than Jinja (no runtime template loading with user-defined filters without an explicit `Dyn`
path); designer-facing template ecosystems stay in Jinja.

### 4.7 Spreadsheets (`@yan/xlsx`; openpyxl, xlsxwriter, xlrd)

Capabilities: `fs` for paths, or none over a `Reader` and `Writer`. `read[R](path)` returns `Table[R]` for a
generated schema; `write(table)` produces a deterministic archive (fixed timestamps, stable ordering).
Improvements [I]: the three Python packages split read, write and legacy formats; one typed package
with reproducible output is a real gap. Deficiencies: charts, pivot tables and macros are out of scope;
legacy `.xls` is read-only if at all.

### 4.8 Cloud SDKs (generator; boto3)

Capabilities: `net.only(connect="*.amazonaws.com")`, `secret`, `time`. Proposal: a generator that turns a
provider's published service model into a typed client at stage 0 (`yan schema gen` already generates
record types from Parquet, 11.9), so the work is the generator and a signing core, not hand-written
clients. Improvement [I]: least-privilege is enforceable because the capability names the hosts.
Deficiency: AWS is hundreds of services; ship S3, STS and one queue as proof and leave the rest generated
on demand. `@yan/object_store` already covers the data path.

### 4.9 What `std` and the spec already deliver, per domain [I]

Dates, validation, settings, logging, CLI, crypto, testing, async: improvements are the same four themes:
(a) typed instead of dynamic, (b) capability-scoped instead of ambient, (c) deterministic instead of
environment-dependent, (d) no hidden global state (the logging root logger, the requests session, pytz
globals, `random` module state). The shared deficiency is breadth: Python has an answer to nearly any
niche, and Yan's `py` bridge (17.8) is the honest answer until native packages exist. Bridge calls cost
the `py` capability, which the spec treats as unconfined authority (7), so a bridged package forfeits
the capability story that motivates replicating it.

## 5. Python-shaped API or an improved one

Recommendation: **improve the API; keep Python's shape in the alias layer.** The long-term correct rule
is below, then the case study behind it.

### 5.1 Case study: what each success actually changed [B unless noted]

| Pair | Kept from the incumbent | Changed | Lesson |
| --- | --- | --- | --- |
| polars vs pandas | verbs and nouns (`filter`, `select`, `group_by`, `join`, `sort`), method chaining, CSV and Arrow I/O | expressions as first-class values (`col("a") * 2`), lazy plans with an optimizer, no row index, immutable frames, one null story, strict dtypes, no `inplace` | the semantics changed, the vocabulary mostly did not; people could read polars on day one [I] |
| httpx vs requests | `get`, `post`, `.json()`, `.status_code`, `Session` shape | explicit `Client`, timeouts on by default, sync and async, HTTP/2, strict URL handling | "same names, safer defaults" won a 753M-download position (section 2 [V]) |
| ruff, uv vs flake8, black, pip | rule codes, config keys, `pip`-shaped commands | a single static binary, speed, one tool for many | compatibility at the surface users touch drove adoption; the engine was free to change entirely |
| pydantic v2 vs v1 | model-class declaration style | rewrite with a Rust core, renamed methods (`parse_obj` to `model_validate`) | the break cost the ecosystem years of dual support; fix the API before others depend on it |
| rich vs print and logging | familiar call shapes | renderables as values, one styling model | output as composable values scales; ad hoc string building does not |
| jax.numpy vs numpy | the function names, broadcasting rules | immutable arrays, `x.at[i].set(v)`, functional transforms | mirror names, change the mutation model, and say loudly where it differs |
| typer vs click | decorators, help text conventions | function signatures and type hints are the schema | when the language can carry the schema, drop the DSL |

Polars did not "avoid pandas' interface" by rejecting its names; it kept the SQL and dplyr verbs and
replaced the index, the mutation model and the eager execution model. Where polars is awkward (expression
contexts, `with_columns` versus `select` semantics, string-named columns that fail at run time), it is
because Python cannot type a column reference. Yan can: `.amount` is a field access checked against `R`,
which is the improvement polars could not make [I].

### 5.2 General rule: when to mirror Python and when to innovate

Applied per name, not per package:

1. **Mirror the vocabulary when behavior is identical on every input a correct Python program gives it.**
   `join`, `split`, `strip`, `sorted`, `sum`, `get`, `items` qualify. Models and migrants already know
   them, and the fidelity audit (2026-11-04) shows the pass@1 gap is library and typing friction, not
   syntax [V, audit in project files].
2. **Do not mirror a name whose Python behavior Yan cannot match.** `df.sort_values(inplace=True)`,
   `requests.get(url)` without a timeout, `d[k]` raising `KeyError`, `dt.now()` reading ambient time. Pick
   the honest name (`sort`, `get(...)` with a timeout, `d.get(k)` returning `T | None`, `time.now()` under
   `uses time`). A same-name different-behavior pair silently produces wrong programs and is the worst
   outcome for a language that promises determinism and checked failure.
3. **Reach the Python spelling through the lenient reader, never the library.** The canonical API stays
   one, small and Yan-shaped; the foreign-name table (14.4, and D12 of the 2026-11-05 study) rewrites
   `np.mean(x, axis=0)`, `pd.read_csv`, `df.groupby`, `requests.get`, `plt.plot` to the canonical call and
   lists each rewrite as a note on standard error. This follows the standing policy that no Python name
   ends in a bare generic error: a real function or an alias with a note.
4. **Take the structure from Yan's own idioms.** Failure is `fail E` and `?`, effects are `uses`, schemas
   are records with `derive(Wire)`, options are `T | None`, configuration is values. Never copy kwargs
   soups (`pd.read_csv` has 50 parameters), module-level state, magic strings for column names, or
   exceptions as control flow.
5. **A package ships only after a model-evaluation check.** Measure first-try pass@1 on a task set
   written in Python's idiom, with and without the alias layer, in the harness behind the 2026-11-07
   three-seed repeat. If the canonical API loses to a Python-shaped one by more than noise, the
   fix is an alias or a diagnostic text, not a shape change, unless the evidence says otherwise (law 11).

### 5.3 Why this rule is the correct long-term option

- The two goals are in different layers. Familiarity matters at write time and at migration time; API
  soundness matters for the life of the package. Yan already separates the two: one canonical form and a
  lenient reader (spec 14). Putting Python's shape in the reader keeps the canonical API free to be right
  and keeps frozen names small [I].
- Mirroring a bad interface freezes it. Spec 20.5 allows an incompatible redesign only as a `.v2` module
  that is deprecated and never removed in 1.x. A Python-shaped mistake would be permanent.
- Familiarity can be bought cheaply and tested. The alias table is data with a probe (the 2026-11-05 study
  warned it decays without an owner), while a library shape is code every user must carry.
- The risk to manage is the alias layer's coverage, not the API. If a package's first-try rate is worse
  than the Python original, the answer is more N-code aliases and better diagnostic text.

## 6. Prioritized shortlist: first 8, then two to watch

Ordering criteria [I]: demand (section 2), fit to Yan's capability and type strengths, design already
done, cost to build and maintain by one developer with agents, and dependency order (what unblocks other
rows). Decisions already taken in the 2026-11-05 study are kept, not re-litigated.

| Rank | Item | Kind | Why now | Depends on |
| --- | --- | --- | --- | --- |
| 1 | `std.http` client surface (Client, timeouts, retry policy, Wire decode) | std completion | demand is the highest of any domain Yan must serve (requests 1442M, httpx 753M, 112k dependents); unblocks web, llm, cloud | existing `std.http`, `std.tls` |
| 2 | Dataframe front end and aliases (`@yan/frame` lazy API, `df.` and `pd.` foreign-name table) | finish existing | pandas 669M and polars 64M; the whole design is accepted; the alias layer decides migration | `Table`, `std.arrow`, schema gen |
| 3 | `std.stats` then `@yan/stats`, `@yan/plot` | package | already the lead-approved first pair; matplotlib 186M and seaborn, scipy.stats | arrays, `Chart` as Wire record |
| 4 | `@yan/web` typed router and OpenAPI | package | fastapi, flask, starlette, uvicorn together exceed 1.9B downloads; Wire plus per-route `uses` is the largest differentiator | item 1, server half of `std.http` |
| 5 | `@yan/llm` | package | openai and anthropic SDKs are 550M; record and replay gives deterministic agent tests no Python SDK has | item 1 |
| 6 | `@yan/term` | package | rich 546M and tqdm 481M; every CLI and agent tool prints; small and low-risk | `io` |
| 7 | `@yan/template` | package | jinja2 and markupsafe 1.5B between them; typed, escaped-by-type templates are a visible win | stage-0 DSL mechanism (10.4) |
| 8 | `@yan/xlsx` | package | openpyxl, xlsxwriter, xlrd 576M combined; pure parsing, no capability complexity | `std.arrow`, `fs` |
| watch | Cloud SDK generator (S3, STS first) | generator | boto3 is #1 at 2672M; worth building after item 1 and `@yan/object_store` prove the signing core | item 1 |
| watch | `@yan/graph` (networkx 244M) | package | large direct-use signal, easy to implement as a typed `Table`-backed graph; wait for a user | `Table`, `Pool` |

Not on the list on purpose: ORM, pillow, sympy, deep-learning frameworks, paramiko, browser automation.
Each needs sustained maintenance that outweighs its capability leverage; the `py` bridge covers them
until a user funds one. The property-testing, mock, date, logging, CLI and validation domains need no
package at all.

## 7. Decisions for the lead

- **D1. Canonical API stance.** Recommended: Yan-native canonical API; Python names only through the
  lenient reader and `yan import --python` (section 5.2). Alternative: Python-shaped packages with Yan
  extensions, which gives easier first contact and permanent technical debt.
- **D2. Evaluation gate.** Recommended: a package reaches 1.0 only with a first-try pass@1 check against a
  Python-idiom task set, reported with the commit and date. Alternative: ship on design review alone.
- **D3. Shortlist order.** Recommended: section 6 as written (http, frame aliases, stats and plot, web,
  llm, term, template, xlsx). Alternative: put `@yan/llm` first because it is the most agent-specific and
  the only domain where Python's SDKs have no determinism story.
- **D4. Scope of `@yan/web`.** Recommended: router, typed handlers, OpenAPI, no ORM, admin or auth.
  Alternative: defer web until after M2 and let a community framework appear.
- **D5. Cloud SDKs.** Recommended: build the generator and ship one proof service, not hand-written
  clients. Alternative: leave all cloud SDKs to the community and the `py` bridge.
- **D6. First-party burden.** Eight new packages are eight maintenance lines for one developer
  (the 2026-11-05 study already adds seven). Recommended: take items 1 to 3 now, 4 to 6 after M2, and 7
  to 8 only when a user asks. Alternative: build all eight before M3.

## 8. Method and limits

- Download figures: hugovk top-pypi-packages, ClickHouse source, last update 2026-11-01 06:41 UTC,
  snapshot fetched 2026-11-17 [V]. Counts include CI and mirror traffic. The same JSON is served at
  both its 30-day and default URL; the figures are 30-day.
- Dependents and stars: libraries.io, fetched 2026-11-17 [V]; rate-limited for about a third of the
  names, mis-mapped for three. GitHub stars via the GitHub API were not available.
- Claims about project history and design (polars, pandas, httpx, ruff, uv, pydantic v2, rich, jax) are
  background knowledge [B]; pandas 3.x defaults were not re-checked. The comparisons rest on named
  public features, not on measured speed claims.
- Spec facts were read from `spec/` at `72fc820` on 2026-11-17 [V]; the earlier studies cited are in this
  directory and `docs/studies/`.
- No implementation was written or run. API sketches are illustrative and may not parse.
