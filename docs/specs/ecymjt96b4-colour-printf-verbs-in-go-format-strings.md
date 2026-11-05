---
task_uid: "01m6j9a04t3cpkzgecymjt96b4"
task: "T443"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "go_printf_highlights::verb_in_errorf_is_a_placeholder: in fmt.Errorf(\"create data dir: %w\", err) the text %w is string.placeholder and \"create data dir: \" stays string"
    required: true
    manual: false
  - id: "C2"
    text: "go_printf_highlights::every_verb_form_is_one_placeholder: %v %d %s %q %x %5.2f %-8s %+v %[1]d %*d each give one string.placeholder span covering the whole verb, and %% gives escape"
    required: true
    manual: false
  - id: "C3"
    text: "go_printf_highlights::printf_family_and_raw_strings_are_covered: Printf, Sprintf, log.Printf, t.Errorf, fmt.Fprintf(w, \"%d\", n) with the format after the writer, and a backquoted raw string all colour their verbs"
    required: true
    manual: false
  - id: "C4"
    text: "go_printf_highlights::non_format_calls_stay_plain: fmt.Println(\"100%d\"), x.Do(\"%d\"), fmt.Printf(format, a) and fmt.Printf(format, \"%d\") with a variable format have no string.placeholder span"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax and cargo clippy -p fe-syntax --all-targets -- -D warnings pass, and cargo deny check licenses passes with tree-sitter-printf in the tree; the go.go.golden diff shows only added placeholder spans"
    required: true
    manual: false
  - id: "C6"
    text: "go_printf_highlights::directives_with_whitespace_stay_plain (the known limit in the spec): fmt.Errorf(\"%w happened\", e) and fmt.Errorf(\"%w: x\", e) have no string.placeholder span and the whole string stays string"
    required: true
    manual: false
---

# Colour printf verbs in Go format strings (%v %d %s %w %q %5.2f %[1]d, %% as an escape) apart from the string, by injecting tree-sitter-printf into Printf-style calls (Mac T1376)

## Problem
The user (2026-11-03, screenshot of Go code with gopls configured): "The formatted strings don't get syntax highlighted". In fmt.Errorf("create data dir: %w", err) the %w is the string colour. GoLand and VS Code colour printf verbs (%v %d %s %w %q %x %5.2f %[1]d %%) apart from the string.

