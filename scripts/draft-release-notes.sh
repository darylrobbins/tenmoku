#!/bin/sh
# Draft release notes from git log, grouped by task type. See --help.
set -eu

usage() {
	cat <<'EOF'
Usage: scripts/draft-release-notes.sh [--since TAG] [--out PATH]

Prints a skeleton of release-notes/X.Y.Z.md for a person to edit, one bullet per commit
subject in TAG..HEAD of the repository in the current directory (merges left out), oldest
first, with the trailing task handles taken off.

  --since TAG   where to start (default: the latest v* tag, by version)
  --out PATH    write the draft there instead of printing it; refused if PATH exists
  -h, --help    this text

A subject's task is its "(Mac T<n>)", else its last "(T<n>)". Its type comes from
`rgt task show T<n> --json --fields type` (read only), and picks the section:

  ## New                               feature
  ## Fixed                             bugfix
  ## Internal (delete before release)  refactor, chore and docs; and whatever the type,
                                       subjects starting "Flake:" or "Spec T<n>:" and
                                       version bumps ("Bump the workspace version ...")
  ## Unsorted                          no task, or another type

RGT names the rgt command (default: rgt on PATH); without one every line is Unsorted. No
network beyond what rgt does, and no LLM. scripts/check-release-version.sh refuses the
Internal and Unsorted headings, so an unedited draft can't be tagged: move each line worth
telling a user into ## New, ## Improved or ## Fixed, reword it for them, and delete the rest.
EOF
}

die() {
	echo "draft-release-notes: $*" >&2
	exit 1
}

need_arg() {
	[ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value (see --help)"
}

since=""
out=""
while [ $# -gt 0 ]; do
	case "$1" in
	--since) need_arg "$@"; since=$2; shift 2 ;;
	--out) need_arg "$@"; out=$2; shift 2 ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

git rev-parse --git-dir >/dev/null 2>&1 || die "not in a git repository"
if [ -z "$since" ]; then
	since=$(git tag --list 'v*' --sort=-v:refname | head -n 1)
	[ -n "$since" ] || die "no v* tag to start from: pass --since"
fi
git rev-parse --verify --quiet "$since^{commit}" >/dev/null || die "--since $since is not a commit or tag"
[ -z "$out" ] || [ ! -e "$out" ] || die "$out exists: delete it, or pass another --out"

rgt=${RGT:-rgt}
command -v "$rgt" >/dev/null 2>&1 || {
	echo "draft-release-notes: no $rgt to ask for task types: every line is under ## Unsorted" >&2
	rgt=""
}

t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
git log --no-merges --reverse --format=%s "$since..HEAD" >"$t/subjects"
echo "draft-release-notes: $(wc -l <"$t/subjects" | tr -d ' ') commits in $since..HEAD" >&2

# type_of T<n>: the task's type from rgt, or nothing. One rgt call per task.
: >"$t/types"
type_of() {
	[ -n "$rgt" ] || return 0
	known=$(awk -v id="$1" '$1 == id { print $2; found = 1 } END { exit !found }' "$t/types") && {
		[ "$known" = - ] || echo "$known"
		return 0
	}
	got=$("$rgt" task show "$1" --json --fields type 2>/dev/null |
		sed -n 's/.*"ok":true.*"type":"\([a-z_-]*\)".*/\1/p' | head -n 1) || got=""
	echo "$1 ${got:--}" >>"$t/types"
	echo "$got"
}

: >"$t/new"
: >"$t/fixed"
: >"$t/internal"
: >"$t/unsorted"
while IFS= read -r subject; do
	# The task: (Mac T<n>) anywhere, else the last (T<n>).
	task=$(printf '%s\n' "$subject" | sed -n 's/.*(Mac \(T[0-9][0-9]*\)).*/\1/p')
	[ -n "$task" ] || task=$(printf '%s\n' "$subject" | sed -n 's/.*(\(T[0-9][0-9]*\)).*/\1/p')
	# The line: the subject without its trailing handles, "(T<n>)" and "(Mac T<n>)".
	line=$(printf '%s\n' "$subject" | sed -E ':a
s/[[:space:]]*\((Mac )?T[0-9]+\)[[:space:]]*$//
ta')
	case "$subject" in
	*) [ -n "$rgt" ] || { printf -- '- %s\n' "$line" >>"$t/unsorted"; continue; } ;;
	esac
	case "$subject" in
	Flake:* | "Spec T"[0-9]*:* | "Bump the workspace version "* | "Bump version "*) section=internal ;;
	*)
		type=""
		[ -z "$task" ] || type=$(type_of "$task")
		case "$type" in
		feature) section=new ;;
		bugfix) section=fixed ;;
		refactor | chore | docs) section=internal ;;
		*) section=unsorted ;;
		esac
		;;
	esac
	printf -- '- %s\n' "$line" >>"$t/$section"
done <"$t/subjects"

draft() {
	first=1
	for pair in "new:## New" "fixed:## Fixed" "internal:## Internal (delete before release)" "unsorted:## Unsorted"; do
		file="$t/${pair%%:*}"
		[ -s "$file" ] || continue
		[ "$first" = 1 ] || echo
		first=0
		printf '%s\n\n' "${pair#*:}"
		cat "$file"
	done
}
if [ -n "$out" ]; then
	draft >"$out"
	echo "$out"
else
	draft
fi
