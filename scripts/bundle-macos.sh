#!/bin/sh
# Assemble Tenmoku.app from an already-built release binary and sign it. See --help.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
macos="$root/crates/fastedit/macos"
icon_png="$root/crates/fastedit/assets/icon/fastedit-1024.png"

usage() {
	cat <<EOF
Usage: scripts/bundle-macos.sh [--binary PATH] [--out DIR] [--version X.Y.Z] [--sparkle]
                               [--feed-url URL] [--ed-public-key KEY]
       scripts/bundle-macos.sh --sign-only APP

Builds DIR/Tenmoku.app from a Tenmoku binary and signs it. It does not run cargo:
build first with 'cargo build --release -p fastedit'.

  --binary PATH     the tenmoku executable to bundle (default: target/release/tenmoku,
                    under CARGO_TARGET_DIR when that is set)
  --out DIR         where Tenmoku.app is written (default: target/bundle); an existing
                    Tenmoku.app there is replaced
  --version X.Y.Z   CFBundleShortVersionString and CFBundleVersion (default: the
                    [workspace.package] version in Cargo.toml)
  --sparkle         embed Sparkle.framework, fetched by scripts/fetch-sparkle.sh
  --feed-url URL    the update feed, https only (default: TENMOKU_FEED_URL)
  --ed-public-key KEY  the feed's EdDSA public key (default: TENMOKU_ED_PUBLIC_KEY)
  --sign-only APP   skip assembly; only re-sign and verify the existing bundle APP
  -h, --help        this text

The bundle holds Contents/MacOS/tenmoku, Contents/Info.plist (from
crates/fastedit/macos/Info.plist) and Contents/Resources/Tenmoku.icns, generated
here from crates/fastedit/assets/icon/fastedit-1024.png with sips and iconutil.
The binary embeds the fonts in crates/fe-fonts/fonts/, so each family's OFL.txt
is copied, unmodified, to Contents/Resources/fonts/<family>/OFL.txt: the SIL Open
Font License travels with the fonts.

With a feed URL and a key, Info.plist gets the five SU* update keys; if either is empty
it has none of them, so the build never polls.

Signing uses the hardened runtime and crates/fastedit/macos/Tenmoku.entitlements.

  SIGN_IDENTITY     the codesign identity (default '-', an ad-hoc signature). For
                    distribution, a "Developer ID Application: <name> (<team>)"
                    certificate: see docs/release/macos-signing.md.
  SPARKLE_FRAMEWORK  embed this framework instead of fetching one (implies --sparkle);
                    its licence must be beside it, as LICENSE. Without this or --sparkle
                    the bundle has no Sparkle.

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
sparkle=""
assembly_opts=""
feed_url=${TENMOKU_FEED_URL:-}
ed_public_key=${TENMOKU_ED_PUBLIC_KEY:-}
while [ $# -gt 0 ]; do
	case "$1" in
	--binary) need_arg "$@"; binary=$2; shift 2 ;;
	--out) need_arg "$@"; out=$2; shift 2 ;;
	--version) need_arg "$@"; version=$2; shift 2 ;;
	--sparkle) sparkle=1; assembly_opts=1; shift ;;
	# These two may be empty: that means no feed, as an unset variable does.
	--feed-url)
		[ $# -ge 2 ] || die "$1 needs a value (see --help)"
		feed_url=$2; assembly_opts=1; shift 2
		;;
	--ed-public-key)
		[ $# -ge 2 ] || die "$1 needs a value (see --help)"
		ed_public_key=$2; assembly_opts=1; shift 2
		;;
	--sign-only) need_arg "$@"; sign_only=$2; shift 2 ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

[ "$(uname -s)" = Darwin ] || die "macOS only (needs codesign, sips and iconutil)"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

