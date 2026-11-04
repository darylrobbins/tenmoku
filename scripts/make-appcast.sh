#!/bin/sh
# Write a one-item Sparkle appcast for a release zip. See --help.
set -eu

usage() {
	cat <<EOF
Usage: scripts/make-appcast.sh --zip PATH --version X.Y.Z --download-url URL
                               --signature BASE64 --out appcast.xml
                               [--notes-url URL] [--min-system 11.0]

Writes an RSS feed with one item, the release in PATH. One is enough: Sparkle updates any
older version straight to the latest. The file is written only once xmllint accepts it.

  --zip PATH           the release zip, holding Tenmoku.app at its top; the enclosure's
                       length is its size
  --version X.Y.Z      sparkle:version and sparkle:shortVersionString; must be the bundle's
                       CFBundleVersion, or Sparkle would offer this release to itself forever
  --download-url URL   where the zip is published, https only
  --signature BASE64   the zip's EdDSA signature, from Sparkle's signing tool. This script
                       only takes the result and never sees the key that made it.
  --out PATH           the appcast to write; its directory must exist
  --notes-url URL      sparkle:releaseNotesLink, https only (default: none)
  --min-system X.Y     sparkle:minimumSystemVersion (default: the bundle's
                       LSMinimumSystemVersion)
  -h, --help           this text

Needs unzip and xmllint. It exits non-zero on any refusal.
EOF
}

die() {
	echo "make-appcast: $*" >&2
	exit 1
}

need_arg() {
	[ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value (see --help)"
}

zip=""
version=""
download_url=""
signature=""
out=""
notes_url=""
min_system=""
while [ $# -gt 0 ]; do
	case "$1" in
	--zip) need_arg "$@"; zip=$2; shift 2 ;;
	--version) need_arg "$@"; version=$2; shift 2 ;;
	--download-url) need_arg "$@"; download_url=$2; shift 2 ;;
	--signature) need_arg "$@"; signature=$2; shift 2 ;;
	--out) need_arg "$@"; out=$2; shift 2 ;;
	--notes-url) need_arg "$@"; notes_url=$2; shift 2 ;;
	--min-system) need_arg "$@"; min_system=$2; shift 2 ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

for pair in "--zip:$zip" "--version:$version" "--download-url:$download_url" \
	"--signature:$signature" "--out:$out"; do
	[ -n "${pair#*:}" ] || die "${pair%%:*} is required (see --help)"
done

check_url() {
	case "$2" in
	https://?*) ;;
	*) die "$1 must be https, got '$2'" ;;
	esac
	case "$2" in
	*[[:space:][:cntrl:]]*) die "$1 has whitespace or a control character in it: '$2'" ;;
	esac
}

is_dotted() {
	case "$1" in
	*[!0-9.]* | .* | *. | *..*) return 1 ;;
	esac
}

is_dotted "$version" && [ "$version" != "${version#*.*.}" ] && [ "${version#*.*.*.}" = "$version" ] ||
	die "--version must be X.Y.Z, got '$version'"
check_url --download-url "$download_url"
[ -z "$notes_url" ] || check_url --notes-url "$notes_url"
[ -z "$min_system" ] || is_dotted "$min_system" || die "--min-system must be dotted numbers, got '$min_system'"
case "$signature" in
*[!A-Za-z0-9+/=]*) die "--signature must be base64, got '$signature'" ;;
esac
[ -f "$zip" ] || die "zip not found: $zip"

# By its full name: Sparkle's Updater.app has a Contents/Info.plist of its own inside the zip.
info=Tenmoku.app/Contents/Info.plist
plist=$(unzip -p "$zip" "$info" 2>/dev/null) || die "$zip does not hold $info"
plist_value() {
	printf '%s\n' "$plist" |
		xmllint --nonet --xpath "string(/plist/dict/key[.='$1']/following-sibling::*[1])" - 2>/dev/null ||
		die "could not read $1 from $info in $zip"
}
bundle_version=$(plist_value CFBundleVersion)
[ "$bundle_version" = "$version" ] ||
	die "the bundle's CFBundleVersion is '$bundle_version', not --version '$version'"
if [ -z "$min_system" ]; then
	min_system=$(plist_value LSMinimumSystemVersion)
	is_dotted "$min_system" && [ -n "$min_system" ] ||
		die "the bundle's LSMinimumSystemVersion is '$min_system', not dotted numbers"
fi

xml_escape() {
	printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g' -e "s/'/\&apos;/g"
}

length=$(wc -c <"$zip" | tr -d ' ')
# RFC 822 names its days and months in English, whatever the machine's locale.
pub_date=$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S +0000')
notes_line=""
if [ -n "$notes_url" ]; then
	notes_line="
      <sparkle:releaseNotesLink>$(xml_escape "$notes_url")</sparkle:releaseNotesLink>"
fi

tmp="$out.tmp.$$"
trap 'rm -f "$tmp"' EXIT
cat >"$tmp" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Tenmoku</title>
    <item>
      <title>Version $version</title>
      <pubDate>$pub_date</pubDate>
      <sparkle:version>$version</sparkle:version>
      <sparkle:shortVersionString>$version</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>$min_system</sparkle:minimumSystemVersion>$notes_line
      <enclosure url="$(xml_escape "$download_url")" length="$length" type="application/octet-stream" sparkle:edSignature="$signature"/>
    </item>
  </channel>
</rss>
EOF
xmllint --nonet --noout "$tmp" || die "the appcast did not pass xmllint"
mv "$tmp" "$out"
echo "$out"
