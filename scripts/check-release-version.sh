#!/bin/sh
# Refuses a release tag that isn't exactly v plus the [workspace.package] version in Cargo.toml.
# The binary has that version built in, and Check for Updates compares it with the latest
# release's tag, so a release tagged otherwise would offer itself as its own update.
# Usage: scripts/check-release-version.sh vX.Y.Z
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
die() {
	echo "check-release-version: $*" >&2
	exit 1
}

[ $# -eq 1 ] || die "usage: check-release-version.sh vX.Y.Z"
version=$(awk '
	/^\[/ { in_pkg = ($0 == "[workspace.package]") }
	in_pkg && /^version[ \t]*=/ { gsub(/.*=[ \t]*"|".*/, ""); print; exit }
' "$root/Cargo.toml")
[ -n "$version" ] || die "no [workspace.package] version in $root/Cargo.toml"
case "$1" in
"v$version") echo "the tag $1 is Cargo.toml's version" ;;
*) die "the tag '$1' isn't v$version: set Cargo.toml's version to the release's, commit it and tag that" ;;
esac
