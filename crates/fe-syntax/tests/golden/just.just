#!/usr/bin/env -S just --justfile
# Settings, variables and an alias.
set shell := ["bash", "-c"]
set dotenv-load

version := "1.0"
export root := justfile_directory() / "src"
today := `date +%F`
mode := if os() == "macos" { "fast" } else { "slow" }

alias b := build

# A recipe with parameters and dependencies.
[group('dev')]
build target="debug" *args: clean (lint target)
    echo "building {{target}} at {{version}}" $HOME
    @echo done
    cargo build {{args}}

clean:
    rm -rf target # not a just comment

lint target:
    cargo clippy --profile {{target}}

python:
    #!/usr/bin/env python3
    print("a shebang's language")
