#!/bin/sh
# Drive a bundled Tenmoku.app with XCUITest (the T1661 spike's smoke tests). See --help.
#
# Never called from regatta.toml, nextest, CI or another script: a run takes over the
# screen, mouse and keyboard of the Mac it runs on.
set -eu

start_dir=$(pwd)
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
spec="$root/tools/e2e-macos/project.yml"
e2e_id=dev.tenmoku.tenmoku.e2e

usage() {
	cat <<EOF
Usage: scripts/e2e-macos.sh [--no-build] [--iterations N] [--only TEST] [--results DIR]
                            [--timeout SECONDS] [--dry-run]

Builds the release binary, bundles Tenmoku.app with scripts/bundle-macos.sh, gives the
bundle the id $e2e_id and re-signs it, generates the XCUITest project in
tools/e2e-macos/project.yml with xcodegen, and runs its SmokeTests with xcodebuild test.
The tests launch the bundle by URL, on a scratch project with scratch config, state and
home folders, never one of the installed Tenmoku.app copies.

  --no-build        skip 'cargo build --release -p fastedit' (the bundle is still made,
                    from the binary already built)
  --iterations N    run each test N times, relaunching the runner each time (default 1)
  --only TEST       run one test, such as testMenuFileSave
  --results DIR     where the result bundle, attachments, AX dumps and JSON summaries go
                    (default: \${CARGO_TARGET_DIR:-target}/e2e-macos/results/<time>)
  --timeout SECONDS stop the run, the test runner and the app after this long
                    (default: 600 plus 180 per iteration)
  --dry-run         print every command and run none
  -h, --help        this text

Everything else is under \${CARGO_TARGET_DIR:-target}/e2e-macos/: bundle/, the generated
TenmokuE2E.xcodeproj, DerivedData/ and scratch/. macOS only, from a logged-in GUI
session with the screen unlocked: a locked screen hangs a UI test. Xcode, xcodegen and
the Accessibility and Automation Mode grants are the user's to set up; this script never
changes System Settings. It prints the result bundle, the attachments folder and a
pass/fail line last, and exits non-zero when a test fails.
EOF
}

die() {
	echo "e2e-macos: $*" >&2
	exit 1
}

need_arg() {
	[ $# -ge 2 ] && [ -n "$2" ] || die "$1 needs a value (see --help)"
}

build=1
iterations=1
only=""
results=""
timeout=""
dry=""
while [ $# -gt 0 ]; do
	case "$1" in
	--no-build) build=""; shift ;;
	--iterations) need_arg "$@"; iterations=$2; shift 2 ;;
	--only) need_arg "$@"; only=$2; shift 2 ;;
	--results) need_arg "$@"; results=$2; shift 2 ;;
	--timeout) need_arg "$@"; timeout=$2; shift 2 ;;
	--dry-run) dry=1; shift ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
done

case "$iterations" in
"" | *[!0-9]* | 0*) die "--iterations takes a whole number from 1, got '$iterations'" ;;
esac
case "$only" in
"" | testTypeAndSaveWithCmdS | testMenuFileSave | testDumpAccessibilityTree) ;;
*) die "--only takes testTypeAndSaveWithCmdS, testMenuFileSave or testDumpAccessibilityTree, got '$only'" ;;
esac
[ -z "$timeout" ] && timeout=$((600 + 180 * iterations))
case "$timeout" in
"" | *[!0-9]* | 0*) die "--timeout takes a whole number of seconds, got '$timeout'" ;;
esac

# Whatever this script (or a tool it starts) launches keeps its state out of the user's
# real ~/.local/state/tenmoku (T1664). The tests' app gets scratch state dirs of its own.
TENMOKU_STATE_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tenmoku-e2e-state.XXXXXX")
TENMOKU_CONFIG_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tenmoku-e2e-config.XXXXXX")
trap 'rm -rf "$TENMOKU_STATE_DIR" "$TENMOKU_CONFIG_DIR"' EXIT
export TENMOKU_STATE_DIR TENMOKU_CONFIG_DIR

[ "$(uname -s)" = Darwin ] || die "macOS only (XCUITest drives AppKit apps)"

