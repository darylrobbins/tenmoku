#!/bin/sh
# Fixture test for heap-by-crate.py: a synthetic malloc_history -allBySize
# sample folds to the right crates, totals match, --depth 2 splits by module,
# unknown frames are unattributed and bad input fails with a message.
# No process is attached.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
tool="python3 $here/heap-by-crate.py"

M='0x1 (libsystem_malloc.dylib) _malloc | 0x2 (libsystem_malloc.dylib) _malloc_zone_malloc'
cat >"$t/in.txt" <<EOT
malloc_history Report Version:  2.0
----

1 call for 1000 bytes: 0x1 (dyld) start | 0x2 (tenmoku) fe_syntax::parse::load | 0x3 (tenmoku) alloc::vec::Vec<T,A>::grow_one | 0x4 (tenmoku) alloc::raw_vec::finish_grow | 0x5 (tenmoku) __rust_realloc | $M
3 calls for 3000 bytes: 0x1 (tenmoku) fe_nav::index::build | 0x2 (tenmoku) hashbrown::raw::RawTable<T>::insert | 0x3 (tenmoku) hashbrown::raw::reserve_rehash | 0x4 (tenmoku) __rust_alloc | $M
2 calls for 200 bytes: 0x1 (tenmoku) fe_syntax::tree::grow | 0x2 (tenmoku) ts_subtree_new_leaf | 0x3 (tenmoku) ts_malloc | $M
1 call for 400 bytes: 0x1 (tenmoku) gpui::run | 0x2 (CoreText) CTFontCreate | 0x3 (CoreText) TFont::alloc | $M
1 call for 50 bytes: 0x1 (tenmoku) _ZN5ropey4rope4Rope6insert17h0123456789abcdefE | 0x2 (tenmoku) _ZN5alloc3vec16Vec\$LT\$T\$C\$A\$GT\$8grow_one17h0123456789abcdefE | $M
1 call for 70 bytes: 0x1 (tenmoku) _RNvNtCs1a2b_6fe_vim3run4step | 0x2 (tenmoku) _RNvNtCs1a2b_4core3fmt5write | $M
1 call for 9 bytes: 0x1 (tenmoku) ??? | $M

Binary Images:
EOT
total=$((1000 + 3000 + 200 + 400 + 50 + 70 + 9))

fail() { echo "FAIL: $*" >&2; exit 1; }
out=$($tool "$t/in.txt")
row() { echo "$out" | awk -v k="$1" '$1 == k || ($1" "$2) == k { print }'; }
want() { # <row key> <bytes> <allocations>
  row "$1" | awk -v b="$2" -v c="$3" '$0 ~ "(^| )" b " " && $NF == c { ok = 1 } END { exit !ok }' \
    || fail "row '$1' is not $2 bytes / $3 allocations:
$out"
}
want fe_syntax 1000 1
want fe_nav 3000 3
want "tree-sitter (C)" 200 2
want CoreText 400 1
want ropey 50 1
want fe_vim 70 1
want unattributed 9 1
echo "$out" | awk -v t=$total '$1 == "total" && $2 == t { ok = 1 } END { exit !ok }' \
  || fail "total is not $total:
$out"

# --depth 2 splits fe_syntax by module (parse is Vec growth, tree is C stack).
d2=$($tool "$t/in.txt" --depth 2)
echo "$d2" | rg -q '^fe_syntax::parse +1000 ' || fail "no fe_syntax::parse at depth 2:
$d2"
echo "$d2" | rg -q '^fe_nav::index +3000 ' || fail "no fe_nav::index at depth 2:
$d2"

# <impl X for Y> goes to Y; --skip applies before framework attribution.
printf '1 call for 7 bytes: 0x1 (tenmoku) <impl fe_x::Tr for fe_y::Ty>::m | 0x2 (tenmoku) alloc::alloc | %s\n' "$M" >"$t/impl.txt"
$tool "$t/impl.txt" | rg -q '^fe_y ' || fail "impl X for Y not charged to Y"
printf '1 call for 7 bytes: 0x1 (tenmoku) fe_a::f | 0x2 (CoreText) CTFont | %s\n' "$M" >"$t/sk.txt"
$tool "$t/sk.txt" --skip CTFont | rg -q '^fe_a ' || fail "--skip did not skip a framework frame"
if msg=$($tool "$t/in.txt" --footprint 999999 2>&1); then fail "bad footprint pid exited 0"; fi
echo "$msg" | rg -q 'footprint failed' || fail "no footprint message: $msg"

# v0 impl methods and trait impls: _RNvMNt..<Self>::new, _RNvXs_..<Self as Trait>::m.
printf '1 call for 5 bytes: 0x1 (tenmoku) _RNvMNtCs1_6fe_vim3modNtB2_3Foo3new | %s\n' "$M" >"$t/v0a.txt"
$tool "$t/v0a.txt" | rg -q '^fe_vim ' || fail "v0 inherent impl not charged to its crate"
printf '1 call for 6 bytes: 0x1 (tenmoku) _RNvXs_C3fooNtB4_3BarNtNtC4core5clone5Clone5clone | %s\n' "$M" >"$t/v0b.txt"
$tool "$t/v0b.txt" | rg -q '^foo ' || fail "v0 trait impl not charged to Self's crate"

# Generic impl methods (function-type and other args) are charged to Self's
# crate, by an external demangler and by the built-in fallback alike.
for sym in _RNvMC3fooINtC3bar3BazFEuE3new _RNvMC3fooINtC3bar3BazRTjmEE3new \
  _RNvMC3fooINtC3bar3BazKj5_E3new _RNvMC3fooINtC3bar3BazFUEuuE3new; do
  printf '1 call for 5 bytes: 0x1 (tenmoku) %s | %s\n' "$sym" "$M" >"$t/g.txt"
  $tool "$t/g.txt" | rg -q '^bar ' || fail "$sym not charged to bar"
  HEAP_BY_CRATE_NO_EXT=1 $tool "$t/g.txt" | rg -q '^bar ' || fail "$sym (no external demangler) not charged to bar"
done
# A truncated v0 symbol must not crash the report: its bytes are still counted.
for sym in _RNvMC3fooINtC3bar3BazRT _RNvMC3fooINtC3bar3Baz _RNvMC3fooIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII; do
  printf '1 call for 11 bytes: 0x1 (tenmoku) %s | %s\n' "$sym" "$M" >"$t/tr.txt"
  for e in 0 1; do
    if [ $e = 1 ]; then export HEAP_BY_CRATE_NO_EXT=1; else unset HEAP_BY_CRATE_NO_EXT; fi
    $tool "$t/tr.txt" | awk '$1 == "total" && $2 == 11 { ok = 1 } END { exit !ok }' \
      || fail "truncated symbol $sym lost its allocation (ext=$e)"
  done
done
unset HEAP_BY_CRATE_NO_EXT

# stdin and --top.
$tool - <"$t/in.txt" >/dev/null || fail "stdin input"
$tool "$t/in.txt" --top 2 | rg -q '\(others\)' || fail "--top lacks (others)"

# Empty and unreadable input fail with a message.
: >"$t/empty.txt"
if msg=$($tool "$t/empty.txt" 2>&1); then fail "empty input exited 0"; fi
echo "$msg" | rg -q 'heap-by-crate:' || fail "no message for empty input: $msg"
if msg=$($tool "$t/missing.txt" 2>&1); then fail "missing input exited 0"; fi
echo "$msg" | rg -q 'cannot read' || fail "no message for missing input: $msg"
echo ok
