#!/bin/sh
# Self-test for ci-test-affected.sh, by hand: a throwaway git repo holding a two-crate cargo
# workspace (a depends on b and fe-syntax, and fastedit, which reads every crate's sources, on none;
# fe-syntax and fastedit read extensions/grammars/), one commit per case on top of the fixture's base, each checked
# with FASTEDIT_AFFECTED_DRY_RUN=1. A docs-only change runs the docs package (b here), not
# nothing; see the script's header.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
PATH="$HOME/.cargo/bin:$PATH"
export PATH
t=$(mktemp -d "${TMPDIR:-/tmp}/ci-test-affected.XXXXXX")
trap 'rm -rf "$t"' EXIT
cd "$t"
git init -q -b main
git config user.name test
git config user.email test@example.com
printf '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\n' >Cargo.toml
mkdir -p crates/a/src crates/b/src crates/fastedit/src crates/fe-syntax/src docs \
	extensions/grammars/x extensions/other
printf '[package]\nname = "a"\nversion = "0.1.0"\nedition = "2024"\n\n[dependencies]\nb = { path = "../b" }\nfe-syntax = { path = "../fe-syntax" }\n' >crates/a/Cargo.toml
printf '[package]\nname = "fe-syntax"\nversion = "0.1.0"\nedition = "2024"\n' >crates/fe-syntax/Cargo.toml
echo 'pub fn s() {}' >crates/fe-syntax/src/lib.rs
echo '; query' >extensions/grammars/x/highlights.scm
echo '# other' >extensions/other/extension.toml
printf '[package]\nname = "b"\nversion = "0.1.0"\nedition = "2024"\n' >crates/b/Cargo.toml
echo 'pub fn a() {}' >crates/a/src/lib.rs
echo 'pub fn b() {}' >crates/b/src/lib.rs
printf '[package]\nname = "fastedit"\nversion = "0.1.0"\nedition = "2024"\n' >crates/fastedit/Cargo.toml
echo 'pub fn f() {}' >crates/fastedit/src/lib.rs
echo 'data' >crates/b/data.txt
echo '# Notes' >docs/notes.md
echo 'version = 2' >regatta.toml
cargo generate-lockfile --offline -q
git add -A
git commit -q -m base
base=$(git rev-parse HEAD)

fail=0
# check <name> <expected command> <path to change> [script args]
check() {
	name=$1 want=$2 path=$3
	shift 3
	git checkout -q -B "case" "$base"
	echo "// $name" >>"$path"
	git commit -q -am "$name"
	got=$(FASTEDIT_TEST_BASE=${case_base-$base} REGATTA_BASE_SHA='' FASTEDIT_AFFECTED_DRY_RUN=1 \
		FASTEDIT_AFFECTED_DOCS_PACKAGE=b "$here/ci-test-affected.sh" "$@" 2>"$t/err")
	if [ "$got" != "$want" ]; then
		echo "FAIL: $name: got '$got', want '$want'; stderr: $(cat "$t/err")"
		fail=1
	fi
}
check "b selects b and a" "cargo nextest run --profile ci -p a -p b -p fastedit" crates/b/src/lib.rs
check "a selects a" "cargo nextest run --profile ci -p a -p fastedit --no-capture" crates/a/src/lib.rs --no-capture
check "b's data skips fastedit" "cargo nextest run --profile ci -p a -p b" crates/b/data.txt
check "docs only" "cargo nextest run --profile ci -p b" docs/notes.md
check "a grammar's file selects its readers alone" "cargo nextest run --profile ci -p fastedit -p fe-syntax" \
	extensions/grammars/x/highlights.scm
check "another extension is outside every package" "cargo nextest run --workspace --profile ci" \
	extensions/other/extension.toml
check "Cargo.lock" "cargo nextest run --workspace --profile ci" Cargo.lock
check "regatta.toml" "cargo nextest run --workspace --profile ci" regatta.toml
case_base=0000000000000000000000000000000000000000
check "unknown base" "cargo nextest run --workspace --profile ci" crates/a/src/lib.rs
# Empty: the default, the merge-base with main.
case_base=
check "merge-base" "cargo nextest run --profile ci -p a -p fastedit" crates/a/src/lib.rs
[ "$fail" = 0 ] && echo ok
exit "$fail"
