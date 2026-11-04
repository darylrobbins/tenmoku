#!/bin/sh
# Fixture test for scripts/make-appcast.sh: a small zip laid out as a release is, no network.
# Prints one line per check and exits non-zero on the first failure. macOS or Linux; needs
# xmllint, zip and unzip.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
make="$here/make-appcast.sh"
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

fail() {
	echo "FAIL: $*"
	exit 1
}
ok() { echo "ok: $*"; }

plist() {
	cat >"$1" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
	<key>CFBundleVersion</key>
	<string>$2</string>
	<!-- The real one has comments among its keys. -->
	<key>LSMinimumSystemVersion</key>
	<!-- And one here, so a value is the next element, not the next node. -->
	<string>$3</string>
</dict></plist>
EOF
}

# Sparkle's Updater.app goes in first, with other values: its Info.plist must not be the one read.
updater=Tenmoku.app/Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app/Contents
mkdir -p "$t/z/$updater"
plist "$t/z/$updater/Info.plist" 9.9.9 99.0
plist "$t/z/Tenmoku.app/Contents/Info.plist" 1.2.3 11.0
(cd "$t/z" && zip -q ../app.zip "$updater/Info.plist" Tenmoku.app/Contents/Info.plist)
zip="$t/app.zip"
size=$(wc -c <"$zip" | tr -d ' ')

url='https://example.test/dl/v1.2.3/app.zip?a=1&b=2'
notes='https://example.test/notes?v=1&w=2'
sig='d1S3994qdXXhjyqoBpiQ5mbdjR7kd1LCFmUXWP2/vwj609U7syIB+/Eyvg8+M5IyplR0TC5HLdrWsbitMCsUAQ=='
sparkle_ns=http://www.andymatuschak.org/xml-namespaces/sparkle
out="$t/appcast.xml"

# make-appcast.sh with every required argument, and the arguments given added after them.
run() {
	"$make" --zip "$zip" --version 1.2.3 --download-url "$url" --signature "$sig" --out "$out" "$@"
}
# The value of an XPath 1.0 string expression, names matched by local name in $out.
x() {
	xmllint --xpath "string($1)" "$out"
}
expect() {
	got=$(x "$2")
	[ "$got" = "$3" ] || fail "$1: got '$got', expected '$3'"
	ok "$1 is $3"
}
elem() { echo "//*[local-name()='$1']"; }
enc="//*[local-name()='enclosure']"

[ "$(run)" = "$out" ] || fail "the last line is not the appcast path"
xmllint --noout "$out" || fail "xmllint refused the appcast"
ok "xmllint accepts the appcast"

expect "the RSS version" "/rss/@version" 2.0
expect "sparkle's namespace" "namespace-uri($(elem version))" "$sparkle_ns"
[ "$(x "count(//item)")" = 1 ] || fail "not one item: $(x "count(//item)")"
ok "one item"
expect "sparkle:version" "$(elem version)" 1.2.3
expect "sparkle:shortVersionString" "$(elem shortVersionString)" 1.2.3
expect "sparkle:minimumSystemVersion, from the app's own Info.plist" "$(elem minimumSystemVersion)" 11.0
expect "the enclosure's url, unescaped" "$enc/@url" "$url"
expect "the enclosure's length, the file's size" "$enc/@length" "$size"
expect "the enclosure's type" "$enc/@type" application/octet-stream
expect "sparkle:edSignature" "$enc/@*[local-name()='edSignature']" "$sig"
expect "the signature's namespace" "namespace-uri($enc/@*[local-name()='edSignature'])" "$sparkle_ns"
[ "$(x "count($(elem releaseNotesLink))")" = 0 ] || fail "a releaseNotesLink without --notes-url"
ok "no releaseNotesLink without --notes-url"
pub=$(x "//item/pubDate")
echo "$pub" | grep -q -E '^(Mon|Tue|Wed|Thu|Fri|Sat|Sun), [0-9]{2} (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) [0-9]{4} [0-9]{2}:[0-9]{2}:[0-9]{2} \+0000$' ||
	fail "pubDate is not RFC 822: '$pub'"
ok "pubDate is RFC 822: $pub"

run --notes-url "$notes" --min-system 12.5 >/dev/null
expect "sparkle:releaseNotesLink, unescaped" "$(elem releaseNotesLink)" "$notes"
expect "--min-system over the bundle's" "$(elem minimumSystemVersion)" 12.5
rm -f "$out"

# Each refusal leaves no appcast and names what was wrong.
refused() {
	what=$1
	word=$2
	shift 2
	if "$make" "$@" >"$t/refused.log" 2>&1; then
		fail "$what was not refused"
	fi
	grep -q -F -- "$word" "$t/refused.log" || fail "$what: the message lacks '$word': $(cat "$t/refused.log")"
	[ ! -e "$out" ] || fail "$what left an appcast"
	ok "$what is refused: $(tail -n 1 "$t/refused.log")"
}
refused "no --zip" --zip --version 1.2.3 --download-url "$url" --signature "$sig" --out "$out"
refused "no --version" --version --zip "$zip" --download-url "$url" --signature "$sig" --out "$out"
refused "no --download-url" --download-url --zip "$zip" --version 1.2.3 --signature "$sig" --out "$out"
refused "no --signature" --signature --zip "$zip" --version 1.2.3 --download-url "$url" --out "$out"
refused "no --out" --out --zip "$zip" --version 1.2.3 --download-url "$url" --signature "$sig"
refused "an empty --signature" --signature --zip "$zip" --version 1.2.3 --download-url "$url" --signature "" --out "$out"

# The arguments after the required ones override them: the last --x given wins.
refused_with() {
	what=$1
	word=$2
	shift 2
	refused "$what" "$word" --zip "$zip" --version 1.2.3 --download-url "$url" --signature "$sig" --out "$out" "$@"
}
refused_with "an http download URL" https --download-url http://example.test/app.zip
refused_with "a bare scheme" https --download-url https://
refused_with "an http notes URL" https --notes-url http://example.test/notes
refused_with "a URL with a space" whitespace --download-url 'https://example.test/a b.zip'
refused_with "a non-base64 signature" base64 --signature 'abc"/><evil'
refused_with "a two-part version" X.Y.Z --version 1.2
refused_with "a four-part version" X.Y.Z --version 1.2.3.4
refused_with "a suffixed version" X.Y.Z --version 1.2.3-rc1
refused_with "a version other than the bundle's" CFBundleVersion --version 1.2.4
refused_with "a bad --min-system" min-system --min-system 'eleven'
refused_with "a missing zip" "not found" --zip "$t/absent.zip"
(cd "$t/z" && zip -q ../other.zip "$updater/Info.plist")
refused_with "a zip without the app" "does not hold" --zip "$t/other.zip"
refused_with "an unknown argument" unknown --bogus

echo ok
