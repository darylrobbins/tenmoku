#!/bin/sh
# Pack Tenmoku.app as a Velopack release and sign its feed (the T1427 spike). See --help.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
entitlements="$root/crates/fastedit/macos/Tenmoku.entitlements"

usage() {
	cat <<'EOF'
Usage: scripts/velopack-pack.sh --version X.Y.Z --feed DIR|URL --out DIR
                                [--channel NAME] [--pack-id ID] [--dry-run]

Builds Tenmoku with the feed and public key baked in, bundles it with
scripts/bundle-macos.sh (no Sparkle, no SU* keys), packs it with vpk into DIR, and
signs DIR/releases.<channel>.json, writing releases.<channel>.json.sig beside it.

  --version X.Y.Z  the release's version (vpk's --packVersion and the bundle's)
  --feed DIR|URL   where the app looks for updates: an https URL, or an absolute
                   directory (the spike's local feed); baked in as TENMOKU_UPDATE_FEED
  --out DIR        vpk's output directory; pack each version into the same one
  --channel NAME   the channel (default: spike); 'osx' (vpk's default) and 'stable'
                   are refused, so a spike can't reach a production channel
  --pack-id ID     the Velopack app id (default: TenmokuSpike, not the bundle id)
  --dry-run        print the cargo, bundle, vpk and sign commands; run none of them
  -h, --help       this text

Keys (nothing secret on a command line):
  TENMOKU_UPDATE_PUBLIC_KEY       the base64 Ed25519 public key baked into the app
  TENMOKU_UPDATE_PRIVATE_KEY      the base64 private key (a 32-byte seed), or
  TENMOKU_UPDATE_PRIVATE_KEY_FILE a file holding it; either goes to the signer's
                                  stdin. Make a pair with:
      cargo run -p fastedit --example sign_update_feed -- generate --out-private FILE

Code signing and notarization, passed to vpk only when set:
  VPK_SIGN_APP_IDENTITY      --signAppIdentity: '-' for an ad-hoc signature, or e.g.
                             "Developer ID Application: Name". A real pack needs it:
                             without one, vpk signs nothing after adding UpdateMac and
                             sq.version, and the bundle's seal is broken.
  VPK_SIGN_INSTALL_IDENTITY  --signInstallIdentity, e.g. "Developer ID Installer: Name"
                             (needs VPK_SIGN_APP_IDENTITY)
  VPK_NOTARY_PROFILE         --notaryProfile, a notarytool keychain profile
                             (needs both identities)
  VPK_KEYCHAIN               --keychain, the keychain file codesign and notarytool use
vpk wants each identity WITHOUT its "(TEAMID)" suffix. vpk signs the app with --deep
and --signEntitlements (crates/fastedit/macos/Tenmoku.entitlements).

  VPK   the vpk to run (default: vpk in PATH, else ~/.dotnet/tools/vpk). It must be the
        version the velopack crate is pinned to in Cargo.toml.
EOF
}

die() {
	echo "velopack-pack: $*" >&2
	exit 1
}

need_arg() {
	[ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value (see --help)"
}

# Prints its arguments as one shell-quoted command line.
show() {
	line=""
	for arg in "$@"; do
		case "$arg" in
		"" | *[!A-Za-z0-9_./:=@%+,-]*) arg="'$(printf '%s' "$arg" | sed "s/'/'\\\\''/g")'" ;;
		esac
		line="$line${line:+ }$arg"
	done
	printf '%s\n' "$line"
}

version=""
feed=""
out=""
channel=spike
pack_id=TenmokuSpike
dry_run=""
while [ $# -gt 0 ]; do
	case "$1" in
	--version) need_arg "$@"; version=$2; shift 2 ;;
	--feed) need_arg "$@"; feed=$2; shift 2 ;;
	--out) need_arg "$@"; out=$2; shift 2 ;;
	--channel) need_arg "$@"; channel=$2; shift 2 ;;
	--pack-id) need_arg "$@"; pack_id=$2; shift 2 ;;
	--dry-run) dry_run=1; shift ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

