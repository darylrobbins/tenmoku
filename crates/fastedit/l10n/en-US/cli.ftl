### The command line (src/cli.rs): `--help` and the unknown-flag error.
### Flag names and the `tenmoku:` prefix stay as they are; only the system
### locale chooses the language, and output carries no isolation marks.

# `--help`'s text, without its last newline.
fastedit-cli-usage =
    Tenmoku, a code editor.

    Usage: tenmoku [options] [--] [path[:line[:column]] ...]

    Each path opens as a tab, in order, the last one active; a path that doesn't
    exist is a new file, which saving creates. The first folder is the project
    root, or with no folder, the first file's folder. Lines and columns count
    from 1. A file really named a:1 opens as itself.

    Options:
          --new-window  Open in a new window on the path's folder
          --wait        Wait until the files' tabs have closed, then exit
          --perf        Measure startup and scrolling on the file, print, and quit
      -h, --help        Print this help
      -V, --version     Print the version
          --            Treat everything after it as a path

# An unknown flag. $flag is the flag as typed.
fastedit-cli-unknown-flag = unknown flag { $flag }
