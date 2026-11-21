#!/bin/sh
# Write a one-item Sparkle appcast for a release zip. See --help.
set -eu

usage() {
	cat <<EOF
Usage: scripts/make-appcast.sh --zip PATH --version VERSION --download-url URL
                               --signature BASE64 --out appcast.xml [--channel-feed preview]
                               [--notes-file PATH | --notes-url URL] [--min-system 11.0]

Writes an RSS feed with one item, the release in PATH. One is enough: Sparkle updates any
older version straight to the latest. The file is written only once xmllint accepts it.

  --zip PATH           the release zip, holding Tenmoku.app at its top; the enclosure's
                       length is its size
  --version VERSION    X.Y.Z, or X.Y.Z-preview.N with --channel-feed preview. The display
                       version, sparkle:shortVersionString. sparkle:version is the bundle's
                       build version, which the bundle's CFBundleVersion must equal (X.Y.Z, or
                       X.Y.Z.preview.N as bundle-macos.sh writes it), or Sparkle would offer
                       this release to itself forever
  --channel-feed preview
                       write the preview feed: one item, a preview or a stable build, with no
                       sparkle:channel element (the feed itself is the channel). Without it a
                       preview version is refused: GitHub's latest/download never lists a
                       prerelease, so one in the stable feed would be an upload bug.
  --download-url URL   where the zip is published, https only
  --signature BASE64   the zip's EdDSA signature, from Sparkle's signing tool. This script
                       only takes the result and never sees the key that made it.
  --out PATH           the appcast to write; its directory must exist
  --notes-file PATH    the release notes (release-notes/X.Y.Z.md), as the item's
                       <description sparkle:format="markdown"> in CDATA, which Sparkle's
                       update alert shows; refused if empty or holding ]]> (default: none)
  --notes-url URL      sparkle:releaseNotesLink, https only (default: none); Sparkle shows
                       the page instead of a description, so not with --notes-file
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
notes_file=""
min_system=""
channel_feed=""
while [ $# -gt 0 ]; do
	case "$1" in
	--zip) need_arg "$@"; zip=$2; shift 2 ;;
	--version) need_arg "$@"; version=$2; shift 2 ;;
	--download-url) need_arg "$@"; download_url=$2; shift 2 ;;
	--signature) need_arg "$@"; signature=$2; shift 2 ;;
	--out) need_arg "$@"; out=$2; shift 2 ;;
	--notes-url) need_arg "$@"; notes_url=$2; shift 2 ;;
	--notes-file) need_arg "$@"; notes_file=$2; shift 2 ;;
	--channel-feed) need_arg "$@"; channel_feed=$2; shift 2 ;;
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

# X.Y.Z, or X.Y.Z-preview.N (N without a leading zero); bundle-macos.sh writes the latter's
# CFBundleVersion as X.Y.Z.preview.N, which is what Sparkle orders by.
is_xyz() {
	is_dotted "$1" && [ "$1" != "${1#*.*.}" ] && [ "${1#*.*.*.}" = "$1" ]
}
build_version=$version
preview=""
case "$version" in
*-preview.*)
	base=${version%%-preview.*}
	n=${version#*-preview.}
	is_xyz "$base" || die "--version must be X.Y.Z or X.Y.Z-preview.N, got '$version'"
	case "$n" in
	"" | *[!0-9]* | 0?*) die "--version must be X.Y.Z or X.Y.Z-preview.N, got '$version'" ;;
	esac
	build_version=$base.preview.$n
	preview=1
	;;
*) is_xyz "$version" || die "--version must be X.Y.Z or X.Y.Z-preview.N, got '$version'" ;;
esac
case "$channel_feed" in
"" | preview) ;;
*) die "--channel-feed must be preview, got '$channel_feed'" ;;
esac
[ -z "$preview" ] || [ -n "$channel_feed" ] || die "a preview must go in the preview feed (--channel-feed preview), not the stable one: '$version'"
check_url --download-url "$download_url"
[ -z "$notes_url" ] || check_url --notes-url "$notes_url"
[ -z "$min_system" ] || is_dotted "$min_system" || die "--min-system must be dotted numbers, got '$min_system'"
case "$signature" in
*[!A-Za-z0-9+/=]*) die "--signature must be base64, got '$signature'" ;;
esac
[ -f "$zip" ] || die "zip not found: $zip"
notes_text=""
if [ -n "$notes_file" ]; then
	[ -z "$notes_url" ] || die "give --notes-file or --notes-url, not both: Sparkle shows only the link"
	[ -f "$notes_file" ] || die "notes file not found: $notes_file"
	grep -q '[^[:space:]]' "$notes_file" || die "the notes file $notes_file is empty"
	! grep -q -F ']]>' "$notes_file" || die "the notes file $notes_file holds ]]>, which would end the description's CDATA"
	notes_text=$(cat "$notes_file")
fi

# By its full name: Sparkle's Updater.app has a Contents/Info.plist of its own inside the zip.
info=Tenmoku.app/Contents/Info.plist
plist=$(unzip -p "$zip" "$info" 2>/dev/null) || die "$zip does not hold $info"
plist_value() {
	printf '%s\n' "$plist" |
		xmllint --nonet --xpath "string(/plist/dict/key[.='$1']/following-sibling::*[1])" - 2>/dev/null ||
		die "could not read $1 from $info in $zip"
}
bundle_version=$(plist_value CFBundleVersion)
[ "$bundle_version" = "$build_version" ] ||
	die "the bundle's CFBundleVersion is '$bundle_version', not '$build_version' (--version '$version')"
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
# Markdown: Sparkle 2.10.1 renders it on every macOS it runs on (12.0 and later, its own
# minimum); see docs/decisions/kc9c8hcj9f-release-notes.md.
if [ -n "$notes_file" ]; then
	notes_line="$notes_line
      <description sparkle:format=\"markdown\"><![CDATA[$notes_text]]></description>"
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
      <sparkle:version>$build_version</sparkle:version>
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
