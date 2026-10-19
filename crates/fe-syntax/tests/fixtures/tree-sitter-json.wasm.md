# tree-sitter-json.wasm

The JSON grammar of [tree-sitter-json](https://github.com/tree-sitter/tree-sitter-json)
0.24.8, the version `Cargo.lock` has, compiled to WebAssembly for
`tests/runtime_grammar.rs`, which loads it at run time and compares its
highlights with the compiled-in JSON grammar's. Its highlight query is the
crate's `HIGHLIGHTS_QUERY`, which the test writes out beside it.

Built from the crate's published source (`grammar.js`, `src/parser.c`) with the
tree-sitter CLI 0.27.0 (its release binary, which fetched wasi-sdk 34 and
binaryen 132 itself):

```sh
cp -R ~/.cargo/registry/src/*/tree-sitter-json-0.24.8 json
cd json
tree-sitter build --wasm -o tree-sitter-json.wasm .
```

SHA-256 `564e489724cbcf9b4563cd758a7fb7f355f896b11127819ae9e8ef71e7c15e60`.

## Licence

The MIT License (MIT)

Copyright (c) 2014 Max Brunsfeld

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
