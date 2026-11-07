//! Installing an extension takes the reserved command namespaces when the
//! install dialog closes, not when it opens.

use std::{
    fs,
    io::Write as _,
    sync::{Mutex, MutexGuard},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::{Launch, Workbench};
use gpui::{Action as _, Focusable as _, TestAppContext};
use kiln_platform::command_actions::{CommandInfo, register_command};
use zip::{ZipWriter, write::SimpleFileOptions};

static SERIAL: Mutex<()> = Mutex::new(());

gpui::actions!(acme, [Tool]);

/// Runs the rest of a test with no other of this binary running, and the
/// state directory `state`.
fn serially(state: &std::path::Path) -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: every test takes the lock before it starts gpui.
    unsafe { std::env::set_var("TENMOKU_STATE_DIR", state) };
    for var in ["TENMOKU_CONFIG_DIR", "XDG_CONFIG_HOME"] {
        // SAFETY: as above.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

#[gpui::test]
fn a_namespace_registered_while_the_dialog_is_open_is_reserved(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let _serial = serially(state.path());
    let sources = tempfile::tempdir().unwrap();
    let zip_path = sources.path().join("acme.zip");
    let manifest = "id = \"acme.tool\"\nname = \"acme.tool\"\nversion = \"1.0.0\"\n\
                    license = \"MIT\"\n\n[engine]\ntenmoku = \"1.1\"\n";
    let mut zip = ZipWriter::new(fs::File::create(&zip_path).unwrap());
    zip.start_file("extension.toml", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(manifest.as_bytes()).unwrap();
    zip.finish().unwrap();

    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, vcx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    vcx.run_until_parked();

    let invocation = Invocation::of("workbench.install_extension")
        .unwrap()
        .context(ContextStack::new().with(ContextFrame::new("workbench")));
    vcx.update(|window, cx| window.dispatch_action(RunCommand(invocation).boxed_clone(), cx));
    vcx.run_until_parked();
    assert!(vcx.did_prompt_for_paths());

    // The namespace appears while the dialog is open.
    vcx.update(|_, cx| {
        register_command::<Tool>(
            cx,
            CommandInfo {
                title: "Tool".into(),
                context: None,
                unlisted: false,
            },
        )
    });
    vcx.simulate_path_prompt_response(move |_| Some(vec![zip_path]));
    vcx.run_until_parked();

    assert!(
        vcx.debug_bounds("toast-0").is_some(),
        "the refusal shows as a toast"
    );
    assert!(
        installed(state.path()).is_empty(),
        "nothing is installed: {:?}",
        installed(state.path())
    );
}

/// The `extension.toml` files under `dir`.
fn installed(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(installed(&path));
        } else if path
            .file_name()
            .is_some_and(|name| name == "extension.toml")
        {
            found.push(path);
        }
    }
    found
}