## Why this mechanism
- Tree-sitter captures whole nodes, and tree-sitter-go makes a string one interpreted_string_literal_content node, so only an injected grammar can colour a part of it without a post-pass.
- gopls emits no format-directive semantic tokens (Tenmoku does paint formatSpecifier tokens, which is how rust-analyzer's {} placeholders work, T-spec 74gt9fndhe), and highlighting must not need a language server. So: inject the printf grammar, as nvim-treesitter's go/injections.scm does.
- fe-syntax already has the machinery: injections: Some(include_str!(..)) on a Def, a hidden: true Def for the injected language (rustdoc_inline is the model), injection.language via #set!, and the tree-sitter Rust binding evaluates #any-of? and #match? itself. Inside the layer its captures win and uncaptured bytes keep the string colour.
- The same pair (a printf Def plus a per-host injections query) later serves C and C++ (printf, fprintf, snprintf, syslog: string_literal args) and Python's "..." % args. It does not cover Rust format!, whose {} placeholders rust-analyzer already colours; those need a different grammar. This card is Go only.

## Dependency (needs the user's approval)
New crate tree-sitter-printf 0.5.1 from crates.io (https://github.com/tree-sitter-grammars/tree-sitter-printf), licence ISC (the crate field says ISC; deny.toml already allows ISC), 13 KB, depends only on tree-sitter-language 0.1 like the other grammars (cc build dep). Its dev dep tree-sitter 0.25 is not pulled in. A THIRD_PARTY.md row is required.

## Change
- Cargo.toml (workspace): tree-sitter-printf = "0.5". crates/fe-syntax/Cargo.toml: optional dep, feature printf = ["dep:tree-sitter-printf"]; the go feature becomes go = ["dep:tree-sitter-go", "printf"] so Go never injects into a missing grammar. Do not add printf to the default list by itself (go pulls it in).
- language.rs: a hidden: true Def "printf" (extensions: &[], injections: None, highlights from queries/printf.scm), and the go Def gets injections: Some(include_str!("../queries/go-injections.scm")). Add its row to THIRD_PARTY.md (feature printf, ISC, queries/printf.scm and queries/go-injections.scm, the latter adapted from nvim-treesitter's queries/go/injections.scm, Apache-2.0: write our own patterns rather than copying, and note the source of the idea). Keep the T1358 and T1363 hunks in language.rs untouched.
- queries/printf.scm (ours; upstream's highlights use @embedded/@boolean/@number, which would paint flags and widths in unrelated colours): capture each whole directive as @string.placeholder (read the grammar's node names in the crate's src/node-types.json: the directive node containing flags/width/precision/size/type), and a literal %% as @escape. string.placeholder is already in the vocabulary and every bundled theme colours it apart from string (T 74gt9fndhe); escape likewise.
- queries/go-injections.scm: inject printf (#set! injection.language "printf") into the interpreted_string_literal_content of an interpreted_string_literal argument of a call_expression whose function is a selector_expression field or an identifier whose name is in a list: Printf Sprintf Fprintf Errorf Fatalf Panicf Logf Skipf Warnf Warningf Infof Debugf Errorf Scanf Sscanf Fscanf Appendf, plus any name matching ^[A-Za-z]+f$ only if that stays simple (the #match? form is acceptable and covers log.Printf, t.Errorf, fmt.Fprintf, slog-style wrappers). The format string is the first string argument for Printf-style and the argument after the writer/t for Fprintf, Fatalf-with-t and similar: use a pattern that takes the first interpreted_string_literal argument (anchor with . before it, or one preceded by one other argument for Fprintf/Fscanf/Sscanf). Raw strings (backquote) are included too via raw_string_literal_content. Plain Println-style calls and non-f names must stay uncoloured.
- Regenerate tests/golden/go.go.golden (its sample has fmt.Errorf("add %q: %w", ...) so the golden gains the placeholder spans); review the diff shows only those spans change.

## Tests
New file crates/fe-syntax/tests/go_printf_highlights.rs, #![cfg(all(feature = "go", feature = "printf"))], with the spans() helper from tests/rust_doc_comment_colours.rs (including the take_missing_languages loop, which compiles the injected printf query).
Cases: fmt.Errorf("create data dir: %w", err) gives "%w" string.placeholder and "create data dir: " string; verbs %v %d %s %q %x %5.2f %-8s %+v %[1]d %*d each one placeholder span (flags, width, precision and index inside the verb span); "%%" is escape, not placeholder; fmt.Fprintf(w, "%d items", n) and log.Printf, t.Errorf, fmt.Sprintf all colour; a backquoted raw string in Printf colours; fmt.Println("100%d") and a non-f call such as x.Do("%d") stay plain string; a Printf whose first argument is a variable (fmt.Printf(format, a)) injects nothing; a Go file edited after load (typing a verb) recolours (use the Syntax edit API if the spans() helper style allows, else skip).

Headless; Mac or Linux. Build time: a small new C grammar compiled by cc, as the other grammars.

## Known limit (decided by the Mac lead for the user, 2026-11-05, review F1)
tree-sitter-printf is a C grammar: it lexes `w` and `t` as length modifiers, so `%w` or `%t` followed by more text does not parse as a Go verb. `'%w happened'` matches `%w h` and `'%w: x'` has no directive. This card does not fix that. printf.scm must not colour any directive containing whitespace (for example `(#not-match? @string.placeholder "\\s")`), so prose is never coloured as a placeholder. As a result `%w` and `%t` followed by text, and `% d`, stay plain string. T450 replaces this with a Go verb scanner. `%w` or `%t` at the end of a string still colour (C1).

F2 is still to be fixed: each function family takes its format at one argument position, so `fmt.Printf(format, "%d")` injects nothing (C4).
