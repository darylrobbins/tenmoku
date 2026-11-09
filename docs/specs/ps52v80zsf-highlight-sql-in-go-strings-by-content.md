---
task_uid: "01m6z6nman6epcb5ps52v80zsf"
task: "T335"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "screenshot_statements_inject_sql: the spec's fixture (database.Exec with a raw INSERT INTO … VALUES string, database.Get(\u0026got, raw SELECT … FROM … JOIN … WHERE b.title = ?, tc.title)) gives exactly two injections, both \"sql\", whose content is the two raw strings' text; runs in the default build (query-level, no sql feature)"
    required: true
    manual: false
  - id: "C2"
    text: "every_statement_form_injects: SELECT … FROM over several lines, INSERT INTO, UPDATE t SET, DELETE FROM, WITH x AS (, WITH RECURSIVE x(n) AS (, CREATE TABLE, CREATE UNIQUE INDEX, ALTER TABLE, DROP VIEW, SELECT 1, SELECT count(*), lower-case select id from t, and an sqlc string starting -- name: GetUser :one then SELECT … FROM, each inject \"sql\" as a raw string and (where it has no escape) as an interpreted string, in each position: call argument (db.Exec(q), db.QueryRowContext(ctx, q, id), db.Get(\u0026x, q, a), conn.Query(ctx, q)), const, var, :=, =, return and a struct-literal field value"
    required: true
    manual: false
  - id: "C3"
    text: "english_strings_stay_plain: \"update the cache\", \"delete failed\", \"select a file\", \"Select a file from the list\", \"Update the cache and set the flag\", \"insert coin\", \"with care as always\", \"drop it\" and \"Create table later\" inject nothing, as call arguments (log.Println, errors.New) and as const values"
    required: true
    manual: false
  - id: "C4"
    text: "printf_format_strings_stay_printf: fmt.Sprintf(\"SELECT * FROM %s WHERE id = ?\", t), fmt.Sprintf(`SELECT a FROM %s`, t) and fmt.Fprintf(w, \"DELETE FROM %s\", t) inject only \"printf\", never \"sql\"; and, with the sql feature, the first keeps %s as string.placeholder and SELECT stays string"
    required: true
    manual: false
  - id: "C5"
    text: "escaped_strings_stay_plain: \"SELECT a\\nFROM t\" (an escape sequence) and the pieces of \"SELECT a \" + \"FROM t\" inject nothing"
    required: true
    manual: false
  - id: "C6"
    text: "screenshot_statements_highlight_as_sql (sql feature): in the fixture INSERT, INTO, VALUES, SELECT, FROM, JOIN, ON and WHERE are keyword spans, ? is variable.parameter, 'Ann' is a string, and database, Exec, Get and tc.title keep their Go highlights"
    required: true
    manual: false
  - id: "C7"
    text: "placeholders_keep_the_statement_highlighted (sql feature): in SELECT * FROM t WHERE id = ? AND x = ?, … = $1 AND x = $2, … = @p1 AND x = @name and … = :id AND x = :name, SELECT, FROM, WHERE and AND are keyword spans; ? and $1 are variable.parameter"
    required: true
    manual: false
  - id: "C8"
    text: "sql_strings_stay_strings_without_sql (default build, cfg(not(feature = \"sql\"))): the fixture's raw strings are one string span each, take_missing_languages() is empty, and highlighting does not panic"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-syntax --features sql --test go_sql_injections, cargo nextest run -p fe-syntax and cargo clippy -p fe-syntax --all-targets --features sql -- -D warnings pass; go_printf_highlights passes unchanged and tests/golden/go.go.golden is unchanged"
    required: true
    manual: false
---

# Highlight SQL in Go strings by content: inject the sql grammar into raw and escape-free strings that read as a statement (SELECT … FROM, INSERT INTO, UPDATE … SET, DELETE FROM, WITH … AS (, CREATE/ALTER/DROP TABLE|INDEX|VIEW), leaving English text and printf format strings as they are (Mac T1642)

## Problem
SQL in Go string literals renders as plain string colour. A user's Go test passes raw strings to `database.Exec(` … `)` and to a sqlx-style `database.Get(&got, ` … `, tc.title)`:

```go
if _, err := database.Exec(`
	INSERT INTO books (title, category) VALUES
		('Dune', 'Science Fiction'),
		('The Go Programming Language', 'Programming'),
		('Stew', 'Cookery'),
		('Uncategorised', '')`); err != nil {
	t.Fatalf("seed pre-migration books: %v", err)
}
var got string
err := database.Get(&got, `
	SELECT c.name FROM books b JOIN categories c ON c.id = b.category_id
	WHERE b.title = ?`, tc.title)
```

