#!/bin/sh
# The gate's test stage: runs `cargo nextest run --profile ci` on the workspace packages the
# committed change touches, plus every workspace member that depends on them (normal, dev or
# build dependency, transitively). Extra arguments go to nextest.
#
# The base is FASTEDIT_TEST_BASE, else REGATTA_BASE_SHA (once Regatta exports it), else
# `git merge-base HEAD main`: a gate snapshot is the candidate integrated onto its base, so the
# merge-base is that base even after main moves. The change is `git diff <base> HEAD`, committed
# state only. A file belongs to the package whose directory is its longest prefix; a root package
# would own only src/, tests/, benches/, examples/ and build.rs. A .rs, .scm or .toml file under
# crates/ or kiln/ also selects fastedit, whose provenance test reads them all.
#
# It runs the whole workspace instead, and says why, when there is no usable base, the diff is
# empty, jq or cargo metadata fails, or a changed path can change every crate's build or tests
# (the table below, or any path outside every package other than docs/** and *.md at the root).
#
# A change to docs/** and root *.md alone runs one small package's tests,
# FASTEDIT_AFFECTED_DOCS_PACKAGE (default fe-text), not nothing: Regatta fails a test stage whose
# JUnit report is missing or has no test cases.
#
# FASTEDIT_AFFECTED_DRY_RUN=1 prints the nextest command instead of running it. The chosen set
# and the reason go to stderr. Self-test: scripts/test-ci-test-affected.sh.
set -eu

say() { echo "ci-test-affected: $*" >&2; }

run() {
	if [ "${FASTEDIT_AFFECTED_DRY_RUN:-}" = 1 ]; then
		echo "$*"
		exit 0
	fi
	exec "$@"
}

whole() {
	say "running the workspace: $why"
	run cargo nextest run --workspace --profile ci "$@"
}

if ! root=$(git rev-parse --show-toplevel 2>/dev/null); then
	why="not in a git repository"
	whole "$@"
fi
cd "$root"

base=${FASTEDIT_TEST_BASE:-${REGATTA_BASE_SHA:-}}
if [ -z "$base" ]; then
	base=$(git merge-base HEAD main 2>/dev/null) || base=
fi
if [ -z "$base" ] || ! base_sha=$(git rev-parse --verify --quiet "$base^{commit}"); then
	why="no base commit (FASTEDIT_TEST_BASE, REGATTA_BASE_SHA or the merge-base with main: '$base')"
	whole "$@"
fi
if [ "$base_sha" = "$(git rev-parse HEAD)" ]; then
	why="the base $base_sha is HEAD"
	whole "$@"
fi
if ! changed=$(git -c core.quotePath=false diff --name-only --no-renames "$base_sha" HEAD); then
	why="git diff against $base_sha failed"
	whole "$@"
fi
if [ -z "$changed" ]; then
	why="no change since $base_sha"
	whole "$@"
fi

# Sort out the paths that change everything, and the docs, before asking cargo. readers names
# the packages that read a changed file without depending on its package.
pkg_paths=
readers=
while IFS= read -r f; do
	case $f in
	# fastedit's tests/third_party.rs checks the headers of every such file, and fe-plugin and
	# kiln-plugin, for two, are not among its dependencies.
	crates/*.rs | crates/*.scm | crates/*.toml | kiln/*.rs | kiln/*.scm | kiln/*.toml)
		readers="fastedit"
		;;
	esac
	case $f in
	Cargo.toml | Cargo.lock | rust-toolchain* | .cargo/* | .config/* | scripts/* | regatta.toml | \
		deny.toml | clippy.toml | .clippy.toml | rustfmt.toml | .rustfmt.toml)
		why="$f is workspace-wide"
		whole "$@"
		;;
	# fe-syntax's tests/indent.rs reads it. The other reads of a file outside the reading crate
	# (fe-workbench of fe-markdown's fixtures, fe-gpui of fe-config's themes, fastedit of
	# fe-syntax's fixtures) are covered: the reader depends on the owner. fastedit's reads of
	# the root NOTICE, LICENSES/, deny.toml and kiln/'s files outside its crates are workspace-wide.
	THIRD_PARTY.md)
		why="$f is read by fe-syntax's tests"
		whole "$@"
		;;
	docs/*) ;;
	*/*)
		pkg_paths="$pkg_paths$f
