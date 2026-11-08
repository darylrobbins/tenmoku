#!/usr/bin/env fish
# A tour of fish syntax.

set -g greeting "Hello, \"fish\"\n"
set -l count 42
set -l ratio 3.14
set -gx PATH $HOME/bin $PATH
set -l names alice bob 'carol\'s'

function greet --description "Say hello to someone"
    argparse 'h/help' 'n/name=' -- $argv
    or return 1

    if set -q _flag_help
        echo "usage: greet [-n NAME]"
        return 0
    else if test -z "$_flag_name"
        echo $greeting
    else
        printf "Hello, %s!\n" $_flag_name
    end
end

function count_files -a dir
    set -l total (count $dir/*)
    echo "found $total files in $dir"
    return $total
end

for name in $names
    if test $name = alice -o $name = bob
        echo "known: $name"
    else if not test -d $name
        echo "unknown: $name"
    end
end

set -l i 0
while test $i -lt 5
    set i (math $i + 1)
    if test (math "$i % 2") -eq 0
        continue
    end
    echo "odd: $i"
    if [ $i -ge 4 ]
        break
    end
end

switch (uname)
    case Linux
        echo "penguin"
    case Darwin '*BSD'
        echo "unix"
    case '*'
        echo "other"
end

begin
    set -l tmp (mktemp)
    echo {a,b,c}.txt > $tmp
    cat $tmp | grep -v b &| wc -l
    rm $tmp
end

echo $names[1] $names[-1] $names[1..2]
ls -la 2>| less
command -v git >/dev/null; and echo "git found"
or echo "git missing"
