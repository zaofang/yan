# Yan Query Language 言查 (`.yql`): syntax study

Status: brainstorm study, 2026-11-04. Not normative. No spec rows, row numbers or diagnostic codes are
proposed here; anything adopted goes through an RFC (`CLAUDE.md`). Every code block below is a
**MOCK**. Names are frozen and used as decided.

## Summary

1. Pipelines won the market argument: Google's pipe SQL (2024), Spark 4.0 and Databricks (2025), KQL,
   PRQL and dplyr all read in dataflow order. Yan Query Language keeps that spine.
2. SQL's real damage is semantic, not lexical: fan-out double counting, three-valued logic, window
   and `WHERE` evaluation order, unordered results. Yan's core already fixes order and determinism;
   the surface should fix the rest by construction.
3. The consolidated proposal adds four ideas to RFC 0002 D4: **nest, don't fan out** (a group-join
   stage), **checked cardinality** (`join` is a keyed lookup, fan-out is spelled `join many`),
   **groups are scopes** (one indented `group` block replaces `HAVING`, `WINDOW`, `QUALIFY` and
   top N per group), and **two-valued logic** (optional columns are types, never `UNKNOWN`).
4. Two more items reuse what Yan already has: `rule` items for recursion (lowering to `@yan/rel`,
   with lattice columns for termination) and `view` items, which compile the same body to an
   incremental DBSP circuit.
5. On five shared tasks the consolidated form is 12% smaller than SQL overall and 27% smaller on
   the four non-recursive tasks (lexical tokens, a proxy), with 13 stage words; every line is a
   type-checked prefix, which suits agents and constrained decoding.

---

## 1. Methodology and process

**Brief.** Antonio's message: research peers and competitors, what users want, what works and what
does not, old and new research; experiment and brainstorm; write five proposals that each optimize
one axis; mix and match; consolidate. Target feel: elegant, concise, powerful, opinionated without
being obnoxious.

**Steps, in order.**

1. Read the current Yan material narrowly: `docs/surfaces.md` section 8 (D4 example and the MOCK
   grammar), RFC 0002 D4, spec 16.7 (R536 admission rules N1 to N9, R539), spec 11.8 and 11.9
   (Arrow columns, `Table[R]`, determinism, query arithmetic, Futamura compilation, windows R553,
   incremental views R625), spec 5.6 (column expressions and `Keyed[K, T]`, R261), spec 17
   (`@yan/rel`, R262), spec 15 (lambdas, precedence, record syntax) and RFC 0001's query
   comprehension grammar.
2. Searched the web for the current state of pipe SQL, PRQL, Malloy, text-to-SQL benchmarks, LLM
   generation of pipe languages (KQL), SQL error studies and low-resource language effects on LLMs.
   Sources are in section 9; claims carry a confidence mark: **(H)** primary source read or
   long-established, **(M)** secondary source or recollection of a primary one, **(L)** anecdote or
   my own judgment.
3. Fixed one schema and five tasks that stress the places query languages differ: filter and
   lookup, grouped aggregation with a post-filter, aggregation across two joins (the fan-out trap),
   top N per group with shares (windows), and recursion.
4. Wrote a standard SQL baseline, then each of the five proposals on the same five tasks.
5. Measured every sample (section 6). The plan was to count model tokens; the container's network
   blocks tokenizer downloads, so the measure is a lexical token count (identifiers, literals,
   operators) plus characters. That is a proxy, stated as such.
6. Scored the proposals against each other, took the best mechanism from each, and checked the mix
   against Yan's constraints: law 1 (syntax only, lowering to `LazyFrame[R]`), N3 one printing, N4
   same item hash, N5 five facts visible, determinism of 11.9.

**One finding from the process itself.** My first SQL draft of task T4 computed each product's
share of its category after filtering to the top three, so the shares summed to 1 over three rows
instead of over the category. SQL evaluates window functions after `WHERE` of the same level, so the
fix needs a second CTE. The bug is silent and easy to make; it shaped the "groups are scopes" idea
in section 7.

---

## 2. Findings

### 2.1 Market map

