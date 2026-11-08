#!/usr/bin/awk -f
# Summarise a log of "name status bytes" lines per host.
@include "join"

function max(a, b) {
    return a > b ? a : b
}

function report(counts, total,    host, line) {
    for (host in counts) {
        line = sprintf("%-12s %5d  %5.1f%%", host, counts[host], 100 * counts[host] / total)
        print line | "sort -k2 -nr"
    }
    close("sort -k2 -nr")
}

BEGIN {
    FS = "[ \t]+"
    OFS = "\t"
    limit = 16 + 1e3
    print "start\tof \"report\"" > "/dev/stderr"
}

# Skip blank lines and comments.
/^[ \t]*(#|$)/ { next }

$2 == "ERR" || $2 ~ /^5[0-9][0-9]$/ {
    errors[$1]++
    worst = max(worst, $3)
    next
}

NR % 100 == 0 { printf "processed %d lines\n", NR }

{
    counts[$1] += 1
    bytes += $3
    if (length($1) > 20) {
        $1 = substr($1, 1, 20)
    } else if (tolower($1) == "localhost") {
        delete counts[$1]
        continue
    }
    while ((getline extra < "hosts.txt") > 0) {
        seen[extra] = 1
    }
}

END {
    n = 0
    do {
        n++
    } while (n < 3)
    gsub(/[aeiou]/, "-", worst)
    report(counts, NR)
    printf "%d errors, worst %s\n", length(errors), worst
    exit (NR > 0) ? 0 : 1
}