sign() {
	app=$1
	identity=${SIGN_IDENTITY:--}
	# codesign refuses Finder info and other extended attributes inside a bundle.
	xattr -cr "$app"
	# Notarization needs a secure timestamp; an ad-hoc signature cannot have one.
	if [ "$identity" = - ]; then timestamp=--timestamp=none; else timestamp=--timestamp; fi
	entitlements="$macos/Tenmoku.entitlements"
	bundled="$app/Contents/Frameworks/Sparkle.framework"
	if [ -d "$bundled" ]; then
		# Inside out, each without the app's entitlements and without --deep.
		for part in "$bundled/Versions/B/Autoupdate" "$bundled/Versions/B/Updater.app" "$bundled"; do
			codesign --force --options runtime "$timestamp" -s "$identity" "$part" ||
				die "codesign failed for $part (identity '$identity')"
		done
		# The hardened runtime won't load an ad-hoc framework: it has no team ID to match.
		if [ "$identity" = - ]; then
			entitlements="$work/Tenmoku-adhoc.entitlements"
			cp "$macos/Tenmoku.entitlements" "$entitlements"
			/usr/libexec/PlistBuddy -c "Add :com.apple.security.cs.disable-library-validation bool true" \
				"$entitlements" >/dev/null || die "could not add disable-library-validation"
		fi
	fi
	codesign --force --options runtime "$timestamp" \
		--entitlements "$entitlements" -s "$identity" "$app" ||
		die "codesign failed for $app (identity '$identity')"
	codesign --verify --strict --deep "$app" || die "codesign --verify failed for $app"
}

if [ -n "$sign_only" ]; then
	[ -n "$binary$out$version$assembly_opts" ] && die "--sign-only takes no other options"
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

# All refusals come before the old bundle is replaced.
case "$feed_url" in
"" | https://?*) ;;
*) die "the feed URL must be https, got '$feed_url'" ;;
esac
case "$feed_url" in
*[[:space:]]*) die "the feed URL has whitespace in it: '$feed_url'" ;;
esac
if { [ -n "$feed_url" ] && [ -z "$ed_public_key" ]; } || { [ -z "$feed_url" ] && [ -n "$ed_public_key" ]; }; then
	echo "bundle-macos: a feed URL and a public key go together; with only one, no SU* keys" >&2
fi

framework=${SPARKLE_FRAMEWORK:-}
if [ -z "$framework" ] && [ -n "$sparkle" ]; then
	framework=$("$here/fetch-sparkle.sh") || die "could not fetch Sparkle"
fi
if [ -n "$framework" ]; then
	[ -d "$framework/Versions/B" ] || die "not a Sparkle.framework: $framework"
	sparkle_license="$(dirname "$framework")/LICENSE"
	[ -f "$sparkle_license" ] || die "Sparkle's licence must ship with it, and $sparkle_license is missing"
fi

mkdir -p "$out"
app="$out/Tenmoku.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$binary" "$app/Contents/MacOS/tenmoku"
chmod 755 "$app/Contents/MacOS/tenmoku"

plist="$app/Contents/Info.plist"
sed "s/@VERSION@/$version/g" "$macos/Info.plist" >"$plist"
if [ -n "$feed_url" ] && [ -n "$ed_public_key" ]; then
	# Not through sed above: a URL can hold /, & and other characters special to sed and XML.
	plutil -replace SUFeedURL -string "$feed_url" "$plist" || die "could not set SUFeedURL"
	plutil -replace SUPublicEDKey -string "$ed_public_key" "$plist" || die "could not set SUPublicEDKey"
else
	for key in SUFeedURL SUPublicEDKey SUEnableAutomaticChecks SUScheduledCheckInterval SUAutomaticallyUpdate; do
		/usr/libexec/PlistBuddy -c "Delete :$key" "$plist" || die "could not delete $key from Info.plist"
	done
fi
plutil -lint -s "$plist" || die "Info.plist does not lint"

if [ -n "$framework" ]; then
	bundled="$app/Contents/Frameworks/Sparkle.framework"
	mkdir -p "$app/Contents/Frameworks"
	ditto "$framework" "$bundled" || die "ditto could not copy $framework"
	# Tenmoku isn't sandboxed, so Sparkle needs only Autoupdate and Updater.app. The top-level
	# XPCServices is a symlink into the deleted directory, and codesign refuses it left dangling.
	rm -rf "$bundled/Versions/B/XPCServices" "$bundled/XPCServices"
	cp "$sparkle_license" "$app/Contents/Resources/Sparkle-LICENSE.txt"
fi

# The OFL-1.1 text of each font family the binary embeds (crates/fe-fonts), before signing.
fonts="$root/crates/fe-fonts/fonts"
copied=0
for licence in "$fonts"/*/OFL.txt; do
	[ -f "$licence" ] || continue
	family=$(basename "$(dirname "$licence")")
	mkdir -p "$app/Contents/Resources/fonts/$family"
	cp "$licence" "$app/Contents/Resources/fonts/$family/OFL.txt" || die "could not copy $licence"
	copied=$((copied + 1))
done
[ "$copied" -gt 0 ] || die "no OFL.txt under $fonts: the bundled fonts' licences must ship with them"

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