absolute() {
	case "$1" in
	/*) printf '%s\n' "$1" ;;
	*) printf '%s\n' "$start_dir/$1" ;;
	esac
}

target=$(absolute "${CARGO_TARGET_DIR:-$root/target}")
e2e="$target/e2e-macos"
stamp=$(date +%Y%m%d-%H%M%S)
results=$(absolute "${results:-$e2e/results/$stamp}")
bundle_dir="$e2e/bundle"
app="$bundle_dir/Tenmoku.app"
project="$e2e/TenmokuE2E.xcodeproj"
derived="$e2e/DerivedData"
scratch="$e2e/scratch/$stamp"
xcresult="$results/TenmokuE2E.xcresult"
attachments="$results/attachments"
xcodebuild_log="$results/xcodebuild.log"
# How long a stopped run gets between TERM and KILL.
grace=10
# Only for exercising the watchdog with a harmless stand-in; never set it otherwise.
xcodebuild=${E2E_XCODEBUILD:-xcodebuild}

# Shell-quotes one word for display, only when it needs it.
quote() {
	case "$1" in
	"" | *[!A-Za-z0-9_./:=,+@%-]*)
		printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
		;;
	*) printf '%s' "$1" ;;
	esac
}

show() {
	line=""
	for word in "$@"; do
		line="$line${line:+ }$(quote "$word")"
	done
	printf '+ %s\n' "$line"
}

# show, then " >FILE": for a command whose output goes to FILE.
show_to() {
	file=$1
	shift
	printf '%s >%s\n' "$(show "$@")" "$(quote "$file")"
}

run() {
	show "$@"
	[ -n "$dry" ] || "$@"
}

# These functions also run from the signal trap, under set -e, so every test in them is
# an if: a false [ ] ending a loop would end the script mid-cleanup.
#
# Process ownership. A run owns xcodebuild's process group, the runner (which testmanagerd
# starts) and the app under test (which launchd starts), both outside that group and
# matched by their paths in this run's build products and bundle, and every descendant
# of any of them. A tracker records each one, as "pid start-time", every second while the
# run lasts, so a descendant stays known after its parent has gone. Cleanup, which only
# the main shell runs, signals the recorded processes that are still the same process.
run_pid=""
watcher=""
tail_pid=""
tracker=""
owned="$results/.owned-processes"

roots() {
	if [ -n "$run_pid" ]; then pgrep -g "$run_pid" 2>/dev/null || true; fi
	pgrep -U "$(id -u)" -f "$derived/Build/Products/" 2>/dev/null || true
	pgrep -U "$(id -u)" -f "$app/Contents/MacOS/tenmoku" 2>/dev/null || true
}

descendants() {
	for child in $(pgrep -P "$1" 2>/dev/null || true); do
		echo "$child"
		descendants "$child"
	done
}

# A process's start time, spaces squeezed and trimmed (ps pads it); empty if it's gone.
started_at() {
	ps -o lstart= -p "$1" 2>/dev/null | tr -s ' ' | sed 's/^ //; s/ $//'
}

record_owned() {
	for owner in $(roots); do
		echo "$owner"
		descendants "$owner"
	done | while read -r pid; do
		started=$(started_at "$pid")
		if [ -n "$started" ]; then echo "$pid $started"; fi
	done >>"$owned"
}

# The recorded processes still alive (not zombies) with the start time recorded.
alive_owned() {
	[ -f "$owned" ] || return 0
	sort -u "$owned" | while read -r pid started; do
		state=$(ps -o stat= -p "$pid" 2>/dev/null) || continue
		case "$state" in Z*) continue ;; esac
		if [ "$(started_at "$pid")" = "$started" ]; then echo "$pid"; fi
	done | sort -u
}

# TERM to every owned process still alive, then KILL to any left after $grace seconds.
# Runs only in the main shell, to the end, whatever xcodebuild does meanwhile.
cleanup_run() {
	if [ -n "$tracker" ]; then
		kill "$tracker" 2>/dev/null || true
		wait "$tracker" 2>/dev/null || true
		tracker=""
	fi
	record_owned
	pids=$(alive_owned)
	# shellcheck disable=SC2086 # a list of pids
	[ -z "$pids" ] || kill -TERM $pids 2>/dev/null || true
	waited=0
	while [ -n "$pids" ] && [ "$waited" -lt "$grace" ]; do
		sleep 1
		waited=$((waited + 1))
		record_owned
		pids=$(alive_owned)
	done
	if [ -n "$pids" ]; then
		# shellcheck disable=SC2086 # one line of pids
		echo "e2e-macos: still running ${grace}s after TERM, sending KILL to:" $pids >&2
		# shellcheck disable=SC2086
		kill -KILL $pids 2>/dev/null || true
	fi
	rm -f "$owned"
}

on_signal() { # on_signal NAME STATUS
	trap - INT TERM
	echo "e2e-macos: got SIG$1; stopping xcodebuild, the runner and the app" >&2
	if [ -n "$watcher" ]; then kill "$watcher" 2>/dev/null || true; fi
	cleanup_run
	if [ -n "$tail_pid" ]; then kill "$tail_pid" 2>/dev/null || true; fi
	echo "e2e-macos: FAILED (SIG$1)"
	exit "$2"
}

# Runs a command in a process group of its own, its output in $xcodebuild_log and shown
# as it comes, then cleans up after it (cleanup_run). After $timeout seconds a watchdog
# sends TERM, and KILL $grace seconds later, to the command's group, so the wait below
# returns, and the status is 124. Everything else is cleanup_run's.
run_watched() {
	: >"$xcodebuild_log"
	: >"$owned"
	set -m
	"$@" >"$xcodebuild_log" 2>&1 &
	run_pid=$!
	set +m
	tail -f "$xcodebuild_log" &
	tail_pid=$!
	(
		while :; do
			record_owned
			sleep 1
		done
	) &
	tracker=$!
	timed_out="$results/.timed-out"
	rm -f "$timed_out"
	(
		waited=0
		while [ "$waited" -lt "$timeout" ]; do
			sleep 1
			kill -0 "$run_pid" 2>/dev/null || exit 0
			waited=$((waited + 1))
		done
		: >"$timed_out"
		echo "e2e-macos: timed out after ${timeout}s; stopping xcodebuild, the runner and the app" >&2
		kill -TERM -- "-$run_pid" 2>/dev/null || true
		waited=0
		while kill -0 -- "-$run_pid" 2>/dev/null && [ "$waited" -lt "$grace" ]; do
			sleep 1
			waited=$((waited + 1))
		done
		kill -KILL -- "-$run_pid" 2>/dev/null || true
	) &
	watcher=$!
	status=0
	wait "$run_pid" || status=$?
	# The watcher only ever touches the group, which cleanup_run covers too.
	kill "$watcher" 2>/dev/null || true
	wait "$watcher" 2>/dev/null || true
	watcher=""
	cleanup_run
	sleep 1
	kill "$tail_pid" 2>/dev/null || true
	wait "$tail_pid" 2>/dev/null || true
	tail_pid=""
	if [ -f "$timed_out" ]; then
		rm -f "$timed_out"
		status=124
	fi
	return "$status"
}

# The tests' own text attachments (ax-tree-*, launched-*, window-title-*, file-menu-*,
# scratch-*, tenmoku-log-*), copied out of the export under the names the tests gave them.
copy_named_attachments() {
	manifest="$attachments/manifest.json"
	[ -f "$manifest" ] || return 1
	t=0
	while plutil -extract "$t" raw -o - "$manifest" >/dev/null 2>&1; do
		a=0
		while file=$(plutil -extract "$t.attachments.$a.exportedFileName" raw -o - "$manifest" 2>/dev/null); do
			name=$(plutil -extract "$t.attachments.$a.suggestedHumanReadableName" raw -o - "$manifest") ||
				return 1
			case "$name" in
			ax-tree-* | launched-* | window-title-* | file-menu-* | scratch-* | tenmoku-log-*)
				# XCTest appends _<n>_<UUID> to a name; drop it and the extension.
				base=$(printf '%s' "$name" | sed -E 's/(_[0-9]+_[0-9A-F-]{36})?(\.[a-z]+)?$//')
				cp "$attachments/$file" "$results/$base.txt" || return 1
				;;
			esac
			a=$((a + 1))
		done
		t=$((t + 1))
	done
}

if [ -n "$dry" ]; then
	echo "# dry run: nothing below is run"
	echo "# would refuse unless 'launchctl managername' is Aqua, /dev/console is $(id -un) and the screen is unlocked"
else
	[ "$(launchctl managername 2>/dev/null)" = Aqua ] ||
		die "no GUI session (launchctl managername is not Aqua): run from a Terminal on the Mac's own login session"
	[ "$(stat -f %Su /dev/console)" = "$(id -un)" ] ||
		die "the console belongs to $(stat -f %Su /dev/console), not $(id -un): log in at the Mac first"
	# A stand-in xcodebuild (E2E_XCODEBUILD) drives no UI, so a locked screen can't hang it.
	if [ -z "${E2E_XCODEBUILD:-}" ] &&
		ioreg -n Root -d1 -a | grep -A1 CGSSessionScreenIsLocked | grep -q '<true/>'; then
		die "the screen is locked, and a UI test hangs on a locked screen: unlock it and rerun"
	fi
	command -v xcodegen >/dev/null 2>&1 || die "xcodegen not found (brew install xcodegen; it is in the Brewfile)"
	command -v xcodebuild >/dev/null 2>&1 || die "xcodebuild not found: install Xcode and select it with xcode-select"
fi

cd "$root"

if [ -n "$build" ]; then
	cargo=cargo
	if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
		cargo="$HOME/.cargo/bin/cargo"
	fi
	run env CARGO_TARGET_DIR="$target" "$cargo" build --release -p fastedit
fi

# A bundle of its own with an id of its own: the installed copies in /Applications and
# ~/Applications share dev.tenmoku.tenmoku, and are never launched or touched.
run "$root/scripts/bundle-macos.sh" --binary "$target/release/tenmoku" --out "$bundle_dir"
run /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $e2e_id" "$app/Contents/Info.plist"
run "$root/scripts/bundle-macos.sh" --sign-only "$app"

# Xcode 27's test runner is sandboxed and can't write here, so the scratch sets are made
# now, one per test run: a project with an empty notes.txt, config, a state folder whose
# setup.toml keeps the setup wizard away, and a home for the app's log.
tests=3
[ -n "$only" ] && tests=1
pool=$((tests * iterations))
run mkdir -p "$results"
echo "+ for n in 001 .. $(printf '%03d' "$pool"): mkdir -p $(quote "$scratch")/\$n/{project,config,state,home};" \
	": >\$n/project/notes.txt; echo 'outcome = \"skipped\"' >\$n/state/setup.toml"
if [ -z "$dry" ]; then
	n=1
	while [ "$n" -le "$pool" ]; do
		set_dir="$scratch/$(printf '%03d' "$n")"
		mkdir -p "$set_dir/project" "$set_dir/config" "$set_dir/state" "$set_dir/home"
		: >"$set_dir/project/notes.txt"
		echo 'outcome = "skipped"' >"$set_dir/state/setup.toml"
		n=$((n + 1))
	done
fi
run xcodegen generate --spec "$spec" --project "$e2e"

only_args=""
[ -n "$only" ] && only_args="-only-testing:TenmokuE2E/SmokeTests/$only"
iteration_args=""
[ "$iterations" -gt 1 ] && iteration_args="-test-iterations $iterations -test-repetition-relaunch-enabled YES"

trap 'on_signal INT 130' INT
trap 'on_signal TERM 143' TERM

expected_runs=$((tests * iterations))
echo "# in a process group of its own, output also in $(quote "$xcodebuild_log"), with a ${timeout}s"
echo "# watchdog that then sends TERM, and KILL ${grace}s later, to it, the runner and the app"
# shellcheck disable=SC2086 # the two option lists split into words on purpose
show env TEST_RUNNER_E2E_APP="$app" TEST_RUNNER_E2E_SCRATCH="$scratch" \
	TEST_RUNNER_E2E_POOL="$pool" \
	xcodebuild test -project "$project" -scheme TenmokuE2E -destination platform=macOS \
	-derivedDataPath "$derived" -resultBundlePath "$xcresult" \
	-test-timeouts-enabled YES -default-test-execution-time-allowance 120 \
	-maximum-test-execution-time-allowance 300 $iteration_args $only_args
test_status=0
if [ -z "$dry" ]; then
	# shellcheck disable=SC2086
	run_watched env TEST_RUNNER_E2E_APP="$app" TEST_RUNNER_E2E_SCRATCH="$scratch" \
		TEST_RUNNER_E2E_POOL="$pool" \
		"$xcodebuild" test -project "$project" -scheme TenmokuE2E -destination platform=macOS \
		-derivedDataPath "$derived" -resultBundlePath "$xcresult" \
		-test-timeouts-enabled YES -default-test-execution-time-allowance 120 \
		-maximum-test-execution-time-allowance 300 $iteration_args $only_args ||
		test_status=$?
fi

# Each step is checked on its own, so a failure here still ends in the result locations
# and a failing final line, and never hides the tests' own status.
artifacts_ok=1
missing=""
note_missing() {
	artifacts_ok=""
	missing="$missing${missing:+, }$1"
}
run xcrun xcresulttool export attachments --path "$xcresult" --output-path "$attachments" ||
	note_missing "attachments"
show_to "$results/tests.json" xcrun xcresulttool get test-results tests --path "$xcresult"
if [ -z "$dry" ]; then
	xcrun xcresulttool get test-results tests --path "$xcresult" >"$results/tests.json" ||
		note_missing "tests.json"
fi
show_to "$results/summary.json" xcrun xcresulttool get test-results summary --path "$xcresult"
if [ -z "$dry" ]; then
	xcrun xcresulttool get test-results summary --path "$xcresult" >"$results/summary.json" ||
		note_missing "summary.json"
fi
echo "+ copy each named text attachment in $(quote "$attachments/manifest.json") to $(quote "$results")/<name>.txt"
if [ -z "$dry" ]; then
	copy_named_attachments || note_missing "named attachments"
fi

# A pass must have run every test the options asked for, as often as they asked: the
# summary counts tests, and the log counts each run of one.
echo "# check: summary.json has $tests tests, all passed; $(quote "$xcodebuild_log") has $expected_runs passing test runs"
count_problem=""
if [ -z "$dry" ]; then
	finished=$(grep -cE "^Test Case '-\[TenmokuE2E\.SmokeTests [A-Za-z0-9_]+\]' (passed|failed) " "$xcodebuild_log" || true)
	passed=$(grep -cE "^Test Case '-\[TenmokuE2E\.SmokeTests [A-Za-z0-9_]+\]' passed " "$xcodebuild_log" || true)
	total=$(plutil -extract totalTestCount raw -o - "$results/summary.json" 2>/dev/null || echo "?")
	summary_passed=$(plutil -extract passedTests raw -o - "$results/summary.json" 2>/dev/null || echo "?")
	echo "test runs: $passed passed of $finished finished, $expected_runs expected; summary: $summary_passed passed of $total tests"
	if [ "$passed" != "$expected_runs" ] || [ "$finished" != "$expected_runs" ] ||
		[ "$total" != "$tests" ] || [ "$summary_passed" != "$tests" ]; then
		count_problem="expected $expected_runs passing test runs of $tests tests; the log shows $passed of $finished and the summary $summary_passed of $total"
	fi
	for record in "$results"/launched-*.txt; do
		[ -f "$record" ] || continue
		echo "launched ($(basename "$record")): $(head -n 1 "$record")"
		break
	done
fi
echo "result bundle: $xcresult"
echo "attachments: $attachments"
echo "AX dumps and launch records: $results"
if [ -n "$dry" ]; then
	echo "e2e-macos: dry run, nothing ran"
elif [ "$test_status" -eq 124 ]; then
	echo "e2e-macos: FAILED (timed out after ${timeout}s)"
	exit 124
elif [ "$test_status" -ne 0 ]; then
	echo "e2e-macos: FAILED (xcodebuild test exited $test_status)"
	exit "$test_status"
elif [ -z "$artifacts_ok" ]; then
	echo "e2e-macos: FAILED (xcodebuild exited 0, but these could not be exported: $missing)"
	exit 1
elif [ -n "$count_problem" ]; then
	echo "e2e-macos: FAILED ($count_problem)"
	exit 1
else
	echo "e2e-macos: passed"
fi
