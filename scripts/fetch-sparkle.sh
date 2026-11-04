#!/bin/sh
# Download the pinned Sparkle release and check its SHA-256. See --help.
set -eu

# Newest 2.x when pinned. To move: change both, then the row in THIRD_PARTY.md.
VERSION=2.10.1
SHA256=1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

usage() {
	cat <<USAGE
Usage: scripts/fetch-sparkle.sh [--out DIR]

Downloads Sparkle $VERSION, refuses it unless its SHA-256 is the pinned one, extracts
Sparkle.framework, LICENSE and bin/{generate_keys,sign_update} into DIR, and prints the
framework path last.

  --out DIR   where to put them (default: target/sparkle/$VERSION under the repository)
  -h, --help  this text

The verified tarball stays in DIR: a later run checks it again and downloads nothing.

  SPARKLE_FRAMEWORK     a framework to use instead: nothing is downloaded and this path is
                        printed
  SPARKLE_URL_OVERRIDE  where to download from instead of GitHub; the SHA-256 still applies
USAGE
}

die() {
	echo "fetch-sparkle: $*" >&2
	exit 1
}

out=""
while [ $# -gt 0 ]; do
	case "$1" in
	--out)
		[ $# -ge 2 ] && [ -n "$2" ] || die "--out needs a value (see --help)"
		out=$2
		shift 2
		;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

if [ -n "${SPARKLE_FRAMEWORK:-}" ]; then
	[ -d "$SPARKLE_FRAMEWORK" ] || die "SPARKLE_FRAMEWORK is not a directory: $SPARKLE_FRAMEWORK"
	echo "$SPARKLE_FRAMEWORK"
	exit 0
fi

out=${out:-$root/target/sparkle/$VERSION}
url=${SPARKLE_URL_OVERRIDE:-https://github.com/sparkle-project/Sparkle/releases/download/$VERSION/Sparkle-$VERSION.tar.xz}
tarball="$out/Sparkle-$VERSION.tar.xz"

mkdir -p "$out"
work=$(mktemp -d "$out/.fetch.XXXXXX")
trap 'rm -rf "$work"' EXIT

sha_of() {
	shasum -a 256 "$1" | awk '{ print $1 }'
}

if [ -f "$tarball" ] && [ "$(sha_of "$tarball")" != "$SHA256" ]; then
	echo "fetch-sparkle: discarding the cached $tarball: its SHA-256 is not the pinned one" >&2
	rm -f "$tarball"
fi
if [ ! -f "$tarball" ]; then
	echo "fetch-sparkle: downloading $url" >&2
	curl -fsSL --retry 3 -o "$work/download" "$url" || die "could not download $url"
	got=$(sha_of "$work/download")
	[ "$got" = "$SHA256" ] || die "SHA-256 mismatch for $url: got $got, expected $SHA256; refusing it"
	mv "$work/download" "$tarball"
fi

# Extracted fresh from the verified tarball each time, so a damaged copy can't be reused.
mkdir "$work/x"
tar -xf "$tarball" -C "$work/x" ./Sparkle.framework ./LICENSE ./bin/generate_keys ./bin/sign_update ||
	die "could not extract $tarball"
rm -rf "${out:?}/Sparkle.framework" "${out:?}/bin" "${out:?}/LICENSE"
mv "$work/x/Sparkle.framework" "$work/x/bin" "$work/x/LICENSE" "$out/"
echo "$out/Sparkle.framework"
