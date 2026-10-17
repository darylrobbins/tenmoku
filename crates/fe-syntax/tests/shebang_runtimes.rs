#![cfg(all(feature = "typescript", feature = "bash"))]

use std::path::Path;

use fe_syntax::{detect_language_name, language_name_for_shebang};

#[test]
fn typescript_runtimes_and_posix_shells_pick_their_language() {
    for (line, language) in [
        ("#!/usr/bin/env deno\n", Some("typescript")),
        (
            "#!/usr/bin/env -S deno run --allow-read\n",
            Some("typescript"),
        ),
        ("#!/usr/bin/env bun\n", Some("typescript")),
        ("#!/usr/local/bin/bun\n", Some("typescript")),
        ("#!/usr/bin/env ts-node\n", Some("typescript")),
        ("#!/bin/ash\n", Some("bash")),
        ("#!/bin/mksh\n", Some("bash")),
        ("#!/usr/bin/yash\n", Some("bash")),
        ("#!/bin/pdksh\n", Some("bash")),
        ("#!/bin/dtksh\n", Some("bash")),
        ("#!/usr/bin/env bunx\n", None),
        ("#!/usr/bin/env deno-x\n", None),
        ("#!/bin/flash\n", None),
        ("#!/bin/tcsh\n", None),
        ("#!/usr/bin/env fish\n", None),
    ] {
        assert_eq!(language_name_for_shebang(line), language, "{line}");
    }
    assert_eq!(
        detect_language_name(Some(Path::new("bin/tool")), "#!/usr/bin/env deno\n"),
        Some("typescript")
    );
}
