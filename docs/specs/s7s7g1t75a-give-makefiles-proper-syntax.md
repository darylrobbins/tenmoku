---
task_uid: "01m6j4arjbqhjmrqs7s7g1t75a"
task: "T1358"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "make_highlights::target_and_prerequisites_are_coloured: in a Makefile with \"build: web\" and \"test: test-go test-web\", the target build is highlighted function and the prerequisites web, test-go, test-web carry a distinct non-plain capture; .PHONY is coloured as a special target (keyword)"
    required: true
    manual: false
  - id: "C2"
    text: "make_highlights::variables_and_references_are_coloured: BINARY in \"BINARY := app\" is variable, \"$(BINARY)\" in a recipe has its name coloured and its \"$(\" and \")\" punctuation.special, $@ $\u003c $^ are variable.builtin, the names in $(shell ...) and $(wildcard ...) are function, := is operator, and a ## comment is comment"
    required: true
    manual: false
  - id: "C3"
    text: "make_highlights::directives_and_recipe_prefixes_are_coloured: include, ifeq, ifdef, else, endif, define, endef and export are keyword, and the leading @, - and + of a recipe line are operator"
    required: true
    manual: false
  - id: "C4"
    text: "make_highlights::recipe_bodies_highlight_as_bash: in @echo \"Go API on :8080\" the quoted text is string; in \"go run ./cmd \u0026\" and \"npm ci || npm install\" the || and \u0026 are operator as bash colours them; in a recipe with a trailing backslash continuation the next physical line is still highlighted as bash"
    required: true
    manual: false
  - id: "C5"
    text: "make_highlights::double_dollar_variable_is_bash: in \"$${ADMIN_PASSWORD:-dev}\" the name ADMIN_PASSWORD is not plain (a variable colour), the $$ is not coloured as a make variable reference, and a make reference $(BINARY) on the same line keeps its make colours"
    required: true
    manual: false
  - id: "C6"
    text: "outline_scripts::make_lists_targets_and_defines_but_not_special_or_pattern_targets still passes unchanged; cargo nextest run -p fe-syntax passes; cargo clippy -p fe-syntax --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Give Makefiles proper syntax highlighting: target names, prerequisites, special targets, variables, automatic variables, functions and directives from our own query, and bash inside recipe lines

The user (2026-11-03, with a screenshot of a Makefile in Tenmoku): "Makefile formatting is kind of lame". Today only .PHONY, variable names, :=, @, $(BINARY) and ## comments are coloured: make uses tree-sitter-make 1.1.1's minimal upstream HIGHLIGHTS_QUERY and `injections: None` (crates/fe-syntax/src/language.rs, make's Def). Target names, prerequisites and whole recipe bodies are plain.

Do:
- Write crates/fe-syntax/queries/make.scm, modelled on nvim-treesitter's and Helix's make highlights, adapted to tree-sitter-make 1.1.1's node types (see its node-types.json), using only capture names highlight.rs maps (function, keyword, operator, punctuation.special, variable, variable.builtin, variable.parameter, string, string.escape, comment, constant…; upstream's legacy @conditional/@include/@repeat alias to keyword). Cover: rule targets (`rule` > `targets`) as function, prerequisites (`prerequisites`, normal and order_only) as a distinct non-plain capture, special targets (.PHONY, .SUFFIXES, .DEFAULT …), variable assignment names and assignment operators, variable_reference / substitution_reference (name coloured, `$(`/`)` punctuation.special), automatic_variable as variable.builtin, function_call / shell_function names as function, directives (include, ifeq/ifneq/ifdef/ifndef/else/endif, define/endef, export/unexport, override, vpath) as keyword, recipe prefixes @ - + as operator, the `$$` escape as string.escape.
- Write crates/fe-syntax/queries/make-injections.scm injecting bash into recipe shell text: `recipe_line` holds a prefix then one or more `shell_text` siblings split at `\` continuations, so use `injection.combined` (combined per parent recipe_line, as injection.rs does). Named children of shell_text (variable_reference, function_call, automatic_variable, `$$` escape …) stay out of the bash layer by default; keep it that way so make references keep make colours. Mind that removing `$$` leaves `{X:-dev}` in the bash text: the criteria require `$${ADMIN_PASSWORD:-dev}` to still read as a variable — if the bash parse can't give that, colour it from the make query instead (e.g. capture the escape plus following braces), but the name must not be plain.
- Wire them in language.rs as rust does: `highlights: || &[include_str!("../queries/make.scm")]`, `injections: Some(include_str!("../queries/make-injections.scm"))`. bash is a default fe-syntax feature.
- Don't touch queries/outline/make.scm or queries/definitions/make.scm.

Tests in the new file crates/fe-syntax/tests/make_highlights.rs (`#![cfg(all(feature = "make", feature = "bash"))]`), copying the spans() helper pattern from crates/fe-syntax/tests/rust_doc_comment_colours.rs (Syntax::new, parse_job, highlights, take_missing_languages loop, language_by_name for injected languages). Fixture like the user's: .PHONY line, `BINARY := bin/app`, dev/web/build/test targets, `build: web`, `test: test-go test-web`, `@echo "Go API on :8080"`, `@( SEED=1 ADMIN_PASSWORD=$${ADMIN_PASSWORD:-dev} go run ./cmd/server & \` continued over two more lines, `npm --prefix web ci || npm --prefix web install`, `$(BINARY)` in a recipe.

Headless; Mac or Linux.

## Implementation notes

- Upstream bash highlights colour `&&` and `|` but not `||` or `&`, which C4
  needs, so language.rs appends `["||" "&"] @operator` to bash's highlights;
  bash.sh.golden gains those two spans.
- make.mk.golden and the make snippet in src/tests.rs were regenerated: names
  are now `variable`, not `constant`/`constant.builtin`.
- `$$` is the `escape` node. The text after it (`{X:-dev}`) is part of an
  anonymous token, so no make query can reach it, and without the `$` bash
  parses it as a `concatenation` of plain words. language.rs therefore also
  appends a bash rule colouring a concatenation shaped exactly `{NAME}` or
  `{NAME:-x}` as `variable`. In ordinary shell scripts that shape is a literal
  word that is rarely written, so goldens did not change.
