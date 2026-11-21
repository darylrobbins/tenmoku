#!/bin/sh
# Fixture test for scripts/check-release-version.sh: a temp tree with a copy of the script, a
# Cargo.toml and release notes. Runs with sh on Linux or macOS; prints one line per check and
# exits non-zero on the first failure.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

fail() {
	echo "FAIL: $*"
	exit 1
}
ok() { echo "ok: $*"; }

mkdir -p "$t/scripts" "$t/release-notes"
cp "$here/check-release-version.sh" "$t/scripts/"
check="$t/scripts/check-release-version.sh"

# setup VERSION: Cargo.toml at VERSION, notes for it.
setup() {
	cat >"$t/Cargo.toml" <<EOF
[workspace]
members = []

[workspace.package]
version = "$1"
edition = "2024"
EOF
	rm -f "$t"/release-notes/*.md
	printf '## Fixed\n\n- A thing\n' >"$t/release-notes/$1.md"
}
accepts() {
	sh "$check" "$1" >"$t/log" 2>&1 || { cat "$t/log"; fail "$1 was refused"; }
}
refuses() {
	if sh "$check" "$1" >"$t/log" 2>&1; then
		fail "$1 was accepted"
	fi
}

setup 0.3.0-preview.2
accepts v0.3.0-preview.2
ok "v0.3.0-preview.2 is Cargo's 0.3.0-preview.2 with notes: accepted"
rm "$t/release-notes/0.3.0-preview.2.md"
refuses v0.3.0-preview.2
grep -q 'release-notes/0.3.0-preview.2.md is missing' "$t/log" || fail "no missing-notes message: $(cat "$t/log")"
ok "a preview tag still needs release-notes/<version>.md"
setup 0.3.0-preview.2
refuses v0.3.0-preview.3
refuses v0.3.0
refuses v0.3.0-preview.1
ok "a tag whose version is not Cargo's is refused"

for bad in 0.3.0-preview 0.3.0-preview.01 0.3.0-rc.1 0.3.0-preview. 0.3.0-preview.1.2 0.3.0-preview.x; do
	setup "$bad"
	refuses "v$bad"
done
ok "v0.3.0-preview, v0.3.0-preview.01, v0.3.0-rc.1 and other malformed tags are refused, even when Cargo.toml has them"
setup 0.3.0-preview.2
refuses 0.3.0-preview.2
ok "a tag without its v is refused"

setup 0.3.0
accepts v0.3.0
refuses v0.3.0-preview.1
ok "a stable tag is still accepted"
setup 0.3.0-preview.10
accepts v0.3.0-preview.10
ok "a two-digit preview number is accepted"

echo ok
