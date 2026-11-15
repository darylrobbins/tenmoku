//! The configurations a project's files imply: see the module's docs above.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kiln_l10n::{tr, verbatim};
use serde_json::Map;

use super::{AdapterRef, Cwd, LaunchConfig, PreLaunch, Program, Project, Request, Source};

/// Cargo's binaries, then the active Python file's.
pub(super) fn configs(project: &Project<'_>) -> Vec<LaunchConfig> {
    let mut configs: Vec<LaunchConfig> = cargo_bins(project)
        .into_iter()
        .map(|bin| {
            let build = [
                "cargo",
                "build",
                "--bin",
                &bin,
                "--message-format=json-render-diagnostics",
            ];
            LaunchConfig {
                pre_launch: Some(PreLaunch::CargoBuild(
                    build.iter().map(|&word| word.to_owned()).collect(),
                )),
                program: Some(Program::CargoArtifact { bin: bin.clone() }),
                ..detected(
                    tr!("workbench-debug-launch-detected-bin", bin = bin.as_str()).into(),
                    AdapterRef::DefaultFor("rust".to_owned()),
                    project.root,
                )
            }
        })
        .collect();
    if project.active_language == Some("python") {
        let adapter = "debugpy";
        configs.push(LaunchConfig {
            program: Some(Program::Written("${file}".to_owned())),
            ..detected(
                tr!("workbench-debug-launch-current-file").into(),
                AdapterRef::Named {
                    written: adapter.to_owned(),
                    name: project.adapters.name_for(adapter),
                },
                project.root,
            )
        });
    }
    configs
}

fn detected(name: String, adapter: AdapterRef, root: &Path) -> LaunchConfig {
    LaunchConfig {
        name,
        source: Source::Detected,
        adapter,
        request: Request::Launch,
        program: None,
        args: Vec::new(),
        cwd: Cwd::At(root.to_path_buf()),
        env: BTreeMap::new(),
        stop_on_entry: false,
        pre_launch: None,
        console: None,
        adapter_args: Map::new(),
        blockers: Vec::new(),
    }
}

/// The root package's binary targets, as cargo finds them: the package's
/// own `src/main.rs`, then each `[[bin]]` in the file's order, then those
/// under `src/bin/` (`<name>.rs`, `<name>/main.rs`) by name. A `[[bin]]`
/// names a found file's binary itself. Whether files are looked for is
/// `autobins`, else cargo's default: yes, except in the 2015 edition (the
/// default edition) when there is a `[[bin]]`. Nothing for a manifest that
/// doesn't parse or has no `[package]`.
fn cargo_bins(project: &Project<'_>) -> Vec<String> {
    let root = project.root;
    let Ok(text) = project
        .files
        .read_to_string(&root.join(verbatim!("Cargo.toml")))
    else {
        return Vec::new();
    };
    let Ok(manifest) = toml::from_str::<toml::Table>(&text) else {
        return Vec::new();
    };
    let Some(package) = manifest.get("package").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    let Some(package_name) = package.get("name").and_then(toml::Value::as_str) else {
        return Vec::new();
    };
    let autobins = match package.get("autobins").and_then(toml::Value::as_bool) {
        Some(autobins) => autobins,
        None if !manifest.contains_key("bin") => true,
        None => match edition(project, &manifest, package) {
            Some(edition) => edition != "2015",
            // Cargo can't build a package whose edition it can't find.
            None => return Vec::new(),
        },
    };

    let mut explicit: Vec<(String, Option<PathBuf>)> = Vec::new();
    for bin in (manifest.get("bin").and_then(toml::Value::as_array))
        .into_iter()
        .flatten()
    {
        if let Some(name) = bin.get("name").and_then(toml::Value::as_str)
            && !name.is_empty()
            && !explicit.iter().any(|(taken, _)| taken == name)
        {
            let path = bin.get("path").and_then(toml::Value::as_str);
            explicit.push((name.to_owned(), path.map(|path| normal(root, path))));
        }
    }
    let named = |name: &str| explicit.iter().any(|(taken, _)| taken == name);
    let claimed = |path: &Path| {
        explicit
            .iter()
            .any(|(_, taken)| taken.as_deref() == Some(path))
    };

    let mut bins = Vec::new();
    let main = root.join("src").join("main.rs");
    if autobins && project.files.exists(&main) && !named(package_name) && !claimed(&main) {
        bins.push(package_name.to_owned());
    }
    bins.extend(explicit.iter().map(|(name, _)| name.clone()));
    if autobins {
        let mut found: Vec<(String, PathBuf)> = project
            .files
            .list_dir(&root.join("src").join("bin"))
            .into_iter()
            .filter_map(|path| {
                // Cargo passes over hidden entries.
                if path.file_name()?.to_str()?.starts_with('.') {
                    return None;
                }
                if project.files.exists(&path) {
                    let is_rust = path.extension().is_some_and(|ext| ext == "rs");
                    let stem = path.file_stem()?.to_str()?.to_owned();
                    is_rust.then_some((stem, path))
                } else {
                    let main = path.join("main.rs");
                    let name = path.file_name()?.to_str()?.to_owned();
                    project.files.exists(&main).then_some((name, main))
                }
            })
            .collect();
        found.sort();
        for (name, path) in found {
            if !named(&name) && !claimed(&path) && !bins.contains(&name) {
                bins.push(name);
            }
        }
    }
    bins
}

/// A manifest's `path` under the root, `./` and all, as cargo reads it.
fn normal(root: &Path, path: &str) -> PathBuf {
    let mut full = root.to_path_buf();
    for part in Path::new(path).components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                full.pop();
            }
            other => full.push(other),
        }
    }
    full
}

/// The package's edition: `2015` when it gives none, or its workspace's
/// `[workspace.package]` one when it inherits it. The workspace is the
/// root's own, the one `package.workspace` points to, or else the first
/// manifest above the root with a `[workspace]`, as cargo looks for it
/// (membership isn't checked). `None` when there's no such edition to
/// inherit, which cargo refuses.
fn edition(project: &Project<'_>, manifest: &toml::Table, package: &toml::Table) -> Option<String> {
    match package.get("edition") {
        None => return Some("2015".to_owned()),
        Some(toml::Value::String(edition)) => return Some(edition.clone()),
        Some(_) => {}
    }
    let workspace_edition = |manifest: &toml::Table| {
        manifest
            .get("workspace")?
            .get("package")?
            .get("edition")?
            .as_str()
            .map(str::to_owned)
    };
    if manifest.contains_key("workspace") {
        return workspace_edition(manifest);
    }
    let read = |dir: &Path| {
        let text = (project.files)
            .read_to_string(&dir.join(verbatim!("Cargo.toml")))
            .ok()?;
        toml::from_str::<toml::Table>(&text).ok()
    };
    if let Some(path) = package.get("workspace").and_then(toml::Value::as_str) {
        return workspace_edition(&read(&normal(project.root, path))?);
    }
    project
        .root
        .ancestors()
        .skip(1)
        .filter_map(read)
        .find(|manifest| manifest.contains_key("workspace"))
        .and_then(|manifest| workspace_edition(&manifest))
}