[ -n "$version" ] || die "--version is required (see --help)"
[ -n "$feed" ] || die "--feed is required (see --help)"
[ -n "$out" ] || die "--out is required (see --help)"
case "$version" in
*[!0-9.]* | .* | *. | *..*) die "version must be dotted numbers, got '$version'" ;;
esac
case "$feed" in
https://?*) ;;
/*) ;;
*) die "the feed must be an https URL or an absolute directory, got '$feed'" ;;
esac
case "$feed" in
*[[:space:]]*) die "the feed has whitespace in it: '$feed'" ;;
esac
case "$channel" in
*[!A-Za-z0-9._-]*) die "the channel may hold only letters, digits, '.', '_' and '-', got '$channel'" ;;
esac
lower=$(printf '%s' "$channel" | tr '[:upper:]' '[:lower:]')
case "$lower" in
osx | stable) die "refusing channel '$channel': the spike stays off production channels" ;;
esac
case "$pack_id" in
*[!A-Za-z0-9._-]*) die "the pack id may hold only letters, digits, '.', '_' and '-', got '$pack_id'" ;;
esac

[ -n "${TENMOKU_UPDATE_PUBLIC_KEY:-}" ] || die "TENMOKU_UPDATE_PUBLIC_KEY is not set (see --help)"
public_key=$TENMOKU_UPDATE_PUBLIC_KEY
case "$public_key" in
*[!A-Za-z0-9+/=]*) die "TENMOKU_UPDATE_PUBLIC_KEY is not base64" ;;
esac
if [ -n "${TENMOKU_UPDATE_PRIVATE_KEY:-}" ] && [ -n "${TENMOKU_UPDATE_PRIVATE_KEY_FILE:-}" ]; then
	die "set TENMOKU_UPDATE_PRIVATE_KEY or TENMOKU_UPDATE_PRIVATE_KEY_FILE, not both"
elif [ -n "${TENMOKU_UPDATE_PRIVATE_KEY:-}" ]; then
	key_from="TENMOKU_UPDATE_PRIVATE_KEY"
elif [ -n "${TENMOKU_UPDATE_PRIVATE_KEY_FILE:-}" ]; then
	key_from="TENMOKU_UPDATE_PRIVATE_KEY_FILE"
	[ -r "$TENMOKU_UPDATE_PRIVATE_KEY_FILE" ] ||
		die "cannot read TENMOKU_UPDATE_PRIVATE_KEY_FILE: $TENMOKU_UPDATE_PRIVATE_KEY_FILE"
else
	die "no private key: set TENMOKU_UPDATE_PRIVATE_KEY or TENMOKU_UPDATE_PRIVATE_KEY_FILE (see --help)"
fi

app_identity=${VPK_SIGN_APP_IDENTITY:-}
install_identity=${VPK_SIGN_INSTALL_IDENTITY:-}
notary_profile=${VPK_NOTARY_PROFILE:-}
keychain=${VPK_KEYCHAIN:-}
if [ -n "$install_identity" ] && [ -z "$app_identity" ]; then
	die "VPK_SIGN_INSTALL_IDENTITY needs VPK_SIGN_APP_IDENTITY too"
fi
if [ -n "$notary_profile" ] && { [ -z "$app_identity" ] || [ -z "$install_identity" ]; }; then
	die "VPK_NOTARY_PROFILE needs VPK_SIGN_APP_IDENTITY and VPK_SIGN_INSTALL_IDENTITY"
fi
if [ -n "$notary_profile" ] && [ "$app_identity" = - ]; then
	die "VPK_NOTARY_PROFILE needs a Developer ID app identity: an ad-hoc signature can't be notarized"
fi
no_identity="no VPK_SIGN_APP_IDENTITY: a pack would refuse (set it to '-' for an ad-hoc signature);
without one, vpk signs nothing after adding UpdateMac and sq.version, and the bundle's seal is broken"

pin=$(sed -n 's/^velopack = { version = "=\([^"]*\)".*/\1/p' "$root/Cargo.toml")
[ -n "$pin" ] || die "no pinned velopack version in $root/Cargo.toml"

