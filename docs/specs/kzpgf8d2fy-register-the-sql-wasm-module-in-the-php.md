---
task_uid: "01m6pgxnyp2zsn7akzpgf8d2fy"
task: "T1386"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/build-grammar-wasm.sh builds all eight grammars and its wasm_parity run passes all eight tests (kotlin, csharp, scala, swift, cpp, sql, php, ruby), including php.php.golden line 218"
    required: true
    manual: false
  - id: "C2"
    text: "The nextest call at the end of scripts/build-grammar-wasm.sh passes --no-fail-fast; crates/fe-syntax/tests/wasm_parity.rs registers sql under its real name for the php test and its header comment says why"
    required: true
    manual: false
---

# Register the sql wasm module in the php wasm parity test so the heredoc SQL injection resolves, and run the whole parity suite with --no-fail-fast (grammars-1 run 42087698315)

The first run of .github/workflows/grammars.yml (run 42087698315, tag grammars-1, commit 16f85127) failed in "Build the grammars and check them against the golden files": `wasm_parity php` panicked at crates/fe-syntax/tests/wasm_parity.rs:126, "php's wasm differs from php.php.golden at line 218":

    - 89:11-90:5 string "<<<SQL\n    "
    + 89:11-91:40 string "<<<SQL\n    SELECT id, name -- newest first\n    FROM tasks ORDER BY id DESC LIMIT {"

The golden has the heredoc split around an injected SQL body; the wasm run left it one string, so nothing was injected. Ruby and Scala passed; the run is fail-fast, so two of the eight were never run.

## Cause

Only the test harness, not the shipped grammar. extensions/grammars/php/injections.scm (identical to crates/fe-syntax/queries/php-injections.scm) injects a heredoc's body into the language named by its closing tag, `(heredoc_end) @injection.language`, so `<<<SQL` asks for the language "sql". Since T1322 sql is not a built-in language (not a default feature of fe-syntax), so the golden test for php (src/tests/golden.rs, built with all features) finds it built in. wasm_parity.rs `load` (line 51) registers only the one module under test, as `php_wasm`; `render` (line 86) resolves missing languages with `fe_syntax::language_by_name`, which finds nothing for "sql" (crates/fe-syntax/src/language.rs:196, falling back to runtime::by_name), so the heredoc stays one string. A user with the PHP and SQL extensions installed gets the injection through the same loader path. (A user with PHP but no SQL extension gets none, as for any injected language that isn't installed.)

The other seven languages' injections.scm files hold no injection that names another language, so php is the only one that depends on a second wasm-only grammar.

## Do

1. In crates/fe-syntax/tests/wasm_parity.rs, make the php parity test also register the sql module (built by scripts/build-grammar-wasm.sh) under its real name "sql", from extensions/grammars/sql, so the injection resolves. `load` registers every language as `<name>_wasm` because a built-in language refuses its name; a language injected by name needs its own name, so add a variant for it (skip registering if `language_by_name("sql")` already finds a built-in, so an all-features run still works). Keep the `sql` test's own `sql_wasm` registration as it is.
2. In scripts/build-grammar-wasm.sh, make the php run build sql too when only php is named (the module is a prerequisite), and pass `--no-fail-fast` to the nextest call at the end so one failing language doesn't hide the rest.
3. Say in the doc comment at the top of wasm_parity.rs that a language whose injections name another wasm-only language must register that one too.

Out of scope: any change to the php queries, the goldens, other extensions, the workflow, or fe-syntax src.

## Verify

The tests are ignored and need the built modules. wasi-sdk is at ~/.cache/tree-sitter/wasi-sdk, the script's default, so the Mac lane can run it:

    scripts/build-grammar-wasm.sh

builds all eight and runs the whole parity suite; all eight must pass. After the fix, re-run the grammars workflow by hand from a new tag to see it green (human step, not a criterion).

No regression test can be added in the gate: the parity tests only run with the wasm built, and the criterion above is how they are checked.
