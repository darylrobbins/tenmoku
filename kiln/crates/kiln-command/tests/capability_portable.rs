// SPDX-License-Identifier: Apache-2.0

//! `process.spawn:` names are pure strings, so these rules hold on every OS.

use kiln_command::Capability;

#[test]
fn capability_portable_spawn_names_refuse_separators() {
    for bad in [
        "process.spawn:tools/rg",
        "process.spawn:tools\\rg",
        "process.spawn:C:rg",
    ] {
        assert!(bad.parse::<Capability>().is_err(), "{bad} parsed");
    }
    assert!("process.spawn:rg".parse::<Capability>().is_ok());
}

#[test]
fn capability_portable_pathext_name_is_a_warning() {
    let exe: Capability = "process.spawn:rg.exe".parse().unwrap();
    assert_eq!(
        exe.warning().as_deref(),
        Some("write `rg`; the extension is found on Windows")
    );
    for quiet in [
        "process.spawn:rg.sh",
        "process.spawn:rg",
        "process.spawn:*",
        "net:example.com",
    ] {
        let capability: Capability = quiet.parse().unwrap();
        assert_eq!(capability.warning(), None, "{quiet}");
    }
}
