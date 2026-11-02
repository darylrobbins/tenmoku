# INI, dotenv and just (T1298)

- `ini` is crates.io `tree-sitter-ini` 1.4.0, `just` is vendored at `5685543` (`vendor/tree-sitter-just`, shaped like
  T1282's gitignore crate, with `scanner.c` too; ABI 15, not regenerated), each behind a default Cargo feature. `dotenv`
  has no grammar or feature of its own: it reuses bash's under `bash`.
- Size: 66,368 bytes of release binary, measured on a small example linking `fe-syntax` (release profile, macOS arm64)
  built with all default features and again without `ini` and `just`; the spec's estimate was about 44 KB of object
  code. `dotenv` adds a table entry and a query string only.
- `FILE_NAME_PREFIXES` is checked after a registered language's name and extension too, not only the built-in ones, so
  an extension's explicit `.env.foo` claim beats the prefix. It ignores ASCII case, as file names do here.
- `dotenv.scm` paints more than the spec's `word`: bash reads `PORT=8080` as a `number`, which bash's query leaves plain,
  and `URL=http://${HOST}/x` as a `concatenation` whose pieces it also leaves plain. Both are unquoted values, so both
  are `string`. `${VAR:-default}` keeps bash's colours.
- Limits of the ini grammar, accepted: a `key: value` line, and a last line with no line break, parse as errors, though
  the tree is still there. A `key: value` line takes the next line's key with it, and a last comment with no line break
  stays plain. The grammar's comment token includes its line break and a value includes the spaces after `=`, which
  shows in spans but not on screen.
- Limits of just's injection query: `set shell := ["zsh", "-c"]` doesn't change a recipe's language. The query captures
  the string with its quotes, which names no language, so upstream's default of bash stays. A recipe's `@` is read by
  bash as part of the command name.
- Left default: the parse-limit ratio (40) for `ini` and `just`; `dotenv` also takes 40, not bash's 45. `just` folds `recipe`; `ini` and `dotenv` fold nothing.
- `cargo deny check licenses bans sources` passes, which is what the gate's `deny` stage runs.
- Runtime change (respec rev 2): T235's accepted-stack rule in `vendor/tree-sitter/src/parser.c` rejected a valid ini
  parse, leaving no tree for any file with a blank line before its first setting or section. `_blank` is a non-terminal
  extra that `document`'s repeat also uses, so it is reduced with a real lookahead and isn't flagged extra, which leaves
  `[_blank, document, eof]` on the stack. Stock 0.27.0 parses these files. `ts_parser__accept` now also admits a non-extra
  tree below the root when it has children and state 1 goes back to itself on its symbol: everything below the root was
  pushed from state 1, so only self-loops get through. Flagging such a tree extra in `ts_parser__reduce` was rejected,
  since a right-recursive rule can self-loop too and stock's `extra` flag would change for it. A corrupt grammar can use
  the exception to leave such a tree beside its root, which is harmless: the root's structural children are still
  bounded by its alias sequences. A root with a nonzero production and many such trees could fail that bound and lose its
  tree; ini's root has production 0.
