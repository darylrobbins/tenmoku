#!/bin/sh
# Packs an already-built Linux release binary into dist/Tenmoku-<version>-linux-<arch>.tar.gz
# with its .sha256. See --help.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

usage() {
	cat <<EOF
Usage: scripts/package-linux.sh [--binary PATH] [--version X.Y.Z] [--arch ARCH] [--out DIR]

Writes DIR/Tenmoku-<version>-linux-<arch>.tar.gz and its .sha256. It does not run cargo:
build first with 'cargo build --release -p fastedit'. The archive holds one folder,
Tenmoku-<version>-linux-<arch>/, with:

  tenmoku                       the binary
  dev.tenmoku.tenmoku.desktop   the desktop entry (packaging/linux/)
  dev.tenmoku.tenmoku.svg       the icon (crates/fastedit/assets/icon/fastedit.svg)
  install.sh                    a per-user install into ~/.local (packaging/linux/)
  Licenses/                     the licence texts the macOS bundle ships, unmodified

  --binary PATH     the tenmoku executable (default: target/release/tenmoku, under
                    CARGO_TARGET_DIR when that is set)
  --version X.Y.Z   the version in the names (default: Cargo.toml's [workspace.package])
  --arch ARCH       the architecture in the names (default: x86_64)
  --out DIR         where the archive goes (default: dist)
  -h, --help        this text

It prints the archive's path last and exits non-zero on any failure.
EOF
}

die() {
	echo "package-linux: $*" >&2
	exit 1
}

need_arg() {
	[ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value (see --help)"
}

binary=""
version=""
arch=x86_64
out=dist
while [ $# -gt 0 ]; do
	case "$1" in
	--binary) need_arg "$@"; binary=$2; shift 2 ;;
	--version) need_arg "$@"; version=$2; shift 2 ;;
	--arch) need_arg "$@"; arch=$2; shift 2 ;;
	--out) need_arg "$@"; out=$2; shift 2 ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

binary=${binary:-${CARGO_TARGET_DIR:-$root/target}/release/tenmoku}
if [ -z "$version" ]; then
	version=$(awk '
		/^\[/ { in_pkg = ($0 == "[workspace.package]") }
		in_pkg && /^version[ \t]*=/ { gsub(/.*=[ \t]*"|".*/, ""); print; exit }
	' "$root/Cargo.toml")
	[ -n "$version" ] || die "no [workspace.package] version in $root/Cargo.toml"
fi
case "$version" in
*[!0-9.]* | .* | *. | *..*) die "version must be dotted numbers, got '$version'" ;;
esac
[ -f "$binary" ] && [ -x "$binary" ] || die "no executable binary at $binary (run cargo build --release -p fastedit)"

name="Tenmoku-$version-linux-$arch"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
dir="$work/$name"
mkdir -p "$dir"

cp "$binary" "$dir/tenmoku"
chmod 755 "$dir/tenmoku"
cp "$root/packaging/linux/dev.tenmoku.tenmoku.desktop" "$dir/"
cp "$root/packaging/linux/install.sh" "$dir/"
chmod 755 "$dir/install.sh"
cp "$root/crates/fastedit/assets/icon/fastedit.svg" "$dir/dev.tenmoku.tenmoku.svg"

# The licence texts, as scripts/bundle-macos.sh lays them out in Contents/Resources: the fonts'
# OFL.txt under fonts/<family>/, the rest flat in Licenses/.
ship() { # ship SOURCE NAME
	[ -f "$1" ] || die "licence missing: $1"
	mkdir -p "$(dirname "$dir/Licenses/$2")"
	cp "$1" "$dir/Licenses/$2" || die "could not copy $1"
}
fonts=0
for licence in "$root"/crates/fe-fonts/fonts/*/OFL.txt; do
	[ -f "$licence" ] || continue
	ship "$licence" "fonts/$(basename "$(dirname "$licence")")/OFL.txt"
	fonts=$((fonts + 1))
done
[ "$fonts" -gt 0 ] || die "no OFL.txt under crates/fe-fonts/fonts: the fonts' licences must ship with them"
ship "$root/LICENSE" LICENSE
ship "$root/NOTICE" NOTICE
ship "$root/LICENSES/Apache-2.0.txt" LICENSES/Apache-2.0.txt
ship "$root/LICENSES/MIT.txt" LICENSES/MIT.txt
ship "$root/THIRD_PARTY.md" THIRD_PARTY.md
ship "$root/kiln/crates/kiln-ui/assets/icons/LICENSE" icons-LICENSE.txt
ship "$root/kiln/crates/kiln-ui/assets/file-icons/LICENSE" file-icons-LICENSE.txt
ship "$root/vendor/tree-sitter/LICENSE" tree-sitter-LICENSE.txt
for grammar in "$root"/vendor/tree-sitter-*/; do
	ship "$grammar/LICENSE" "vendor-$(basename "$grammar")-LICENSE.txt"
done
for grammar in "$root"/extensions/grammars/*/; do
	g=$(basename "$grammar")
	ship "$grammar/LICENSE" "grammar-$g-LICENSE.txt"
	for extra in "$grammar"LICENSE-* "$grammar"NOTICE; do
		[ -f "$extra" ] || continue
		ship "$extra" "grammar-$g-$(basename "$extra").txt"
	done
done

mkdir -p "$out"
out=$(cd "$out" && pwd)
archive="$name.tar.gz"
tar -C "$work" -czf "$out/$archive" "$name" || die "tar failed"
if command -v sha256sum >/dev/null 2>&1; then
	(cd "$out" && sha256sum "$archive" >"$archive.sha256")
else
	(cd "$out" && shasum -a 256 "$archive" >"$archive.sha256")
fi
echo "$out/$archive"
