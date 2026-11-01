#!/bin/sh
# Assemble Tenmoku.app from an already-built release binary and sign it. See --help.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
macos="$root/crates/fastedit/macos"
icon_png="$root/crates/fastedit/assets/icon/fastedit-1024.png"

usage() {
	cat <<EOF
Usage: scripts/bundle-macos.sh [--binary PATH] [--out DIR] [--version X.Y.Z]
       scripts/bundle-macos.sh --sign-only APP

Builds DIR/Tenmoku.app from a Tenmoku binary and signs it. It does not run cargo:
build first with 'cargo build --release -p fastedit'.

  --binary PATH     the tenmoku executable to bundle (default: target/release/tenmoku,
                    under CARGO_TARGET_DIR when that is set)
  --out DIR         where Tenmoku.app is written (default: target/bundle); an existing
                    Tenmoku.app there is replaced
  --version X.Y.Z   CFBundleShortVersionString and CFBundleVersion (default: the
                    [workspace.package] version in Cargo.toml)
  --sign-only APP   skip assembly; only re-sign and verify the existing bundle APP
  -h, --help        this text

The bundle holds Contents/MacOS/tenmoku, Contents/Info.plist (from
crates/fastedit/macos/Info.plist) and Contents/Resources/Tenmoku.icns, generated
here from crates/fastedit/assets/icon/fastedit-1024.png with sips and iconutil.

Signing uses the hardened runtime and crates/fastedit/macos/Tenmoku.entitlements.

  SIGN_IDENTITY     the codesign identity (default '-', an ad-hoc signature). For
                    distribution, a "Developer ID Application: <name> (<team>)"
                    certificate: see docs/release/macos-signing.md.

It prints the app path last and exits non-zero on any failure.
EOF
}

die() {
	echo "bundle-macos: $*" >&2
	exit 1
}

need_arg() {
	[ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value (see --help)"
}

binary=""
out=""
version=""
sign_only=""
while [ $# -gt 0 ]; do
	case "$1" in
	--binary) need_arg "$@"; binary=$2; shift 2 ;;
	--out) need_arg "$@"; out=$2; shift 2 ;;
	--version) need_arg "$@"; version=$2; shift 2 ;;
	--sign-only) need_arg "$@"; sign_only=$2; shift 2 ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

[ "$(uname -s)" = Darwin ] || die "macOS only (needs codesign, sips and iconutil)"

sign() {
	app=$1
	identity=${SIGN_IDENTITY:--}
	# codesign refuses Finder info and other extended attributes inside a bundle.
	xattr -cr "$app"
	# Notarization needs a secure timestamp; an ad-hoc signature cannot have one.
	if [ "$identity" = - ]; then timestamp=--timestamp=none; else timestamp=--timestamp; fi
	codesign --force --options runtime "$timestamp" \
		--entitlements "$macos/Tenmoku.entitlements" -s "$identity" "$app" ||
		die "codesign failed for $app (identity '$identity')"
	codesign --verify --strict --deep "$app" || die "codesign --verify failed for $app"
}

if [ -n "$sign_only" ]; then
	[ -n "$binary$out$version" ] && die "--sign-only takes no other options"
	[ -d "$sign_only/Contents/MacOS" ] || die "not an app bundle: $sign_only"
	sign "$sign_only"
	echo "$sign_only"
	exit 0
fi

target=${CARGO_TARGET_DIR:-$root/target}
binary=${binary:-$target/release/tenmoku}
out=${out:-$root/target/bundle}
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

[ -f "$binary" ] || die "binary not found: $binary (this script does not build; run cargo build --release -p fastedit)"
[ -x "$binary" ] || die "binary is not executable: $binary"
[ -f "$icon_png" ] || die "icon not found: $icon_png"

mkdir -p "$out"
app="$out/Tenmoku.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$binary" "$app/Contents/MacOS/tenmoku"
chmod 755 "$app/Contents/MacOS/tenmoku"

sed "s/@VERSION@/$version/g" "$macos/Info.plist" >"$app/Contents/Info.plist"
plutil -lint -s "$app/Contents/Info.plist" || die "Info.plist does not lint"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
iconset="$work/Tenmoku.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
	double=$((size * 2))
	sips -z "$size" "$size" "$icon_png" --out "$iconset/icon_${size}x${size}.png" >/dev/null ||
		die "sips failed at ${size}px"
	if [ "$double" -eq 1024 ]; then
		cp "$icon_png" "$iconset/icon_${size}x${size}@2x.png"
	else
		sips -z "$double" "$double" "$icon_png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null ||
			die "sips failed at ${double}px"
	fi
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Tenmoku.icns" || die "iconutil failed"

sign "$app"
echo "$app"