"
		;;
	*.md) ;;
	*)
		pkg_paths="$pkg_paths$f
"
		;;
	esac
done <<EOF
$changed
EOF

if ! command -v jq >/dev/null 2>&1; then
	why="jq is not on PATH"
	whole "$@"
fi
if ! meta=$(cargo metadata --format-version 1 --locked --offline 2>/dev/null); then
	why="cargo metadata --locked --offline failed"
	whole "$@"
fi

if [ -z "$pkg_paths" ]; then
	docs_pkg=${FASTEDIT_AFFECTED_DOCS_PACKAGE:-fe-text}
	if ! printf '%s' "$meta" | jq -e --arg p "$docs_pkg" \
		'. as $m | any(.packages[]; .name == $p and (.id as $id | $m.workspace_members | index($id)))' >/dev/null; then
		why="docs only, and $docs_pkg is not a workspace member"
		whole "$@"
	fi
	say "docs only: running $docs_pkg so the report has tests"
	run cargo nextest run --profile ci -p "$docs_pkg" "$@"
fi

# Prints "outside <path>" for each path no package owns, else "changed <names>" and
# "affected <names>" (the changed packages and the readers that are workspace members, and their
# reverse dependencies), sorted.
result=$(printf '%s' "$meta" | jq -r --arg files "$pkg_paths" --arg readers "$readers" '
	. as $m
	| ($m.workspace_root + "/") as $root
	| [$m.packages[] | select(.id as $id | $m.workspace_members | index($id))] as $ws
	| ($ws | map(.name)) as $names
	| ($ws | map({name, dir: (.manifest_path | ltrimstr($root) | rtrimstr("Cargo.toml"))})) as $dirs
	| def owner($f):
		([$dirs[] | select(.dir as $d | $d != "" and ($f | startswith($d)))] | max_by(.dir | length) | .name)
		// ([$dirs[] | select(.dir == "" and ($f | test("^(src|tests|benches|examples)/|^build\\.rs$")))]
			| first | .name);
	  ($files | split("\n") | map(select(. != "")) | map({f: ., p: owner(.)})) as $owned
	| if any($owned[]; .p == null) then
		$owned[] | select(.p == null) | "outside \(.f)"
	  else
		(reduce $ws[] as $p ({};
			reduce ($p.dependencies[] | select(.path != null) | .name
				| select(. as $n | $names | index($n))) as $d
			(.; .[$d] += [$p.name]))) as $rdeps
		| def close: . as $s | ([$s[], ($s[] | $rdeps[.] // [] | .[])] | unique)
			| if . == $s then . else close end;
		  ($owned | map(.p) + ($readers | split(" ") | map(select(. as $r | $names | index($r))))
			| unique) as $changed
		| "changed \($changed | join(" "))", "affected \($changed | close | join(" "))"
	  end
') || {
	why="jq failed on cargo metadata"
	whole "$@"
}

outside=$(printf '%s\n' "$result" | sed -n 's/^outside //p' | head -n 1)
if [ -n "$outside" ]; then
	why="$outside is outside every package"
	whole "$@"
fi
direct=$(printf '%s\n' "$result" | sed -n 's/^changed //p')
affected=$(printf '%s\n' "$result" | sed -n 's/^affected //p')
say "changed: $direct; with reverse dependencies: $affected"
args=
for p in $affected; do
	args="$args -p $p"
done
# Package names never contain spaces, so $args splits into -p pairs.
# shellcheck disable=SC2086
run cargo nextest run --profile ci $args "$@"