| Language | Model | What works | What does not |
| --- | --- | --- | --- |
| ISO SQL (SQL:2023) | declarative clauses in fixed order | universal, huge training corpus, optimizers | clause order differs from evaluation order; 3VL nulls; bag semantics without order; dialects; window/`WHERE` order traps (H) |
| DuckDB "friendly SQL" | SQL plus shortcuts | `FROM` first, `GROUP BY ALL`, `SELECT * EXCLUDE`, `COLUMNS(...)` remove repetition (H) | shortcuts are implicit; silent mistakes reported by users (L) |
| Pipe SQL (GoogleSQL 2024, BigQuery, Spark 4.0, Databricks 16.2+) | `\|>` operators appended to SQL | adopted by three major engines; each prefix is a query; no new language to learn (H) | inherits SQL expressions, nulls and fan-out; two syntaxes in one language (M) |
| PRQL | pipeline compiled to SQL | elegant, top to bottom, functions, `group (...)` with a nested pipeline (H) | limited by target SQL dialect; development has slowed; maintainers say LLMs reduced demand (M) |
| Malloy | semantic model plus queries | reusable measures and dimensions, nested results, symmetric aggregates prevent fan-out (H) | needs a model up front; niche adoption (M) |
| KQL (Kusto), SPL, ES\|QL, PPL | log-search pipelines | `summarize ... by`, `top N by`, one operator per line; very fast to write (H) | vendor-locked; loose typing (M) |
| LINQ (C#) | comprehensions over typed sequences | fully typed, composable, nested results (H) | most code uses the method chain, not the query syntax (L) |
| dplyr / tidyverse | verbs on data frames | the verb set (`filter`, `mutate`, `summarise`, `group_by`, `arrange`) is the most copied design in data work (H) | non-standard evaluation surprises; grouped state carried silently between verbs (M) |
| Polars, Ibis | expression API, lazy plans | expressions as values; lazy optimizer; Arrow (H) | method chains get long; Python typing does not see columns (M) |
| q / kdb+ qSQL | terse array language with SQL words | extreme brevity, ordered tables, time series (H) | hard to read for newcomers; small community (M) |
| Datalog (Soufflé, Logica, Rel, `@yan/rel`) | rules, fixpoints | recursion and graph queries are natural; semi-naive evaluation (H) | unfamiliar; aggregation and negation need stratification (H) |
| EdgeQL (Gel) | shapes over linked objects | nested results without joins; cardinality in the schema (H) | tied to its database (M) |
| Cypher / GQL (ISO/IEC 39075:2024), SQL/PGQ | graph patterns | path patterns read as pictures (H) | separate world from tables (M) |
| DAX, LookML, dbt semantic layer | measures evaluated in context | metrics defined once (H) | DAX context transition is a known source of confusion (M) |
| DBSP / Feldera, Materialize, differential dataflow | incremental SQL | same query batch or incremental (H) | SQL surface keeps SQL's problems (M) |

### 2.2 What users want

- **Dataflow reading order.** The pipe SQL paper's main claim, and the reason Google, Databricks and
  Spark shipped it (H). Completion also gets better when `from` comes first (M).
- **Less repetition.** Repeated `GROUP BY` expressions, repeated aggregate expressions in `HAVING`,
  `SELECT *` minus one column (H: DuckDB's most cited additions).
- **Reuse.** Variables, functions and named measures; SQL's lack of abstraction is the most common
  complaint after clause order (M: "Against SQL", HN threads on PRQL).
- **Correct joins.** Double counting after a one-to-many join is the canonical analytics bug;
  Malloy's symmetric aggregates exist to stop it (H).
- **Nested results.** JSON-shaped answers without self-joins (EdgeQL, Malloy, GraphQL) (M).
- **Predictable nulls.** `NOT IN` with a null, `COUNT(col)` versus `COUNT(*)`, `NULL = NULL`
  (H: Guagliardo and Libkin show SQL's null semantics give wrong answers on incomplete data).
- **Tests next to queries.** dbt's popularity is largely tests and modularity (M).
- **Agents that write correct queries.** Text-to-SQL accuracy on realistic enterprise schemas was
  low at release of Spider 2.0 (ICLR 2025: low double digits for frontier models) (H); errors are
  mostly semantic, not syntactic (H for humans: Taipalus et al. 2018; M for models: KQL study: large models
  zero-shot score 0.90 to 0.97 on syntax but 0.27 to 0.70 on semantics).

### 2.3 What research says

- **Old and settled.** Codd's relational model (1970); SEQUEL (1974); Date's critique of SQL (1984);
  The Third Manifesto (no nulls, no duplicates, typed relations); comprehension syntax as the
  calculus for collections (Wadler 1990; Buneman, Libkin, Suciu, Tannen, Wong 1994). Comprehensions
  normalize to flat joins without query avalanches (Cheney, Lindley, Wadler, ICFP 2013), so a typed
  surface loses no performance to SQL (H).
- **Physical.** Group-join fuses a join with the aggregation that follows it (Moerkotte and Neumann,
  VLDB 2011) (H): a surface `nest` stage maps to a known fast operator. Morsel-driven execution (Leis
  et al. 2014) and data-centric compilation (Neumann 2011) are already in 11.9 (H).
- **Recursion and lattices.** Semi-naive Datalog; lattice-valued columns guarantee termination for
  shortest-path style rules (Flix, Madsen et al. PLDI 2016) (H).
- **Incrementality.** DBSP (Budiu et al. VLDB 2023) gives every relational operator an incremental
  form over Z-sets; R625 already adopts it (H). Provenance semirings (Green et al. 2007) are the
  same algebra (H).
- **Humans.** SQL semantic errors and complications persist after syntax errors are fixed (Taipalus,
  Siponen, Vartiainen 2018) (H); joins, grouping and subqueries are the hot spots (M).
- **Models.** Low-resource languages score much lower than Python unless the model is trained on
  them (MultiPL-T, OOPSLA 2024) (H), so a new query surface should reuse words models already know.
  Grammar-constrained decoding removes syntax errors for SQL (PICARD 2021) and for general grammars
  (Geng et al. 2023) (H); a small LL(1) stage grammar is cheap to constrain. Claims that pipe SQL
  improves model accuracy are so far projections, not measurements (M).

### 2.4 What this means for Yan

- Yan already has what SQL lacks underneath: typed rows, `T | None` as the only nullability (11.8),
  stable deterministic order (11.9), checked overflow ending in `fail QueryError`, Futamura-compiled
  constant plans, windows (R553), DBSP views (R625), Datalog (`@yan/rel`), units and money. The
  surface's job is to make those properties the easy path, not to invent semantics (N1, law 1).
- Bare column names and one stage per line (D4) are right and stay.
- The gaps in D4 are the semantic traps: joins that fan out, aggregates over joined rows, windows,
  per-group stages (`take N per K` is one special case of a general idea), recursion, and nulls in
  comparisons.

---

## 3. Shared schema and tasks

```
type Customer = { id: i64, name: Str, region: Str, email: Str }
type Order    = { id: i64, customer_id: i64, status: Status, placed: PlainDate, due: PlainDate }
type Line     = { order_id: i64, product_id: i64, qty: i64, price: Money }
type Product  = { id: i64, category: Str, name: Str }
type Employee = { id: i64, name: Str, manager_id: i64 | None }

type Late          = { id: i64, email: Str, days_late: i64 }
type RegionMonth   = { region: Str, month: YearMonth, revenue: Money, avg_order: Money }
type CustomerStats = { id: i64, name: Str, orders: i64, revenue: Money }
type TopProduct    = { category: Str, name: Str, revenue: Money, share: f64, rank: i64 }
type Report        = { id: i64, name: Str, depth: i64 }
type OrderTotal    = { id: i64, customer_id: i64, amount: Money }
type Under         = { boss: i64 | None, id: i64, depth: i64 }
```

- **T1 Late orders.** Open orders past due, with the customer's email and days late, worst 20.
- **T2 Monthly revenue.** Per region and month of 2026: revenue and average order value, only
  months over 10,000 USD.
- **T3 Customer stats.** Per customer: order count and revenue, including customers with no orders.
  The trap: joining orders and lines then counting orders double counts.
- **T4 Top products.** Top 3 products per category by revenue, with rank and share of the whole
  category's revenue.
- **T5 Reports.** Everyone under manager `m`, directly or indirectly, with depth.

### SQL baseline

**T1**

```
SELECT o.id, c.email, :today - o.due AS days_late
FROM orders o JOIN customers c ON c.id = o.customer_id
WHERE o.status = 'open' AND o.due < :today
ORDER BY days_late DESC
LIMIT 20;
```

**T2**

```
SELECT c.region, date_trunc('month', o.placed) AS month,
       sum(l.qty * l.price) AS revenue,
       sum(l.qty * l.price) / count(DISTINCT o.id) AS avg_order
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN lines l ON l.order_id = o.id
WHERE o.placed >= DATE '2026-01-01' AND o.placed < DATE '2027-01-01'
GROUP BY c.region, month
HAVING sum(l.qty * l.price) > 10000;
```

**T3**

```
SELECT c.id, c.name,
       count(DISTINCT o.id) AS orders,
       coalesce(sum(l.qty * l.price), 0) AS revenue
FROM customers c
LEFT JOIN orders o ON o.customer_id = c.id
LEFT JOIN lines l ON l.order_id = o.id
GROUP BY c.id, c.name;
```

**T4**

```
WITH rev AS (
  SELECT p.category, p.name, sum(l.qty * l.price) AS revenue
  FROM lines l JOIN products p ON p.id = l.product_id
  GROUP BY p.category, p.name
), ranked AS (
  SELECT *, row_number() OVER (PARTITION BY category ORDER BY revenue DESC) AS rank,
         revenue / sum(revenue) OVER (PARTITION BY category) AS share
  FROM rev
)
SELECT category, name, revenue, share, rank FROM ranked
WHERE rank <= 3
ORDER BY category, rank;
```

**T5**

```
WITH RECURSIVE under(id, name, depth) AS (
  SELECT id, name, 1 FROM employees WHERE manager_id = :m
  UNION ALL
  SELECT e.id, e.name, u.depth + 1
  FROM employees e JOIN under u ON e.manager_id = u.id
)
SELECT * FROM under ORDER BY depth, id;
```

SQL notes: `row_number()` with tied revenue is nondeterministic in SQL; T5 loops forever on a cycle
in the data; T3 needs `count(DISTINCT ...)` and `coalesce` to be right.

---

## 4. Five proposals

Each proposal pushes one axis as far as it goes. The extremes are deliberate: the mix in section 7
takes mechanisms, not whole proposals.

### P1 Terse (axis: brevity)

**Idea.** q, KQL and APL. Stages are told apart by shape, not keywords: a boolean expression
filters, `+a = e` derives, a name list selects, `aggs by keys` groups. `join T` infers keys from
reference types (`customer_id: Ref[Customer]`). `top N key per K` covers sort, limit and top N per
group. Line-leading `|` chains stages.

**T1**

```
query late(orders: Table[Order], customers: Table[Customer], today: PlainDate) -> Table[Late] =
  orders | status == .open and due < today | join customers
  | +days_late = (today - due).days()
  | id, email, days_late | top 20 -days_late
```

**T2**

```
query by_month(orders: Table[Order], customers: Table[Customer], lines: Table[Line]) -> Table[RegionMonth] =
  orders | placed.year() == 2026 | join customers | join lines
  | +month = placed.year_month(), +amt = qty * price
  | revenue = sum(amt), avg_order = sum(amt) / count_distinct(id) by region, month
  | revenue > 10_000usd
```

**T3**

```
query stats(customers: Table[Customer], orders: Table[Order], lines: Table[Line]) -> Table[CustomerStats] =
  customers | left join orders | left join lines
  | orders = count_distinct(orders.id), revenue = sum(qty * price) else 0usd by id, name
```

**T4**

```
query top(lines: Table[Line], products: Table[Product]) -> Table[TopProduct] =
  lines | join products
  | revenue = sum(qty * price) by category, name
  | +share = revenue / sum(revenue) per category
  | top 3 -revenue per category as rank
```

**T5**

```
query reports(employees: Table[Employee], m: i64) -> Table[Report] =
  employees | manager_id == m | +depth = 1
  | rec r: employees | manager_id == r.id | +depth = r.depth + 1
  | id, name, depth | sort depth, id
```

**Strengths.** Smallest text; reads fast once learned; `top ... per` is a gem.
**Costs.** Shape-based stages are ambiguous to humans and parsers (is `a, b` a select or a tuple?);
`id` after two joins resolves by a hidden rule, the silent-error class users complain about in
friendly SQL; inferred join keys break when a table has two references to the same type; `|` is
already bitwise or in Yan; fan-out and nulls are SQL's.

### P2 Familiar (axis: learnability for SQL users)

**Idea.** Pipe SQL's operator words in Yan lexis: `from`, `where`, `extend`, `select`, `aggregate
... group by`, `order by ... desc`, `limit`, `join ... on`, `left join`, `with recursive`, windows
with `over (partition by ... order by ...)`. Every SQL user and every model reads it on sight.

**T1**

```
query late(orders: Table[Order], customers: Table[Customer], today: PlainDate) -> Table[Late] =
  from orders as o
  where o.status == Status.open and o.due < today
  join customers as c on c.id == o.customer_id
  extend days_late = (today - o.due).days()
  select o.id, c.email, days_late
  order by days_late desc
  limit 20
```

**T2**

```
query by_month(orders: Table[Order], customers: Table[Customer], lines: Table[Line]) -> Table[RegionMonth] =
  from orders as o
  where o.placed >= 2026-01-01 and o.placed < 2027-01-01
  join customers as c on c.id == o.customer_id
  join lines as l on l.order_id == o.id
  extend month = o.placed.year_month(), amount = l.qty * l.price
  aggregate revenue = sum(amount), avg_order = sum(amount) / count_distinct(o.id)
    group by c.region, month
  where revenue > 10_000usd
```

**T3**

```
query stats(customers: Table[Customer], orders: Table[Order], lines: Table[Line]) -> Table[CustomerStats] =
  from customers as c
  left join orders as o on o.customer_id == c.id
  left join lines as l on l.order_id == o.id
  aggregate orders = count_distinct(o.id), revenue = sum(l.qty * l.price) else 0usd
    group by c.id, c.name
```

**T4**

```
query top(lines: Table[Line], products: Table[Product]) -> Table[TopProduct] =
  from lines as l
  join products as p on p.id == l.product_id
  aggregate revenue = sum(l.qty * l.price) group by p.category, p.name
  extend rank = row_number() over (partition by category order by revenue desc),
         share = revenue / sum(revenue) over (partition by category)
  where rank <= 3
  order by category, rank
```

**T5**

```
query reports(employees: Table[Employee], m: i64) -> Table[Report] =
  with recursive r = (
    from employees where manager_id == m select id, name, depth = 1
    union all
    from employees as e join r on e.manager_id == r.id
    select e.id, e.name, depth = r.depth + 1
  )
  from r
  order by depth, id
```

**Strengths.** Zero learning for SQL users; models have seen these words millions of times; easy
migration and `yan view --as sql` style export.
**Costs.** Carries SQL's semantic traps into Yan (fan-out, `count_distinct` as a patch, window
frames, recursion without termination); aliases everywhere; two-word keywords; `with recursive` is
the least loved part of SQL.

### P3 Agent-first (axis: model token cost and repair)

**Idea.** One stage per line, each starting with one of a closed set of PRQL words models know
(`from where derive select join group sort take window iterate`). No aliases: joined columns are
named through the input's own name (`customers.email`). Every prefix is a valid query whose row
type the tool can print, so a repair loop gets a per-line error with the row type at that line.
Grammar is LL(1) after the first word: cheap to export as GBNF or llguidance for constrained
decoding, which Plan A's harness already plans.

**T1**

```
query late(orders: Table[Order], customers: Table[Customer], today: PlainDate) -> Table[Late] =
  from orders
  where status == Status.open and due < today
  join customers on customer_id == customers.id
  derive days_late = (today - due).days()
  select id, customers.email, days_late
  sort -days_late
  take 20
```

**T2**

```
query by_month(orders: Table[Order], customers: Table[Customer], lines: Table[Line]) -> Table[RegionMonth] =
  from orders
  where placed.year() == 2026
  join customers on customer_id == customers.id
  join lines on id == lines.order_id
  derive month = placed.year_month(), amount = lines.qty * lines.price
  group customers.region, month (revenue = sum(amount), orders = count_distinct(id))
  derive avg_order = revenue / orders
  where revenue > 10_000usd
  select region, month, revenue, avg_order
```

**T3**

```
query stats(customers: Table[Customer], orders: Table[Order], lines: Table[Line]) -> Table[CustomerStats] =
  from customers
  left join orders on id == orders.customer_id
  left join lines on orders.id == lines.order_id
  group id, name (orders = count_distinct(orders.id), revenue = sum(lines.qty * lines.price) else 0usd)
```

**T4**

```
query top(lines: Table[Line], products: Table[Product]) -> Table[TopProduct] =
  from lines
  join products on product_id == products.id
  group products.category, products.name (revenue = sum(qty * price))
  window category sort -revenue (rank = row_number(), share = revenue / sum(revenue))
  where rank <= 3
  sort category, rank
```

**T5**

```
query reports(employees: Table[Employee], m: i64) -> Table[Report] =
  from employees
  where manager_id == m
  select id, name, depth = 1
  iterate frontier
    join employees on frontier.id == employees.manager_id
    select employees.id, employees.name, depth = frontier.depth + 1
  sort depth, id
```

**Strengths.** Predictable, line-local, known words; the window sum is the whole partition (Yan
R553 has explicit `cum_sum`), so SQL's default running frame trap is gone.
**Costs.** Qualified names are long (`products.category`); fan-out still needs `count_distinct`;
`window` and `group` are two ways to say "per group".

### P4 Strict (axis: safety and explicitness)

**Idea.** Every join states its cardinality (`join one`, `join opt`, `join many`); `join one`
requires the right side to be keyed on the compared column (`Table[Customer] key id`). Aggregating
after `join many` is refused; `nest` groups the right side into a `Table` column instead. Optional
columns cannot be ordered or added without `else`. `take` needs a total `sort`. The final `select`
must spell the output type. Recursion needs a bound.

**T1**

```
query late(orders: Table[Order], customers: Table[Customer] key id, today: PlainDate)
    -> Table[Late] =
  from orders
  where status == Status.open and due < today
  join one c = customers on c.id == customer_id
  derive days_late = (today - due).days()
  select id, email = c.email, days_late
  sort -days_late, id
  take 20
```

**T2**

```
query by_month(orders: Table[Order], customers: Table[Customer] key id, lines: Table[Line])
    -> Table[RegionMonth] =
  from orders
  where placed.year() == 2026
  join one c = customers on c.id == customer_id
  nest ls = lines on ls.order_id == id
  derive month = placed.year_month(), amount = sum(ls.qty * ls.price)
  agg revenue = sum(amount), avg_order = mean(amount) by region = c.region, month
  where revenue > 10_000usd
  select region, month, revenue, avg_order
```

**T3**

```
query stats(customers: Table[Customer], orders: Table[Order], lines: Table[Line])
    -> Table[CustomerStats] =
  from customers
  nest os = order_totals(orders, lines) on os.customer_id == id
  select id, name, orders = os.len(), revenue = sum(os.amount)

query order_totals(orders: Table[Order], lines: Table[Line]) -> Table[OrderTotal] =
  from orders
  nest ls = lines on ls.order_id == id
  select id, customer_id, amount = sum(ls.qty * ls.price)
```

**T4**

```
query top(lines: Table[Line], products: Table[Product] key id) -> Table[TopProduct] =
  from lines
  join one p = products on p.id == product_id
  agg revenue = sum(qty * price) by category = p.category, name = p.name
  derive share = revenue / sum(revenue).over(by = [category])
  derive rank = row_number().over(by = [category], sort = [-revenue, name])
  where rank <= 3
  sort category, rank
  select category, name, revenue, share, rank
```

**T5**

```
query reports(employees: Table[Employee] key id, m: i64) -> Table[Report] =
  from employees
  where manager_id == m
  select id, name, depth = 1
  iterate r max 64
    join many e = employees on e.manager_id == r.id
    select id = e.id, name = e.name, depth = r.depth + 1
  sort depth, id
  select id, name, depth
```

**Strengths.** The fan-out bug cannot be written; T3 needs no `count_distinct` and no `coalesce`
(sum of an empty table is `0usd`); every invariant is on the page.
**Costs.** `join one` and the mandatory final `select` are noise in the common case; `max 64` is a
guess dressed as safety; the `over(...)` calls are long.

### P5 Power (axis: expressiveness)

**Idea.** Tuple calculus, not bare columns: `for x in t` binds rows, `let` binds values, `group g
by (...)` binds a group, `yield` builds the output record. Correlated subqueries are ordinary
expressions and normalize to joins (Cheney et al. 2013). Recursion is `rule` items in Datalog
shape, with a lattice column (`min depth`) so cyclic data terminates.

**T1**

```
query late(orders: Table[Order], customers: Table[Customer], today: PlainDate) -> Table[Late] =
  for o in orders
  where o.status == Status.open and o.due < today
  for c in customers where c.id == o.customer_id
  let days_late = (today - o.due).days()
  sort -days_late
  take 20
  yield Late(id: o.id, email: c.email, days_late: days_late)
```

**T2**

```
query by_month(orders: Table[Order], customers: Table[Customer], lines: Table[Line]) -> Table[RegionMonth] =
  for o in orders where o.placed.year() == 2026
  for c in customers where c.id == o.customer_id
  let amount = lines.where(l -> l.order_id == o.id).sum(l -> l.qty * l.price)
  group g by (region = c.region, month = o.placed.year_month())
  let revenue = g.sum(r -> r.amount)
  where revenue > 10_000usd
  yield RegionMonth(region: g.region, month: g.month, revenue: revenue, avg_order: revenue / g.len())
```

**T3**

```
query stats(customers: Table[Customer], orders: Table[Order], lines: Table[Line]) -> Table[CustomerStats] =
  for c in customers
  let os = orders.where(o -> o.customer_id == c.id)
  let ls = lines.where(l -> l.order_id in os.id)
  yield CustomerStats(id: c.id, name: c.name, orders: os.len(), revenue: ls.sum(l -> l.qty * l.price))
```

**T4**

```
query top(lines: Table[Line], products: Table[Product]) -> Table[TopProduct] =
  for l in lines
  for p in products where p.id == l.product_id
  group g by (category = p.category, name = p.name)
  let revenue = g.sum(r -> r.l.qty * r.l.price)
  group c by (category = g.category)
  let total = c.sum(r -> r.revenue)
  for (rank, r) in c.sort(r -> -r.revenue).take(3).enumerate(1)
  yield TopProduct(category: c.category, name: r.g.name, revenue: r.revenue,
                   share: r.revenue / total, rank: rank)
```

**T5**

```
rule under(employees: Table[Employee]) -> Table[Under] min depth =
  Under(boss: e.manager_id, id: e.id, depth: 1) if e in employees
  Under(boss: u.boss, id: e.id, depth: u.depth + 1)
    if u in under, e in employees, e.manager_id == u.id

query reports(employees: Table[Employee], m: i64) -> Table[Report] =
  for u in under(employees) where u.boss == m
  for e in employees where e.id == u.id
  sort (u.depth, e.id)
  yield Report(id: e.id, name: e.name, depth: u.depth)
```

**Strengths.** Anything expressible; nested results and correlated subqueries are natural; `rule`
with a lattice is the cleanest recursion of all five and terminates on cycles.
**Costs.** Lambdas and row variables everywhere (`r -> r.l.qty`); grouping twice in T4 is hard to
read; longest text; reads like code, not like a query.

---

## 5. Comparison

Scores are my judgment (L), 1 to 5, higher is better for the axis.

| | P1 Terse | P2 Familiar | P3 Agent | P4 Strict | P5 Power |
| --- | --- | --- | --- | --- | --- |
| Brevity | 5 | 2 | 3 | 2 | 1 |
| Readable at first sight | 2 | 5 | 4 | 4 | 2 |
| Agent generation and repair | 2 | 4 | 5 | 4 | 2 |
| Safety (fan-out, nulls, termination) | 1 | 1 | 2 | 5 | 4 |
| Expressiveness | 3 | 3 | 3 | 3 | 5 |
| One printing (N3) friendliness | 2 | 3 | 4 | 4 | 3 |

What each contributes to the mix:

- **P1:** `agg ... by` on one line (KQL's `summarize by`), `-col` for descending, per-group take.
- **P2:** the words people and models know: `from`, `where`, `select`, `join`, `sort`, `take`.
- **P3:** one stage per line, prefix-valid, LL(1), row type per line, whole-partition aggregates.
- **P4:** keyed joins, `nest`, two-valued optionals, empty-aware aggregates, explicit fan-out.
- **P5:** `rule` items with lattice columns; measures as functions over nested tables.

---

## 6. Experiment: size of the samples

Method: count every sample body (the item header is excluded, since SQL has none; helper items and
`rule` items count). Lexical tokens are identifiers, numbers, string literals and operators, with
`==`, `>=`, `->`, `|>` counted as one; indentation is not counted. This is a proxy for model tokens,
not a measurement of them (tokenizer downloads were blocked in this container).

| Proposal | T1 | T2 | T3 | T4 | T5 | Total tokens | Total chars | Lines | vs SQL |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| SQL baseline | 51 | 109 | 73 | 111 | 66 | 410 | 1421 | 40 | +0% |
| P1 Terse | 37 | 58 | 33 | 38 | 41 | 207 | 774 | 16 | -50% |
| P2 Familiar | 60 | 100 | 63 | 78 | 59 | 360 | 1297 | 35 | -12% |
| P3 Agent-first | 45 | 83 | 51 | 57 | 46 | 282 | 1240 | 33 | -31% |
| P4 Strict | 52 | 86 | 67 | 94 | 61 | 360 | 1344 | 37 | -12% |
| P5 Power | 68 | 122 | 78 | 139 | 134 | 541 | 1626 | 35 | +32% |
| Mix | 47 | 91 | 61 | 53 | 109 | 361 | 1308 | 39 | -12% |

Reading the numbers:

- P1 is half of SQL and loses on everything else (section 4).
- On T1 to T4 the mix is 252 tokens against SQL's 344 (27% smaller) and P3's 236, while carrying
  P4's guarantees: `nest` removes `count_distinct`, `coalesce` and the second CTE, and the `group`
  block removes the window boilerplate (T4: 53 against SQL's 111).
- T5 is where the mix pays: the `rule` item plus its query is 109 tokens against SQL's 66 and P3's
  46. That buys termination on cyclic data and typed records; open question 11 asks whether a
  P3-style `iterate` stage should exist for simple closures.
- Yan expressions cost tokens SQL does not spend (`Status.open`, `(today - due).days()`), which is
  why P2 is larger than SQL on T1.
- Size is not the goal. The real test is pass@1 and repair rounds on Sonnet and Haiku with the
  Plan A harness (open question 9).

---

## 7. The mix: a consolidated proposal

### 7.1 Ten frameworks

1. **The pipeline is the spine.** `from` first, one stage per line, every prefix is a valid query
   with a row type the tool prints. Errors name a line and list the columns available there.
2. **Known words, closed set.** Thirteen stage words: `from where derive select drop join nest agg
   group sort take distinct append`. Item words: `query`, `view`, `rule`. Nothing else is a keyword
   inside a query body.
3. **Nest, don't fan out.** `nest ls = lines on ls.order_id == id` gives each row a `Table[Line]`
   column. Aggregates over it are per row. A row with no matches gets an empty table, never `None`.
   It lowers to a group-join, so it is as fast as the join it replaces. Left joins and
   `count(DISTINCT ...)` mostly disappear.
4. **Cardinality is checked.** `join c = customers on c.id == customer_id` is a lookup: the right
   side must be keyed on the compared column (`Table[Customer] key id`), so the row count never
   grows. `join opt` makes the joined record optional. A genuine fan-out is spelled `join many`, the
   only form that multiplies rows, so it is visible in review.
5. **Groups are scopes.** `agg a = e by k` collapses to one row per key. A `group k` block runs an
   indented sub-pipeline per group; inside it, aggregates in `derive` and `where` are over the whole
   group (windows and `QUALIFY`), `sort` and `take` are per group (top N per group), and
   `row_number()` and `cum_*` follow the block's `sort`. One construct replaces `HAVING`, `WINDOW`,
   `OVER`, `QUALIFY` and D4's `take N per K`.
6. **Two-valued logic.** A `T | None` column compares equal or not, never unknown. Ordering,
   arithmetic and aggregation on an optional are type errors until `else` gives a default. `sum`
   and `count` of an empty table are zero; `mean`, `min` and `max` return `T | None` except inside
   `agg ... by`, whose groups are never empty.
7. **Order is semantics.** Every table is a sequence. Each stage has a stated order effect (7.4),
   so results are the same on every machine, as 11.9 already requires, and readers can predict
   them.
8. **Measures are functions.** A reusable metric is an ordinary Yan function over a table, such as
   `total(ls: Table[Line]) -> Money = sum(ls.qty * ls.price)`, called on a nested table. No
   semantic-layer keyword: the semantic layer is the module.
9. **One body, two clocks.** `view` instead of `query` compiles the same body to an incremental
   `View[R]` (R625); `apply(delta)` updates it. A future streaming source changes nothing in the
   body.
10. **Rules for recursion, lattices for termination.** `rule` items hold Datalog-shaped clauses over
    typed records and lower to `@yan/rel`. A lattice column (`min depth`) keeps the best value per
    key and makes rules over cyclic data terminate.

What was declined: P1's shape-typed stages and inferred join keys (silent errors), P2's aliases
with `as` and two-word keywords, P3's qualified names and separate `window` stage, P4's `join one`,
mandatory final `select` and `max N` bounds, P5's row variables and lambdas for ordinary queries.

### 7.2 Samples

**T1**

```
query late(orders: Table[Order], customers: Table[Customer] key id, today: PlainDate)
    -> Table[Late] =
  from orders
  where status == Status.open and due < today
  join c = customers on c.id == customer_id
  derive days_late = (today - due).days()
  select id, c.email, days_late
  sort -days_late
  take 20
  ex late(fixtures.orders, fixtures.customers, 2026-11-04).len() == 20
```

**T2**

```
total(ls: Table[Line]) -> Money = sum(ls.qty * ls.price)

query by_month(orders: Table[Order], customers: Table[Customer] key id, lines: Table[Line])
    -> Table[RegionMonth] =
  from orders
  where placed.year() == 2026
  join c = customers on c.id == customer_id
  nest ls = lines on ls.order_id == id
  derive month = placed.year_month(), amount = total(ls)
  agg revenue = sum(amount), avg_order = mean(amount) by c.region, month
  where revenue > 10_000usd
```

**T3**

```
query stats(customers: Table[Customer], orders: Table[Order], lines: Table[Line])
    -> Table[CustomerStats] =
  from customers
  nest os = order_totals(orders, lines) on os.customer_id == id
  select id, name, orders = os.len(), revenue = sum(os.amount)

query order_totals(orders: Table[Order], lines: Table[Line]) -> Table[OrderTotal] =
  from orders
  nest ls = lines on ls.order_id == id
  select id, customer_id, amount = total(ls)
```

**T4**

```
query top(lines: Table[Line], products: Table[Product] key id) -> Table[TopProduct] =
  from lines
  join p = products on p.id == product_id
  agg revenue = sum(qty * price) by p.category, p.name
  group category
    derive share = revenue / sum(revenue)
    sort -revenue
    derive rank = row_number()
    take 3
  sort category
```

**T5**

```
rule under(employees: Table[Employee]) -> Table[Under] min depth =
  Under(boss: e.manager_id, id: e.id, depth: 1) if e in employees
  Under(boss: u.boss, id: e.id, depth: u.depth + 1)
    if u in under, e in employees, e.manager_id == u.id

query reports(employees: Table[Employee] key id, m: i64) -> Table[Report] =
  from under(employees)
  where boss == m
  join e = employees on e.id == id
  select id, e.name, depth
  sort depth, id
```

Notes on the samples: in T4 `share` is `Money / Money`, so units make it a plain `f64`; `sum` in the
block is over the whole group because it runs before `take`; the tie order of equal revenue is the
input order, the same on every run. In T3 a customer with no orders has `os.len() == 0` and
`sum(os.amount) == 0usd`, with no `coalesce`. In T5 `boss` is `i64 | None` and `boss == m` is
two-valued.

### 7.3 MOCK grammar

```
query_item = ("query" | "view") NAME [tparams] "(" [params] ")" "->" type ["fail" type] "="
             NEWLINE INDENT pipeline {ex} DEDENT
rule_item  = "rule" NAME "(" [params] ")" "->" type [lattice] "="
             NEWLINE INDENT clause {clause} {ex} DEDENT
lattice    = ("min" | "max") NAME
clause     = expr NEWLINE [INDENT] "if" premise {"," premise} NEWLINE
premise    = NAME "in" expr | expr
pipeline   = "from" expr NEWLINE {stage NEWLINE}
stage      = "where" expr
           | "derive" assign {"," assign}
           | "select" item {"," item}
           | "drop" NAME {"," NAME}
           | "join" ["opt" | "many"] NAME "=" expr "on" expr
           | "nest" NAME "=" expr "on" expr
           | "agg" assign {"," assign} ["by" item {"," item}]
           | "group" item {"," item} NEWLINE INDENT stage NEWLINE {stage NEWLINE} DEDENT
           | "sort" ["-"] expr {"," ["-"] expr}
           | "take" expr
           | "distinct" [item {"," item}]
           | "append" expr
item       = NAME | NAME "." NAME | assign
assign     = NAME "=" expr
param_type = type ["key" NAME {"," NAME}]        -- on Table parameters only
```

Name resolution: bare names are columns of the current row; a `join` or `nest` binding names the
joined record or nested table; `c.region` as an `item` produces a column named `region`. After
`agg` or `select`, only the listed columns remain. Expressions, types and patterns are plain Yan
(15.2). The final row must match the output record by field name and type; extra columns are an
error that lists them, so a closing `select` is needed only when the row has extras.

### 7.4 Stage meaning, order and lowering

| Stage | Meaning | Output order | Lowers to (11.9, `@yan/frame`) |
| --- | --- | --- | --- |
| `from t` | start | input order | `t.lazy()` |
| `where e` | keep rows | kept rows in order | `filter` with a `ColExpr` |
| `derive a = e` | add columns | unchanged | `with_columns` |
| `select`, `drop` | keep, rename, compute columns | unchanged | `select` |
| `join c = t on p` | keyed lookup, at most one match | left order | `join` (inner); key proof from the parameter type |
| `join opt c = t on p` | optional lookup | left order | left `join`, `c: R \| None` |
| `join many c = t on p` | fan-out | left order, then right order per left row | `join` |
| `nest n = t on p` | group-join | left order | group-join: `group_by` on the right key, left `join`, empty `Table` default |
| `agg ... by k` | one row per key | first appearance of each key | `group_by` and `agg` |
| `agg ...` | one row | one row | `agg` |
| `group k` block | per-group sub-pipeline | groups by first appearance, rows by the block | aggregates: `.over(partition_by=[k])`; `sort`: `order_by`; `take n`: `filter(row_number().over(...) <= n)` |
| `sort` | stable sort | sorted, ties keep order | stable `sort` |
| `take n` | prefix | prefix | `head(n)` |
| `distinct` | first row per key | first occurrence | `group_by` first |
| `append t` | concatenate | left, then right | `concat` |
| `rule` item | fixpoint | first derivation order | `@yan/rel` semi-naive solver |
| `view` item | incremental | as the batch result | `.incremental()` (R625) |

Five facts (N5): failure is `fail QueryError` in the header (overflow, failed strict casts,
`join` key proof failing on a parameter built at run time); `uses none`; no mutation; no visible
concurrency (execution is parallel and deterministic); compile-time evaluation is implicit because
every `query` body is a stage-0 constant plan, compiled by first Futamura projection in release.

One printing (N3): each plan node prints one way. A `group k` block whose only stage is `agg`
prints as `agg ... by k`; there is no `take N per K` shorthand, because the `group` block is its
one printing.

### 7.5 Diagnostics wanted (no codes assigned)

- A column not in the row at that line: list the columns available and the nearest name.
- `join` on a side that is not keyed on the compared column: suggest `nest`, `join many`, or a
  `key` on the parameter.
- An aggregate outside `agg`, a `group` block or a nested table.
- Ordering or arithmetic on an optional column: suggest `else`.
- Output mismatch: list missing and extra columns against the output record.

### 7.6 Changes against RFC 0002 D4

| D4 | Mix |
| --- | --- |
| `group k (aggs)` | `agg aggs by k`; `group k` becomes a block |
| `take N per K` | `group K` block with `sort` and `take N` |
| `join T on P`, `left join` | `join` (keyed), `join opt`, `join many`; `nest` |
| no windows in the first slice | windows come free with the `group` block |
| recursion via `@yan/rel` later | `rule` items in `.yql` |
| incremental later | `view` items |

---

## 8. Open questions

1. Replace D4's `group k (aggs)` with `agg ... by k`, and make `group` a block? (Recommended: yes.)
2. Key proof on `Table` parameters: a new `Table[R] key id` type form, a generalized `Keyed[K, T]`
   (R261), or a run-time uniqueness check at query entry?
3. Should plain `join` fail with `QueryError` when a key has no match (referential integrity, my
   lean) or drop the row like SQL's inner join?
4. Nested `Table` columns inside `LazyFrame[R]` plans: Arrow supports list-of-struct columns; does
   `@yan/frame` carry them, or only rewrite `nest` to a group-join when every use is an aggregate?
5. Measures over any row with `qty` and `price` need row polymorphism, which the core does not have;
   concrete-type measures work today.
6. `rule` items in `.yql` or only through `@yan/rel`; lattice columns (`min`, `max`) beyond those two.
7. Grid literals for examples (`ex late(...) == table` followed by a header line and rows) would
   make query tests readable; that is a plain-Yan literal, not a `.yql` feature.
8. A `yan view --as sql` export to pipe SQL for review and interop, and the reverse import for
   agents fluent in SQL.
9. Measure before adopting the agent claims: run the five tasks and a larger set in SQL, PRQL-shaped
   P3 and the mix on Sonnet and Haiku with the Plan A harness, pass@1 and repair rounds, after the
   M1 runs.
10. Is `view` an item keyword (one printing of an incremental plan) or a call (`q.incremental()`)
    only?
11. Add an `iterate` stage (P3) for transitive closure next to `rule`? It is shorter, but needs a
    termination story (P4's `max N` was declined) and gives recursion two printings.

---

## 9. Sources

- Shute et al., "SQL Has Problems. We Can Fix Them: Pipe Syntax In SQL", PVLDB 17, 2024.
  https://www.vldb.org/pvldb/vol17/p4051-shute.pdf
- Google Cloud, pipe syntax in BigQuery and Cloud Logging.
  https://cloud.google.com/blog/products/data-analytics/simplify-your-sql-with-pipe-syntax-in-bigquery-and-cloud-logging
- Databricks, "SQL gets easier: announcing new pipe syntax", 2025-04-30 (Spark 4.0, DBR 16.2).
  https://www.databricks.com/blog/sql-gets-easier-announcing-new-pipe-syntax
- PRQL status discussion, 2026. https://alt-hn.vercel.app/item/47099980 ; https://github.com/PRQL/prql
- Malloy documentation. https://docs.malloydata.dev/documentation/user_guides/quickstart_modeling
- Lei et al., Spider 2.0, ICLR 2025. https://www.alphaxiv.org/abs/2411.07763v2
- Small language models for KQL generation, arXiv 2512.06660. https://arxiv.org/html/2512.06660v2
- Pipe SQL fine-tuning design doc (projections, not results).
  https://huggingface.co/nittygritty-zzy/pipe-sql-1.5b/blob/main/docs/pipe-sql-fine-tuning-design-doc.md
- Taipalus, Siponen, Vartiainen, "Errors and Complications in SQL Query Formulation", ACM TOCE 2018.
  https://jyx.jyu.fi/handle/123456789/59322
- Cassano et al., "Knowledge Transfer from High-Resource to Low-Resource Programming Languages for
  Code LLMs" (MultiPL-T), OOPSLA 2024. https://arxiv.org/html/2308.09895v3
- Known from the literature, not re-fetched for this study: Codd 1970; Chamberlin and Boyce 1974;
  Date, "A Critique of the SQL Database Language", 1984; Date and Darwen, The Third Manifesto;
  Wadler, "Comprehending Monads", 1990; Buneman et al., "Comprehension Syntax", SIGMOD Record 1994;
  Cheney, Lindley, Wadler, "A Practical Theory of Language-Integrated Query", ICFP 2013; Moerkotte and
  Neumann, "Accelerating Queries with Group-By and Join by Groupjoin", PVLDB 2011; Neumann,
  "Efficiently Compiling Efficient Query Plans", PVLDB 2011; Leis et al., "Morsel-Driven
  Parallelism", SIGMOD 2014; Guagliardo and Libkin, "A Formal Semantics of SQL Queries", PVLDB 2017;
  Madsen, Yee, Lhoták, "From Datalog to Flix", PLDI 2016; Budiu et al., "DBSP", PVLDB 2023; Green,
  Karvounarakis, Tannen, "Provenance Semirings", PODS 2007; Scholak et al., "PICARD", EMNLP 2021;
  Geng et al., "Grammar-Constrained Decoding", EMNLP 2023; Wickham, "Tidy Data", JSS 2014; Brandon,
  "Against SQL", 2021; ISO/IEC 39075:2024 (GQL); ISO/IEC 9075:2023 (SQL, including SQL/PGQ).