Wanted: the statements coloured as SQL (keywords, identifiers, literals, `?` as a parameter), as GoLand and nvim-treesitter do. That fixture (the screenshot's two statements, verbatim; note the t.Fatalf format string beside them must stay printf) is the first test case; the sqlx named-parameter fixture below is the second.

## Change
Only crates/fe-syntax/queries/go-injections.scm and one new test file. No src change: no injection.rs, language.rs or Cargo.toml edit.

### How it resolves
Add patterns to go-injections.scm that set `injection.language "sql"` on Go string content, as the printf patterns there set "printf" and as php-injections.scm's heredocs name "sql" (T1386). `injected_language("sql")` (src/language.rs ~329) finds the built-in `sql` Def when the `sql` cargo feature is on (language.rs ~845), else a registered runtime language (the extensions/grammars/sql wasm extension, through `runtime::injected`), else returns `Injected::Unknown`, and the string stays a plain string with no error and no missing-language report. Nothing to register. A user with the SQL extension installed gets the colouring; one without it sees no change.

### Where a string injects (by position)
Detection goes by content, not method name, so database/sql, sqlx, pgx, sqlc output and custom wrappers like `database.Get` all work. A string injects in these positions only:
1. An argument of a call whose function, an `identifier` or a `selector_expression`'s `field_identifier` captured as `@_fn`, satisfies `(#not-match? @_fn "^[A-Za-z]+f$")`. Every function the two printf patterns accept ends in `f`, so a printf format string never gets a second, SQL layer. Two layers over the same bytes would paint in tree-sitter's match order, which we don't control (`Painter::paint` overlays each injection in turn). So `fmt.Sprintf("SELECT * FROM %s WHERE id = ?", t)` stays printf only, with verbs coloured and the rest string, exactly as today. Any argument position counts: `db.QueryRowContext(ctx, q, id)`, `db.Get(&x, q, a)`, `conn.Query(ctx, q)`.
2. A value in an `expression_list` (const, var, `:=`, `=` and return).
3. A `literal_element` (a struct or slice literal's field value, as in test tables).

Strings in other positions stay plain.

### Which string forms
- **Raw strings:** `(raw_string_literal (raw_string_literal_content) @injection.content)`.
- **Interpreted strings with no escape:** `(interpreted_string_literal . (interpreted_string_literal_content) @injection.content .)`. Anchored on both sides, this matches only a string whose content is one piece. A string with an escape sequence (`"SELECT a\nFROM t"`) stays plain. Its pieces would join as "SELECT aFROM t" without `injection.include-gaps`, and with it the layer would get the escape's backslash.
- **Concatenations:** each piece of `"SELECT a " + "FROM t"` is matched on its own text, so a piece injects only if it reads as a statement by itself.

### Which content injects
Use `(#match? @injection.content "<regex>")`. Its dialect is Rust's `regex` crate: vendor/tree-sitter/binding_rust/lib.rs builds `regex::bytes::Regex::new` for `#match?` and runs `is_match` on the whole node text. So `(?i)`, `(?s)`, `\A`, `\b` and `\s` (which matches newlines) all work, and `^` is the start of the text unless `(?m)` is set.

The query parser (vendor/tree-sitter/src/query.c, string literals) keeps only `\n \r \t \0` as escapes and drops the backslash from any other escape. Write `\\s` and `\\b` in the .scm, as extensions/grammars/svelte/injections.scm does. A bare `\s` would become `s`.

The content matches when, after leading whitespace and any number of leading `--` comment lines (sqlc's `-- name: GetUser :one`), it starts with a statement in SQL shape. Both keywords must have the same case: both upper or both lower. That rejects sentence-case English ("Select a file from the list", "Update the cache and set …") while accepting `SELECT … FROM` and `select … from`. The statement shapes:
- `SELECT … FROM` (any text between, across lines)
- `INSERT INTO`
- `UPDATE <one token> SET`
- `DELETE FROM`
- `WITH [RECURSIVE] <name> [(<cols>)] AS (`. The `(` after AS is required, so English "with care as always" doesn't match.
- `CREATE|ALTER|DROP [UNIQUE] TABLE|INDEX|VIEW` (also `CREATE UNIQUE INDEX`)
- A bare upper-case `SELECT` followed by a digit, `*`, `'`, `(` or a call such as `count(` or `now(`. This covers `SELECT 1` and `SELECT count(*)`. Lower-case `select` needs `from`.

You may tighten these shapes. You may not loosen them past the C3 negatives below.

### Credit
The file's header comment already says "Ours" and credits nvim-treesitter's queries/go/injections.scm (Apache-2.0) for the printf idea. Extend it: injecting SQL into Go strings by their content is also an idea nvim-treesitter's Go injections use, and the patterns and regex are written afresh. Keep "Ours." as the first line (crates/fastedit/tests/third_party.rs reads query headers). Add a comment above the SQL patterns covering three points:
- why the call pattern excludes `…f` functions (one layer per string)
- why interpreted strings with escapes stay plain
- the case rule

### Placeholders (checked against tree-sitter-sequel 0.3.11, the crate both the feature and the wasm build from)
- `?` and `$1` parse as `(parameter)`, highlighted `@parameter`, which maps to `variable.parameter`.
- `@p1` and `@name` parse as plain identifiers in a `field`.
- sqlx's `:name` leaves a one-character `(ERROR)` at the `:`. The rest of the statement still parses: `WHERE id = :id AND name = :name` keeps WHERE and AND as keywords (the last `name` lands in an ERROR as `keyword_name`).

No query change is needed for any of them. The tests pin that keywords around each placeholder still colour.

### Goldens
Leave tests/golden/go.go and go.go.golden alone. Adding SQL there would make the golden depend on the `sql` feature.

### Second fixture: sqlx named parameters (user's screenshot, 11-09)
The user also wants this case covered: a multi-line raw string passed to sqlx's `NamedExecContext`, opening with a newline and indentation, an `INSERT INTO` with a parenthesised column list and a `VALUES (...)` list of `:name` parameters. Use it verbatim as the second fixture (tabs as shown):

```go
func (r *Repo) Create(ctx context.Context, b *Book) error {
	if b.LendingStatus == LendingOnLoan {
		return fmt.Errorf("cannot create a book with lending status %q: a create has no patron to lend it to", LendingOnLoan)
	}
	return r.inTx(ctx, func(tx *sqlx.Tx) error {
		res, err := tx.NamedExecContext(ctx, `
			INSERT INTO books (
				title, isbn10, isbn13, publisher, published_year, format, series,
				series_index, page_count, cover_path, reading_status,
				lending_status, condition, rating, started_at, finished_at, notes,
				location_id, language, edition, acquired_source, acquired_date, acquired_price
			) VALUES (
				:title, :isbn10, :isbn13, :publisher, :published_year, :format, :series,
				:series_index, :page_count, :cover_path, :reading_status,
				:lending_status, :condition, :rating, :started_at, :finished_at, :notes,
				:location_id, :language, :edition, :acquired_source, :acquired_date, :acquired_price
			)`, b)
		if err != nil {
			return fmt.Errorf("insert book: %w", err)
		}
		return nil
	})
}
```

Required:
- The raw string injects `sql` (query-level test), and both `fmt.Errorf` strings stay printf only (`insert book: %w` must not inject: lower-case `insert` with no `into`).
- With `sql` on: `INSERT`, `INTO` and `VALUES` colour as keywords, and every one of the 23 `:name` parameters colours the same way as the others, with no run of the column list or the VALUES list falling into a large `(ERROR)` that paints keywords or punctuation wrongly. The spec above checked `:name` only in a WHERE clause; a VALUES list of 23 is the case to prove. Columns that tree-sitter-sequel itself treats as keywords (e.g. `format`, `condition`, `language`) may colour as keywords, as they would in a .sql file: pin what it does, don't fight it.
- If sequel's error recovery does break this statement and fixing it needs a change outside go-injections.scm (e.g. the sql highlights query), block the card and tell the lead, who will respec; don't widen scope yourself.

## Tests
New file crates/fe-syntax/tests/go_sql_injections.rs, `#![cfg(feature = "go")]`.

**Query-level tests, `#[cfg(feature = "go")]` only.** They run in the gate's default build, which has no `sql`. Each test:
1. Compiles `include_str!("../queries/go-injections.scm")` as a `tree_sitter::Query` for `tree_sitter_go::LANGUAGE`. Both are fe-syntax dependencies, usable from its integration tests.
2. Runs it with a `QueryCursor` over a Go snippet, passing the text so `#match?` is evaluated.
3. Collects each match's `@injection.content` text with the language its pattern's `property_settings` set.

Write one helper, `injected(go_src) -> Vec<(String, String)>`, returning (content text, language) pairs.

**Highlight tests, `#[cfg(feature = "sql")]`.** Use the spans() helper of tests/go_printf_highlights.rs, with its `take_missing_languages` loop.

**Default-build test, `#[cfg(not(feature = "sql"))]`.** The no-sql behaviour.

## Out of scope
- Strings with escapes, and statements split across a `+`.
- printf format strings that hold SQL. Those stay printf; a second, SQL layer under the verbs would need deterministic layer order in injection.rs.
- Other host languages (Python, Java, TS, Rust).
- Go's golden sample.
- The wasm parity test.
