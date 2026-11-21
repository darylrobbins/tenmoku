---
task_uid: "01m7th2zchfa4cwhags66ttq44"
task: "T1997"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`--version` accepts `X.Y.Z-preview.N` and requires the bundle's CFBundleVersion to equal the value CH4 chose for it; the item's sparkle:shortVersionString is the display version; an `X.Y.Z` stays as today."
    required: true
    manual: false
  - id: "C2"
    text: "New `--channel-feed preview` flag: the output is the preview feed, one item, the build given (preview or stable), with no sparkle:channel element (the feed itself is the channel). The default output is unchanged byte for byte (a test compares against the current fixture)."
    required: true
    manual: false
  - id: "C3"
    text: "scripts/test-appcast.sh cases: preview item validates with xmllint; stable item unchanged; a preview version with --channel-feed absent is still accepted for the stable path only if the caller asked for it (refuse a prerelease in the stable feed: `a preview must go in the preview feed`)."
    required: true
    manual: false
---

# make-appcast.sh writes the preview appcast: a prerelease item and a merged newest-overall feed (channels 5)

# Preview appcast

Sparkle needs a feed URL per channel (see CH1 design). The stable appcast stays one item at releases/latest/download/appcast.xml. This script gains the ability to write the preview feed, which CH10 uploads to the rolling `preview` release. Read scripts/make-appcast.sh and scripts/test-appcast.sh first. The refusal rule matters: GitHub's latest/download never lists a prerelease, so a preview in the stable appcast would be an upload bug.


Read the files named before starting. Do not widen --scope; if a file outside it is needed, stop and respec. New tests go in new files.
