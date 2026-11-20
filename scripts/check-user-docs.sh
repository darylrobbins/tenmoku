#!/bin/sh
# Structure check for the user docs book (docs/user). No mdbook needed.
# Usage: scripts/check-user-docs.sh [book-dir]
# Fails on: a SUMMARY entry without a file, a file under src/ (other than the
# root SUMMARY.md) missing from SUMMARY, a relative link (inline or
# reference-style) whose file does not exist, a .md page without an H1, a page
# outside reference/ without a "> Status:" line. Link #fragments are not
# checked. A failing utility fails the script (exit 2), never a silent pass.
# Supported subset: inline links on one line; reference definitions, <...>
# destinations and unclosed links FAIL rather than being guessed. Fenced code
# is skipped. Indented code blocks are not recognised.
# Uses only POSIX sh, find, sed, awk, sort and test.
book=${1:-docs/user}
src=$book/src
die() { echo "check-user-docs: $*" >&2; exit 2; }
[ -f "$src/SUMMARY.md" ] || { echo "error: $src/SUMMARY.md not found"; exit 1; }

tmp=$(mktemp -d) || die "mktemp failed"
trap 'rm -rf "$tmp"' EXIT
: > "$tmp/errors"
err() { echo "$*" >> "$tmp/errors"; }

# Fence tracking: sets skip=1 for lines in or delimiting a fenced code block;
# closes only on the same character with at least the opening length.
FENCE='{
  skip = 0
  if (match($0, /^ ? ? ?(```+|~~~+)/)) {
    d = $0; sub(/^ ? ? ?/, "", d)
    c = substr(d, 1, 1); k = 0
    while (substr(d, k + 1, 1) == c) k++
    rest = substr(d, k + 1)
    if (!fence) { fence = 1; fc = c; fl = k; skip = 1 }
    else if (c == fc && k >= fl && rest ~ /^[ \t]*$/) { fence = 0; skip = 1 }
  }
  if (fence) skip = 1
}'

# count REGEX FILE: matching lines outside fenced code (``` or ~~~).
count() {
  awk -v re="$1" "$FENCE"'
    skip { next }
    $0 ~ re { n++ }
    END { print n + 0 }' "$2"
}

awk "$FENCE"'skip { next }
  /^[ ]*[-*] \[[^]]*\]\([^)]*\)/ {
    t = $0
    sub(/^[ ]*[-*] \[[^]]*\]\(/, "", t)
    sub(/\).*$/, "", t)
    sub(/#.*$/, "", t)
    print t
  }' "$src/SUMMARY.md" > "$tmp/entries" || die "awk failed on SUMMARY.md"
(cd "$src" && find . -type f) > "$tmp/found" || die "find failed"
sed 's|^\./||' "$tmp/found" > "$tmp/stripped" || die "listing sed failed"
sort "$tmp/stripped" > "$tmp/files" || die "listing sort failed"

# scan_links FILE: prints each inline link destination, or "ERR: ..." for a
# form outside the supported subset (reference definitions, <...> destinations,
# a "](" with no ")" on its line). Fenced code is skipped.
scan_links() {
  awk "$FENCE"'
    skip { next }
    {
      if ($0 ~ /^ ? ? ?\[[^]]+\]:/) { print "ERR: unsupported reference-style link definition at line " NR; next }
      line = $0
      while ((i = index(line, "](")) > 0) {
        rest = substr(line, i + 2)
        if (substr(rest, 1, 1) == "<") { print "ERR: unsupported angle-bracketed link destination at line " NR; break }
        j = index(rest, ")")
        if (j == 0) { print "ERR: link destination not closed on its line at line " NR; break }
        t = substr(rest, 1, j - 1)
        if (index(t, "(") > 0) { print "ERR: unsupported parenthesis in link destination at line " NR; break }
        sub(/[ ].*$/, "", t)
        print t
        line = substr(rest, j + 1)
      }
    }' "$1"
}

# SUMMARY.md must use only supported link forms.
scan_links "$src/SUMMARY.md" > "$tmp/slinks" || die "awk failed on SUMMARY.md"

# Every list line in SUMMARY.md must be "- [Title](path)" (marker - or *, one space).
awk "$FENCE"'skip { next }
  /^[ ]*[-*+]/ && $0 !~ /^[ ]*[-*] \[[^]]*\]\([^)]*\)/ { print "ERR: unsupported SUMMARY list entry at line " NR }' "$src/SUMMARY.md" >> "$tmp/slinks" || die "awk failed on SUMMARY.md"

while IFS= read -r l; do
  case $l in ERR:*) err "SUMMARY.md: ${l#ERR: }" ;; esac
done < "$tmp/slinks"

while IFS= read -r e; do
  [ -f "$src/$e" ] || err "SUMMARY entry has no file: $e"
done < "$tmp/entries"

while IFS= read -r f; do
  [ "$f" = SUMMARY.md ] && continue
  n=$(awk -v f="$f" '$0 == f { n++ } END { print n + 0 }' "$tmp/entries") || die "awk failed"
  [ "$n" -gt 0 ] || err "file not in SUMMARY: $f"
  case $f in *.md) ;; *) continue ;; esac
  n=$(count '^# ' "$src/$f") || die "awk failed"
  [ "$n" -gt 0 ] || err "no H1: $f"
  case $f in
    reference/*) ;;
    *) n=$(count '^> Status:' "$src/$f") || die "awk failed"
       [ "$n" -gt 0 ] || err "no Status line: $f" ;;
  esac
  scan_links "$src/$f" > "$tmp/links" || die "awk failed on $f"
  dir=$(dirname "$src/$f") || die "dirname failed"
  while IFS= read -r l; do
    case $l in
      ERR:*) err "$f: ${l#ERR: }"; continue ;;
      ''|\#*|http://*|https://*|mailto:*|*://*) continue ;;
    esac
    p=${l%%#*}
    [ -e "$dir/$p" ] || err "broken link in $f: $l"
  done < "$tmp/links"
done < "$tmp/files"

if [ -s "$tmp/errors" ]; then
  cat "$tmp/errors"
  exit 1
fi
echo "user docs structure ok"
