## public
## Generic text other crates may use.

probe-cancel = Cancel

## Probe-only messages.

probe-title = Probe Labels
probe-only-english = Only in English
probe-files = { $count ->
    [one] One file
   *[other] { $count } files
}
probe-command = Run Probe
    .description = Runs the probe
