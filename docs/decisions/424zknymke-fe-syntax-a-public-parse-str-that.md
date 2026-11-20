# fe-syntax `parse_str` (T706)

`fe_syntax::parse_str(&Language, &str, out_of_time) -> Option<Tree>` parses a
string on the calling thread through `ThreadParser::for_language` and
`syntax::parse_text`, so WebAssembly grammars, grammar deadlines and panic
handling behave as for a `Syntax` parse. `None` when the callback stopped the
parse, a deadline passed, or the language could not be set.

`Language::tree_sitter_language` is not exported: structural search (SS3)
does not need it.

The "set_language fails" criterion is a unit test in `parse_str.rs`: a Language
whose grammar is a fake of ABI version 0, which no parser accepts, gives `None`.
An integration test cannot build one.
