# Hostile grammars

Grammars of ours that misbehave, for `tests/runtime_grammar.rs`. They're this
repository's own code, under its licence.

- `loop`: its external scanner never returns
  (`a_looping_grammar_is_stopped_at_its_deadline`,
  `a_looping_injection_is_stopped_at_its_deadline`).
- `loopinit`: a constructor never returns, so its module initializer doesn't.
- The rest are for `hostile_grammars_are_errors_not_crashes`:
  - `lexnull`: a constructor calls the host's `eof` lexer callback while no
    lexer runs.
  - `debugmsg`: a constructor passes `tree_sitter_debug_message` a string past
    the end of its memory.
  - `badscan`, `badlex` and `badkeyword`: the external scanner, the main lexer
    and the keyword lexer each return token 60000, which the grammar doesn't
    have.

Each was generated with the tree-sitter CLI 0.27.0 (its release binary):
`tree-sitter generate` in a directory holding the `grammar.js` (and
`src/scanner.c`) below. After that, `badlex`'s `src/parser.c` has
`ACCEPT_TOKEN(sym_word)` in `ts_lex` changed to `ACCEPT_TOKEN(60000)`, and
`badkeyword`'s has `ACCEPT_TOKEN(sym_let)` in `ts_lex_keywords` changed the
same way. `loop` was compiled with `tree-sitter build --wasm -o
tree-sitter-loop.wasm .`. The others were compiled by `build.sh` below, from
the parent directory (`sh build.sh loopinit lexnull …`), with the wasi-sdk 34
clang the CLI fetched. `build.sh` passes the same flags as `tree-sitter build`,
but also exports `__wasm_call_ctors`, without which a constructor wouldn't run,
and puts `scanner.c` first, which `lexnull` relies on. (`loopinit` was built
with `parser.c` first, before that change; its order doesn't matter.)

SHA-256:

```
b13f2e191c83b8adf7d601db152466779d6e175f5c4b36331520a689c5900ab3  tree-sitter-loop.wasm
34e50617e6dc1557d188942287c119b5caf7aa64e614137076e08a80cb881d51  tree-sitter-loopinit.wasm
903297137434946e6ada87ca7db48ce7b92811e959b70c10b161720872759af6  tree-sitter-badscan.wasm
8af45429a681b37d305df8488fbbfac9c83d77fd563df80238b5253244e75515  tree-sitter-badlex.wasm
d148d54432a3eaaeab955166671360ff4512911c74a532048cf233ecee913935  tree-sitter-badkeyword.wasm
fd92d09dd046cd14e7048e01aa3f4d40c0b023b5f995982c579adc17cbdaa150  tree-sitter-lexnull.wasm
855f03b30c3e6782edb8215ed9fedbde70b01b59e23e9c7b6d2c5cbb863448f0  tree-sitter-debugmsg.wasm
```

## build.sh

```sh
#!/bin/sh
# Builds each grammar directory named on the command line, after
# `tree-sitter generate` (and any patch to its parser.c), into
# tree-sitter-<name>.wasm, as `tree-sitter build --wasm` does, exporting
# `__wasm_call_ctors` too where a constructor needs to run.
set -eu
W=${WASI_SDK:-$HOME/.cache/tree-sitter/wasi-sdk}
for g in "$@"; do
  # scanner.c first: its functions then take the first table slots.
  srcs=src/parser.c
  [ -f "$g/src/scanner.c" ] && srcs="src/scanner.c $srcs"
  (cd "$g" && "$W/bin/clang" --target=wasm32-wasip1 --sysroot="$W/share/wasi-sysroot" \
    -o "../tree-sitter-$g.wasm" -fPIC -shared -Os \
    -Wl,--export="tree_sitter_$g" -Wl,--export=__wasm_call_ctors \
    -Wl,--allow-undefined -Wl,--no-entry -nostdlib -fno-exceptions \
    -fvisibility=hidden -I src $srcs)
done
```

## loop

`grammar.js`:

```js
module.exports = grammar({
  name: 'loop',
  externals: $ => [$.spin],
  rules: {
    source: $ => repeat(choice($.spin, $.word)),
    word: _ => /[a-z]+/,
  },
});
```

`src/scanner.c`:

```c
#include "tree_sitter/parser.h"

void *tree_sitter_loop_external_scanner_create(void) { return NULL; }
void tree_sitter_loop_external_scanner_destroy(void *payload) {}
unsigned tree_sitter_loop_external_scanner_serialize(void *payload, char *buffer) { return 0; }
void tree_sitter_loop_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {}

// Never returns: the parse's deadline has to stop it.
bool tree_sitter_loop_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  for (;;) {
    lexer->mark_end(lexer);
  }
}
```

## loopinit

`grammar.js`:

```js
module.exports = grammar({
  name: 'loopinit',
  rules: {
    source: $ => repeat($.word),
    word: _ => /[a-z]+/,
  },
});
```

`src/scanner.c`:

```c
// No external tokens: only a constructor, which never returns, so the
// module's initializer has to be stopped by the load's deadline.
static volatile unsigned spins;

__attribute__((constructor, used)) static void spin(void) {
  for (;;) {
    spins++;
  }
}
```

## badscan

`grammar.js`:

```js
module.exports = grammar({
  name: 'badscan',
  externals: $ => [$.token],
  rules: {
    source: $ => repeat(choice($.token, $.word)),
    word: _ => /[a-z]+/,
  },
});
```

`src/scanner.c`:

```c
#include "tree_sitter/parser.h"

void *tree_sitter_badscan_external_scanner_create(void) { return NULL; }
void tree_sitter_badscan_external_scanner_destroy(void *payload) {}
unsigned tree_sitter_badscan_external_scanner_serialize(void *payload, char *buffer) { return 0; }
void tree_sitter_badscan_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {}

// Finds a token that isn't one of the grammar's external tokens.
bool tree_sitter_badscan_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  lexer->result_symbol = 60000;
  return true;
}
```

## badlex

`grammar.js`:

```js
module.exports = grammar({
  name: 'badlex',
  rules: {
    source: $ => repeat($.word),
    word: _ => /[a-z]+/,
  },
});
```

## badkeyword

`grammar.js`:

```js
module.exports = grammar({
  name: 'badkeyword',
  word: $ => $.identifier,
  rules: {
    source: $ => repeat(choice($.let, $.identifier)),
    let: _ => 'let',
    identifier: _ => /[a-z]+/,
  },
});
```

## lexnull

`grammar.js`:

```js
module.exports = grammar({
  name: 'lexnull',
  externals: $ => [$.token],
  rules: {
    source: $ => repeat(choice($.token, $.word)),
    word: _ => /[a-z]+/,
  },
});
```

`src/scanner.c`:

```c
#include <stdint.h>
#include "tree_sitter/parser.h"

void *tree_sitter_lexnull_external_scanner_create(void) { return NULL; }
void tree_sitter_lexnull_external_scanner_destroy(void *payload) {}
unsigned tree_sitter_lexnull_external_scanner_serialize(void *payload, char *buffer) { return 0; }
void tree_sitter_lexnull_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {}
bool tree_sitter_lexnull_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  return false;
}

// The store puts its lexer callbacks (advance, mark_end, get_column,
// is_at_included_range_start, eof) in the function table just below this
// module's functions. Compiled before parser.c, this file's functions come
// first among them, so `eof` is the entry below the lowest, called here, at
// load, while no lexer runs. (The language's own pointers can't be used: the
// constructor may run before the data relocations that fill them in.)
__attribute__((constructor, used)) static void call_eof(void) {
  // Stored one by one, in code: an initialized array would be copied from
  // data the relocations haven't reached yet.
  volatile uintptr_t pointers[5];
  pointers[0] = (uintptr_t)&tree_sitter_lexnull_external_scanner_create;
  pointers[1] = (uintptr_t)&tree_sitter_lexnull_external_scanner_destroy;
  pointers[2] = (uintptr_t)&tree_sitter_lexnull_external_scanner_serialize;
  pointers[3] = (uintptr_t)&tree_sitter_lexnull_external_scanner_deserialize;
  pointers[4] = (uintptr_t)&tree_sitter_lexnull_external_scanner_scan;
  uintptr_t base = pointers[0];
  for (unsigned i = 1; i < 5; i++) {
    if (pointers[i] < base) base = pointers[i];
  }
  bool (*eof)(const TSLexer *) = (bool (*)(const TSLexer *))(base - 1);
  eof(0);
}
```

## debugmsg

`grammar.js`:

```js
module.exports = grammar({
  name: 'debugmsg',
  rules: {
    source: $ => repeat($.word),
    word: _ => /[a-z]+/,
  },
});
```

`src/scanner.c`:

```c
// The host prints a debug message's string, which here starts past the end
// of the module's memory.
void tree_sitter_debug_message(const char *message, unsigned value);

__attribute__((constructor, used)) static void debug(void) {
  tree_sitter_debug_message((const char *)0xfffffff0, 1);
}
```
