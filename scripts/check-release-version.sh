#!/bin/sh
# Refuses a release tag that isn't exactly v plus the [workspace.package] version in Cargo.toml,
# or whose release notes, release-notes/<version>.md, are missing, empty or break a line rule.
# The binary has that version built in, and Check for Updates compares it with the latest
# release's tag, so a release tagged otherwise would offer itself as its own update. The notes
# file is the GitHub release's body, the Sparkle appcast's description and the What's New tab's
# text (its rules: crates/fe-workbench/src/whats_new.rs).
# A tag is vX.Y.Z or vX.Y.Z-preview.N (N a number without a leading zero); nothing else, so
# v0.3.0-preview, v0.3.0-preview.01 and v0.3.0-rc.1 are refused even if Cargo.toml says so.
# Usage: scripts/check-release-version.sh vX.Y.Z[-preview.N]
#        scripts/check-release-version.sh --notes X.Y.Z[-preview.N]   (the notes file only)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
die() {
	echo "check-release-version: $*" >&2
	exit 1
}

# is_version V: V is X.Y.Z or X.Y.Z-preview.N.
is_version() {
	base=${1%%-preview.*}
	case "$base" in
	"" | *[!0-9.]* | .* | *. | *..*) return 1 ;;
	esac
	[ "$base" != "$1" ] || return 0
	n=${1#*-preview.}
	case "$n" in
	"" | *[!0-9]* | 0?*) return 1 ;;
	esac
}

# check_notes X.Y.Z: the file is there and not empty, and no line of it breaks a line-level rule:
# a line starting with "# " (an H1: the What's New tab and the release add their own title), a
# draft's "## Internal" or "## Unsorted" heading (scripts/draft-release-notes.sh, to be emptied
# before release), or "]]>" anywhere (it would end the appcast's CDATA). Prints every rule
# broken, with its line. The Markdown grammar of the notes (only the ## New, ## Improved and
# ## Fixed sections, bullets of text, no images, raw HTML or code blocks) is checked by
# fe-workbench's tests::whats_new::the_shipped_notes_are_well_formed, which parses every
# release-notes/*.md with fe-markdown in each gate, so on the commit that is tagged. This check
# refuses nothing that test accepts.
check_notes() {
	rel="release-notes/$1.md"
	file="$root/$rel"
	[ -f "$file" ] || die "$rel is missing: draft it with scripts/draft-release-notes.sh, edit it, and commit it with the version bump"
	grep -q '[^[:space:]]' "$file" || die "$rel is empty: it needs at least one of ## New, ## Improved or ## Fixed"
	awk -v rel="$rel" -v q="'" '
		function bad(why) { print rel ":" NR ": " why; broken = 1 }
		/^# / || $0 == "#" { bad("an H1 (" $0 "): the What" q "s New tab and the release add their own title") }
		/^## (Internal|Unsorted)( |$)/ { bad("the draft" q "s heading \"" $0 "\": move what a user would notice into ## New, ## Improved or ## Fixed and delete the rest") }
		index($0, "]]>") { bad("holds ]]>, which would end the appcast CDATA early") }
		END { exit broken }
	' "$file" >&2 || die "$rel breaks the release notes rules (above)"
	echo "$rel is well formed"
}

version_of_cargo() {
	awk '
		/^\[/ { in_pkg = ($0 == "[workspace.package]") }
		in_pkg && /^version[ \t]*=/ { gsub(/.*=[ \t]*"|".*/, ""); print; exit }
	' "$root/Cargo.toml"
}

case "${1-}" in
--notes)
	[ $# -eq 2 ] && [ -n "$2" ] || die "usage: check-release-version.sh --notes X.Y.Z[-preview.N]"
	check_notes "$2"
	exit 0
	;;
esac

[ $# -eq 1 ] || die "usage: check-release-version.sh vX.Y.Z[-preview.N]"
version=$(version_of_cargo)
[ -n "$version" ] || die "no [workspace.package] version in $root/Cargo.toml"
case "$1" in
v*) is_version "${1#v}" || die "the tag '$1' isn't vX.Y.Z or vX.Y.Z-preview.N (N a number without a leading zero)" ;;
*) die "the tag '$1' isn't vX.Y.Z or vX.Y.Z-preview.N" ;;
esac
case "$1" in
"v$version") echo "the tag $1 is Cargo.toml's version" ;;
*) die "the tag '$1' isn't v$version: set Cargo.toml's version to the release's, commit it and tag that" ;;
esac
check_notes "$version"
