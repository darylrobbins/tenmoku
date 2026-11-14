---
task_uid: "01m7btthxq7hv36722hj50sdac"
task: "T627"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Every extensions/grammars/*/extension.toml has a packs line naming only packs registry.toml defines, as the spec's table says; the diff changes nothing else"
    required: true
    manual: false
  - id: "C2"
    text: "fe-registry's every_first_party_pack_is_defined and fastedit's grammar_install_tests::every_first_party_manifest_parses pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish note lists any table language with no directory yet, and any directory not in the table with the packs chosen for it"
    required: true
    manual: false
---

# Give every first-party grammar directory its packs (the languages that landed before the packs key) (Mac T1487)

# Give every first-party grammar directory its packs

The index card added `packs = [...]` to `[[contributes.grammars]]` and gave the
first eight theirs. The language cards (T1435–T1480) were drafted before the
key existed, so they land without it, and those languages are in no pack. This
card adds the one `packs` line to every `extensions/grammars/*/extension.toml`
that lacks it. Nothing else changes.

Use this table (the first pack listed is the main pack, which the prompt
offers). For a directory not in the table, pick from `registry.toml`'s packs and
say so in the finish note. For a table language with no directory yet (its card
hasn't landed, or was lent to another host), list it in the finish note so the
lead can add the line to that card.

| Language | packs |
| --- | --- |
| powershell, fish, awk | shells |
| vue, svelte, scss, graphql, astro, twig | web |
| hcl, terraform, nix, nginx, starlark | infra |
| haskell, ocaml, elm, gleam, erlang, scheme, racket | functional |
| elixir | functional, web |
| clojure | functional, jvm |
| fsharp | dotnet, functional |
| groovy, properties | jvm |
| zig, fortran | systems |
| objc | mobile, systems |
| dart | mobile |
| r, julia | data |
| protobuf, kdl, ron | data, config |
| solidity | web |
| latex, typst | docs |
| glsl, wgsl, hlsl | shaders |
| verilog, vhdl | hardware |
| cmake, vim | config |
| perl | scripting |

Use the directory names the language cards chose (e.g. `objc` or
`objective_c`). Match them by the `language` key.

Add no gate test that every language has a pack. Language cards lent to other
hosts may still land without the key, and the index generator's warning covers
them. Scope is the `extension.toml` files only.
