// SPDX-License-Identifier: Apache-2.0

//! The tier model: interactive limits as they were, and the background tier's limits, their
//! manifest table, the host's ceilings and what counts as an escalation.

use std::{fs, time::Duration};

use kiln_command::{Capability, Grants};
use kiln_core::{ApiVersion, ExtensionApi, load_manifest};
use kiln_plugin::{
    grants::{Decision, GrantRecord, evaluate_background},
    limits::{
        BACKGROUND_MEMORY_BYTES, BACKGROUND_WALL, BackgroundLimit, BackgroundLimits, CALL_DEADLINE,
        Ceilings, EVENT_QUEUE, Limits, MAX_READ_BYTES, MEMORY_BYTES,
    },
    manifest::{ExtensionOrigin, PluginManifest, PluginManifestError, plugin_kind},
};

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

fn manifest(background: &str) -> Result<PluginManifest, String> {
    let dir = std::env::temp_dir().join(format!(
        "kiln-plugin-tiers-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    let text = format!(
        "id = \"acme.tool\"\nname = \"Tool\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\nnotes = \"1.0\"\n\n[contributes.plugin]\nwasm = \"p.wasm\"\n{background}"
    );
    fs::write(dir.join("extension.toml"), text).unwrap();
    fs::write(dir.join("p.wasm"), b"\0asm").unwrap();
    let api = ExtensionApi {
        app: "notes".into(),
        version: ApiVersion { major: 1, minor: 0 },
        reserved: vec![],
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (loaded, diagnostics) = load_manifest(&dir, &api);
    let loaded = loaded.ok_or_else(|| format!("{diagnostics:?}"))?;
    let parsed = PluginManifest::from_extension(&loaded, ExtensionOrigin::User)
        .map_err(|error: PluginManifestError| error.to_string())?;
    fs::remove_dir_all(&dir).ok();
    Ok(parsed.unwrap())
}

#[test]
fn tiers_interactive_limits_are_unchanged() {
    let limits = Limits::default();
    assert_eq!(limits.call_deadline, Duration::from_millis(100));
    assert_eq!(limits.call_deadline, CALL_DEADLINE);
    assert_eq!(limits.memory_bytes, 64 << 20);
    assert_eq!(limits.memory_bytes, MEMORY_BYTES);
    assert_eq!(limits.event_queue, 256);
    assert_eq!(limits.event_queue, EVENT_QUEUE);
    assert_eq!(limits.max_read_bytes, 16 << 20);
    assert_eq!(limits.max_read_bytes, MAX_READ_BYTES);
    // And the background defaults are the design's.
    let background = BackgroundLimits::default();
    assert_eq!(background.wall, Duration::from_secs(600));
    assert_eq!(background.wall, BACKGROUND_WALL);
    assert_eq!(background.memory_bytes, 256 * MIB);
    assert_eq!(background.memory_bytes, BACKGROUND_MEMORY_BYTES);
    assert_eq!(background.data_bytes, None);
}

#[test]
fn tiers_background_parses_units() {
    assert_eq!(
        manifest("").unwrap().background,
        BackgroundLimits::default()
    );
    let parsed = manifest(
        "[contributes.plugin.background]\nmemory = \"1GiB\"\nwall = \"30m\"\ndata = \"256MiB\"\n",
    )
    .unwrap()
    .background;
    assert_eq!(parsed.memory_bytes, GIB);
    assert_eq!(parsed.wall, Duration::from_secs(30 * 60));
    assert_eq!(parsed.data_bytes, Some(256 * MIB));
    let small = manifest("[contributes.plugin.background]\nmemory = \"512KiB\"\n");
    assert!(small.is_err(), "below 1 MiB");
    let small = manifest("[contributes.plugin.background]\nmemory = \"2048KiB\"\nwall = \"45s\"\n")
        .unwrap()
        .background;
    assert_eq!(small.memory_bytes, 2 * MIB);
    assert_eq!(small.wall, Duration::from_secs(45));
    for bad in [
        "wall = \"0s\"",
        "wall = \"30\"",
        "wall = \"30h\"",
        "wall = 30",
        "memory = \"big\"",
        "memory = \"-1MiB\"",
        "data = \"3GiB\"",
        "colour = \"red\"",
    ] {
        let text = format!("[contributes.plugin.background]\n{bad}\n");
        assert!(manifest(&text).is_err(), "{bad} is refused");
    }
}

#[test]
fn tiers_background_over_ceiling_is_clamped() {
    // 8 GiB of RAM: 2 GiB a job.
    let ceilings = Ceilings::for_ram(8 * GIB);
    let parsed =
        manifest("[contributes.plugin.background]\nmemory = \"4GiB\"\nwall = \"60m\"\n").unwrap();
    let (limits, warnings) = parsed.background_within(&ceilings);
    assert_eq!(limits.memory_bytes, 2 * GIB);
    assert_eq!(limits.wall, Duration::from_secs(3600));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("acme.tool"), "{warnings:?}");

    // A small machine clamps a value that was fine on a big one.
    let (limits, warnings) = parsed.background_within(&Ceilings::for_ram(2 * GIB));
    assert_eq!(limits.memory_bytes, 512 * MIB);
    assert_eq!(warnings.len(), 1);

    // Within the ceiling: untouched, no warning.
    let (limits, warnings) = manifest("").unwrap().background_within(&ceilings);
    assert_eq!(limits, BackgroundLimits::default());
    assert!(warnings.is_empty());

    // Wall time past 60 minutes is clamped, whatever the RAM.
    let long = BackgroundLimits {
        wall: Duration::from_secs(2 * 3600),
        ..BackgroundLimits::default()
    };
    let (limits, clamped) = ceilings.clamp(long);
    assert_eq!(limits.wall, Duration::from_secs(3600));
    assert_eq!(clamped.len(), 1);
    assert_eq!(clamped[0].limit, BackgroundLimit::Wall);
}

#[test]
fn tiers_ceiling_follows_physical_ram() {
    let cases = [
        // (RAM, per job, all jobs)
        (GIB, GIB / 4, GIB / 2),
        (4 * GIB, GIB, 2 * GIB),
        (8 * GIB, 2 * GIB, 4 * GIB),
        (16 * GIB, 2 * GIB, 4 * GIB),
        (128 * GIB, 2 * GIB, 4 * GIB),
    ];
    for (ram, job, all) in cases {
        let ceilings = Ceilings::for_ram(ram);
        assert_eq!(ceilings.job_memory_bytes, job, "{ram} per job");
        assert_eq!(ceilings.all_jobs_memory_bytes, all, "{ram} all jobs");
        assert_eq!(ceilings.wall, Duration::from_secs(3600));
    }
    // The wall time never passes 60 minutes.
    assert!(Ceilings::host().wall <= Duration::from_secs(60 * 60));
}

#[test]
fn tiers_raising_background_limits_is_an_escalation() {
    let caps: Grants = [Capability::parse("buffer.read").unwrap()]
        .into_iter()
        .collect();
    let accepted = BackgroundLimits {
        memory_bytes: 512 * MIB,
        wall: Duration::from_secs(30 * 60),
        data_bytes: None,
    };
    let record = GrantRecord {
        version: Some((1, 0, 0)),
        granted: caps.clone(),
        background: Some(accepted),
        ..GrantRecord::default()
    };
    let same = evaluate_background(Some(&record), &caps, &accepted);
    assert_eq!(same, Decision::Load(caps.clone()));

    for raised in [
        BackgroundLimits {
            memory_bytes: GIB,
            ..accepted
        },
        BackgroundLimits {
            wall: Duration::from_secs(31 * 60),
            ..accepted
        },
    ] {
        match evaluate_background(Some(&record), &caps, &raised) {
            Decision::Escalation {
                added,
                keep_running,
            } => {
                assert!(added.iter().next().is_none());
                assert_eq!(keep_running, Some((1, 0, 0)));
            }
            other => panic!("{other:?}"),
        }
    }

    // Lowering, or the reserved data quota, never asks.
    let lowered = BackgroundLimits {
        memory_bytes: 64 * MIB,
        wall: Duration::from_secs(60),
        data_bytes: Some(GIB),
    };
    assert_eq!(
        evaluate_background(Some(&record), &caps, &lowered),
        Decision::Load(caps.clone())
    );

    // A record that accepted nothing accepts the defaults: asking for more asks.
    let plain = GrantRecord {
        background: None,
        ..record
    };
    let more = BackgroundLimits {
        memory_bytes: BACKGROUND_MEMORY_BYTES + 1,
        ..BackgroundLimits::default()
    };
    assert!(matches!(
        evaluate_background(Some(&plain), &caps, &more),
        Decision::Escalation { .. }
    ));
    assert_eq!(
        evaluate_background(Some(&plain), &caps, &BackgroundLimits::default()),
        Decision::Load(caps)
    );
}

#[test]
fn tiers_a_decline_covers_the_limits_asked_with_it() {
    let caps: Grants = [Capability::parse("buffer.read").unwrap()]
        .into_iter()
        .collect();
    let wider: Grants = [
        Capability::parse("buffer.read").unwrap(),
        Capability::parse("commands").unwrap(),
    ]
    .into_iter()
    .collect();
    let big = BackgroundLimits {
        memory_bytes: GIB,
        ..BackgroundLimits::default()
    };
    let medium = BackgroundLimits {
        memory_bytes: 512 * MIB,
        ..BackgroundLimits::default()
    };
    // Installed with the defaults; a raise to 1 GiB declined.
    let record = GrantRecord {
        version: Some((1, 0, 0)),
        granted: caps.clone(),
        declined: Some(caps.clone()),
        declined_background: Some(big),
        ..GrantRecord::default()
    };
    assert_eq!(
        evaluate_background(Some(&record), &caps, &big),
        Decision::Declined
    );
    // Another raise is a new request.
    assert!(matches!(
        evaluate_background(Some(&record), &caps, &medium),
        Decision::Escalation { .. }
    ));
    // As is the same capabilities with the limits back at the defaults.
    assert_eq!(
        evaluate_background(Some(&record), &caps, &BackgroundLimits::default()),
        Decision::Load(caps.clone())
    );
    // A capability escalation declined with the defaults is asked again if limits rise too.
    let record = GrantRecord {
        declined: Some(wider.clone()),
        declined_background: None,
        ..record
    };
    assert_eq!(
        evaluate_background(Some(&record), &wider, &BackgroundLimits::default()),
        Decision::Declined
    );
    assert!(matches!(
        evaluate_background(Some(&record), &wider, &medium),
        Decision::Escalation { .. }
    ));
    // An installation never granted anything: the same.
    let never = GrantRecord {
        declined: Some(caps.clone()),
        declined_background: Some(big),
        ..GrantRecord::default()
    };
    assert_eq!(
        evaluate_background(Some(&never), &caps, &big),
        Decision::Declined
    );
    assert!(matches!(
        evaluate_background(Some(&never), &caps, &medium),
        Decision::Prompt(_)
    ));
}
