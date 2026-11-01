#!/bin/sh
# Developer ID signing and notarization of an already-bundled Tenmoku.app. See --help.
# Runs anywhere: with no certificate secret it says so and leaves the ad-hoc signature.
#
# Never echo a secret and never enable shell tracing here: CI logs are public.
set -eu

here=$(cd "$(dirname "$0")" && pwd)

usage() {
	cat <<EOF
Usage: scripts/sign-notarize-macos.sh APP_PATH ZIP_PATH

Re-signs APP_PATH (a Tenmoku.app from scripts/bundle-macos.sh) with a Developer ID
certificate, then notarizes and staples it. ZIP_PATH is where the zip uploaded to
Apple's notary service is written; it is a scratch file, not the release zip (zip
the app again after this script, once the ticket is stapled).

Each stage runs only when its environment variables are set and non-empty, and
prints a "skipping" line otherwise. The release workflow sets them from the
repository secrets of the same names (docs/release/macos-signing.md, "Releasing").

  Signing:       MACOS_CERTIFICATE (a base64 .p12), MACOS_CERTIFICATE_PASSWORD,
                 MACOS_SIGN_IDENTITY ("Developer ID Application: <name> (<team>)")
  Notarization:  APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD (app-specific password);
                 only after signing

Without a certificate the bundle keeps its ad-hoc signature, which this script
verifies, and it exits 0. It exits non-zero on any failure of a stage that runs.
EOF
}

die() {
	echo "sign-notarize-macos: $*" >&2
	exit 1
}

case "${1:-}" in
-h | --help) usage; exit 0 ;;
esac
[ $# -eq 2 ] || die "expected APP_PATH ZIP_PATH (see --help)"
app=$1
zip=$2
[ "$(uname -s)" = Darwin ] || die "macOS only (needs codesign, security and xcrun)"
[ -d "$app/Contents/MacOS" ] || die "not an app bundle: $app"

if [ -z "${MACOS_CERTIFICATE:-}" ]; then
	echo "skipping Developer ID signing and notarization: no certificate secret"
	codesign --verify --strict --deep "$app" || die "the bundle's ad-hoc signature does not verify: $app"
	echo "$app stays ad-hoc signed"
	exit 0
fi
[ -n "${MACOS_CERTIFICATE_PASSWORD:-}" ] || die "a certificate is set but its password secret is empty"
[ -n "${MACOS_SIGN_IDENTITY:-}" ] || die "a certificate is set but the signing identity secret is empty"

# A throwaway keychain holds the certificate for this run only. Its password is random
# and never leaves this process; the trap deletes it and restores the search list.
work=$(mktemp -d)
keychain="$work/tenmoku-signing.keychain-db"
saved_list="$work/keychains"
security list-keychains -d user | sed 's/^[[:space:]]*"//; s/"[[:space:]]*$//' >"$saved_list"
cleanup() {
	if [ -s "$saved_list" ]; then
		# Word-split on newlines only: keychain paths may hold spaces.
		old_ifs=$IFS
		IFS='
'
		# shellcheck disable=SC2046
		security list-keychains -d user -s $(cat "$saved_list") || true
		IFS=$old_ifs
	fi
	security delete-keychain "$keychain" 2>/dev/null || true
	rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

keychain_pass=$(openssl rand -hex 24)
security create-keychain -p "$keychain_pass" "$keychain" || die "could not create a keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_pass" "$keychain" || die "could not unlock the keychain"
printf '%s' "$MACOS_CERTIFICATE" | base64 --decode >"$work/certificate.p12" ||
	die "the certificate secret is not valid base64"
security import "$work/certificate.p12" -k "$keychain" -f pkcs12 \
	-P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null ||
	die "could not import the certificate (wrong password, or not a .p12?)"
rm -f "$work/certificate.p12"
# Let codesign use the key without a UI prompt, which a CI runner cannot answer.
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_pass" "$keychain" >/dev/null ||
	die "could not allow codesign to use the key"
old_ifs=$IFS
IFS='
'
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(cat "$saved_list")
IFS=$old_ifs

SIGN_IDENTITY="$MACOS_SIGN_IDENTITY" "$here/bundle-macos.sh" --sign-only "$app" >/dev/null ||
	die "Developer ID signing failed"
echo "signed $app with the Developer ID certificate"

rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip" || die "ditto could not zip $app"

if [ -z "${APPLE_ID:-}" ] || [ -z "${APPLE_TEAM_ID:-}" ] || [ -z "${APPLE_APP_PASSWORD:-}" ]; then
	echo "skipping notarization: the Apple ID, team ID or app-specific password secret is empty"
	echo "$app is Developer ID signed but not notarized; Gatekeeper will warn on other Macs"
	exit 0
fi

echo "submitting $zip for notarization (this usually takes a few minutes)"
result="$work/notary.json"
xcrun notarytool submit "$zip" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
	--password "$APPLE_APP_PASSWORD" --wait --output-format json >"$result" ||
	{ cat "$result" >&2; die "notarytool submit failed"; }
if ! grep -q '"status" *: *"Accepted"' "$result"; then
	cat "$result" >&2
	id=$(sed -n 's/.*"id" *: *"\([^"]*\)".*/\1/p' "$result")
	if [ -n "$id" ]; then
		xcrun notarytool log "$id" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
			--password "$APPLE_APP_PASSWORD" >&2 || true
	fi
	die "notarization was not accepted"
fi
echo "notarization accepted"

xcrun stapler staple "$app" || die "stapler could not staple $app"
xcrun stapler validate "$app" || die "the stapled ticket does not validate"
codesign --verify --strict --deep "$app" || die "codesign --verify failed after stapling"
echo "$app is signed, notarized and stapled"