# Where cargo builds: CARGO_TARGET_DIR, or a cargo config's build.target-dir.
target=$(cd "$root" && cargo metadata --format-version 1 --no-deps 2>/dev/null |
	sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p') || true
[ -n "$target" ] || target=${CARGO_TARGET_DIR:-$root/target}
binary="$target/release/tenmoku"
signer="$target/release/examples/sign_update_feed"
app="$root/target/bundle/Tenmoku.app"
feed_file="$out/releases.$channel.json"

vpk=${VPK:-}
if [ -z "$vpk" ]; then
	vpk=$(command -v vpk || true)
	[ -n "$vpk" ] || vpk="$HOME/.dotnet/tools/vpk"
fi

set -- "$vpk" pack --packId "$pack_id" --packVersion "$version" --packDir "$app" \
	--mainExe tenmoku --channel "$channel" --outputDir "$out" --signEntitlements "$entitlements"
[ -n "$app_identity" ] && set -- "$@" --signAppIdentity "$app_identity"
[ -n "$install_identity" ] && set -- "$@" --signInstallIdentity "$install_identity"
[ -n "$notary_profile" ] && set -- "$@" --notaryProfile "$notary_profile"
[ -n "$keychain" ] && set -- "$@" --keychain "$keychain"

if [ -n "$dry_run" ]; then
	show env TENMOKU_UPDATE_FEED="$feed" TENMOKU_UPDATE_PUBLIC_KEY="$public_key" \
		cargo build --release -p fastedit --bin tenmoku --example sign_update_feed
	show env -u TENMOKU_FEED_URL -u TENMOKU_ED_PUBLIC_KEY -u SPARKLE_FRAMEWORK \
		"$here/bundle-macos.sh" --binary "$binary" --version "$version"
	show "$@"
	printf '%s | %s\n' "<private key from $key_from>" "$(show "$signer" sign "$feed_file")"
	[ -n "$app_identity" ] || echo "velopack-pack: note: $no_identity" >&2
	exit 0
fi

[ -n "$app_identity" ] || die "$no_identity"

[ "$(uname -s)" = Darwin ] || die "macOS only (vpk pack for osx, codesign)"

# vpk is a .NET tool: a user-local SDK needs DOTNET_ROOT.
if [ -z "${DOTNET_ROOT:-}" ] && [ -x "$HOME/.dotnet/dotnet" ]; then
	DOTNET_ROOT="$HOME/.dotnet"
	export DOTNET_ROOT
fi
install_hint="dotnet tool install vpk --version $pin --tool-path ~/.dotnet/tools"
[ -x "$vpk" ] || die "no vpk at '$vpk'; install it with: $install_hint"
vpk_version=$("$vpk" -h 2>&1 | sed -n 's/.*Velopack CLI \([^ ]*\).*/\1/p' | head -n 1)
[ "$vpk_version" = "$pin" ] ||
	die "vpk is '${vpk_version:-unknown}', not $pin as the velopack crate is pinned; install it with: $install_hint"

mkdir -p "$out"

# The key reaches the signer only: unexported before cargo, bundle-macos.sh and vpk run.
private_key=${TENMOKU_UPDATE_PRIVATE_KEY:-}
private_key_file=${TENMOKU_UPDATE_PRIVATE_KEY_FILE:-}
unset TENMOKU_UPDATE_PRIVATE_KEY TENMOKU_UPDATE_PRIVATE_KEY_FILE

TENMOKU_UPDATE_FEED="$feed" TENMOKU_UPDATE_PUBLIC_KEY="$public_key" \
	cargo build --release -p fastedit --bin tenmoku --example sign_update_feed ||
	die "cargo build failed"

env -u TENMOKU_FEED_URL -u TENMOKU_ED_PUBLIC_KEY -u SPARKLE_FRAMEWORK \
	"$here/bundle-macos.sh" --binary "$binary" --version "$version" >/dev/null ||
	die "bundle-macos.sh failed"

"$@" || die "vpk pack failed"
[ -f "$feed_file" ] || die "vpk wrote no $feed_file"

rm -f "$feed_file.sig"
if [ "$key_from" = TENMOKU_UPDATE_PRIVATE_KEY ]; then
	# printf is a builtin: the key reaches the signer's stdin and no argv.
	signed=$(printf '%s' "$private_key" | "$signer" sign "$feed_file") ||
		die "signing $feed_file failed"
else
	signed=$("$signer" sign "$feed_file" <"$private_key_file") ||
		die "signing $feed_file failed"
fi
if [ "$signed" != "$public_key" ]; then
	rm -f "$feed_file.sig"
	die "the private key's public half is not TENMOKU_UPDATE_PUBLIC_KEY; no signature written"
fi
echo "$feed_file.sig"
