//! Messages routed to toasts: a language server's errors and warnings, and
//! files and folders that can't open.

use std::{fs, os::unix::fs::PermissionsExt as _};

use gpui::TestAppContext;
use lsp_types::{MessageType, ShowMessageParams, notification::ShowMessage};

use super::Level;
use crate::tests::lsp::Lsp;

/// What each toast shown says, top first: level, message and detail.
fn shown(s: &mut Lsp) -> Vec<(Level, String, Option<String>)> {
    let toasts = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    toasts.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                (toast.level, toast.message.clone(), toast.detail.clone())
            })
            .collect()
    })
}

mod lsp {
    use super::*;

    #[gpui::test]
    fn errors_and_warnings_toast_and_info_and_log_dont(cx: &mut TestAppContext) {
        let mut s = Lsp::new(cx);
        s.open("src/main.rs");
        s.fake.wait_initialized();
        let say = |s: &Lsp, typ, message: &str| {
            s.fake.notify::<ShowMessage>(ShowMessageParams {
                typ,
                message: message.into(),
            })
        };
        // Info and log first: when the warning's toast shows, they would
        // have by now.
        say(&s, MessageType::INFO, "just so you know");
        say(&s, MessageType::LOG, "a logged line");
        say(&s, MessageType::WARNING, "careful");
        s.until("the warning's toast", |s| !shown(s).is_empty());
        say(&s, MessageType::ERROR, "it broke");
        s.until("the error's toast", |s| shown(s).len() == 2);
        assert_eq!(
            shown(&mut s),
            [
                (
                    Level::Warning,
                    "rust-analyzer".to_owned(),
                    Some("careful".to_owned())
                ),
                (
                    Level::Error,
                    "rust-analyzer".to_owned(),
                    Some("it broke".to_owned())
                ),
            ]
        );
    }
}

mod open_failures {
    use super::*;

    /// Makes `path` unreadable, restoring it when dropped so the temporary
    /// folder can be removed.
    struct Locked(std::path::PathBuf, u32);

    impl Locked {
        fn new(path: std::path::PathBuf) -> Self {
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            Self(path, mode)
        }
    }

    impl Drop for Locked {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1)).ok();
        }
    }

    #[gpui::test]
    fn an_unreadable_file_and_folder_toast(cx: &mut TestAppContext) {
        let mut s = Lsp::new(cx);
        let file = s.path("src/other.rs");
        let _file = Locked::new(file.clone());
        let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
        s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(file, window, cx)));
        s.until("the file's toast", |s| !shown(s).is_empty());
        let toasts = shown(&mut s);
        assert_eq!(toasts.len(), 1, "{toasts:?}");
        let (level, message, detail) = &toasts[0];
        assert_eq!(
            (*level, message.as_str()),
            (Level::Error, "Can't open other.rs")
        );
        assert!(
            detail.as_deref().is_some_and(|d| !d.is_empty()),
            "{detail:?}"
        );

        let folder = s.path("locked");
        fs::create_dir(&folder).unwrap();
        let _folder = Locked::new(folder.clone());
        let workbench = s.workbench.clone();
        let root = folder.clone();
        s.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
        });
        let name = format!("Can't open folder {}", folder.display());
        s.until("the folder's toast", |s| {
            shown(s).iter().any(|(_, message, _)| *message == name)
        });
    }
}
