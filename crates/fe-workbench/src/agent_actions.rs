//! Asking the chat agent about something from anywhere: [`Workbench::ask_agent`]
//! opens the chat with some context attached and a prompt, and sends it
//! only if the chat can. A button elsewhere (Problems, Runs, Terminal) builds
//! an [`AgentAsk`], with [`text_item`] and [`rows_item`] for its items.

use std::path::Path;

use fe_chat::{ContextItem, ContextKind, Rows};
use gpui::{Context, Focusable as _, Window};
use kiln_l10n::tr;

use super::{
    ChatPanel,
    chat_context::{self, MAX_FILE},
};
use crate::{
    Workbench,
    workbench::toast::{self, Toast},
};

/// What to ask the agent.
#[derive(Clone, Debug)]
pub(crate) struct AgentAsk {
    /// The composer's text.
    pub prompt: String,
    /// What the chat attaches, as the user's own attachments are. Every
    /// item that names a file is vetted as a file: its path and where it
    /// really is, and its size.
    pub context: Vec<ContextItem>,
    /// Blocks of text made by [`text_item`] (output, a message): not
    /// files, so the deny list doesn't apply, though the size limit does.
    pub output: Vec<ContextItem>,
    /// Whether to send it at once, if the chat can.
    pub send: bool,
}

/// A block of text (a command's output, a message) as an item named
/// `label`, for [`AgentAsk::output`], cut to the chat's per-item limit at a character boundary.
pub(crate) fn text_item(label: &str, text: &str) -> ContextItem {
    let mut end = text.len().min(MAX_FILE);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    ContextItem {
        kind: ContextKind::File {
            path: label.to_owned(),
            rows: None,
        },
        label: label.to_owned(),
        text: text[..end].to_owned(),
    }
}

/// `rows` of the file at `path` (as the chat shows paths), with their
/// `text`.
pub(crate) fn rows_item(path: &str, rows: Rows, text: &str) -> ContextItem {
    ContextItem {
        kind: ContextKind::File {
            path: path.to_owned(),
            rows: Some(rows),
        },
        label: format!("{path}:{rows}"),
        text: text.to_owned(),
    }
}

/// The file an item names, if it names one: every file, whole or its rows.
fn file_of(item: &ContextItem) -> Option<&str> {
    match &item.kind {
        ContextKind::File { path, .. }
        | ContextKind::Selection { path, .. }
        | ContextKind::ActiveFile { path, .. } => Some(path),
        ContextKind::Diagnostics | ContextKind::OpenEditors => None,
    }
}

/// Why the file `path` (as the chat shows it) can't be attached under
/// `root`: the deny list on its path and on where it really is, which a
/// link can't get past, and the size and binary limits on `text`. A file
/// that isn't there yet (an unsaved buffer's) is fine.
fn refusal(root: Option<&Path>, path: &str, text: &str) -> Option<String> {
    let at = root.map_or_else(|| path.into(), |root| root.join(path));
    if let Some(reason) = chat_context::refusal(root, &at) {
        return Some(reason);
    }
    let root = root?;
    if let Err(reason) = chat_context::open_vetted(root, &at) {
        return Some(reason);
    }
    chat_context::unattachable(text.as_bytes(), text.len())
}

impl ChatPanel {
    /// Sends the composer's `prompt` if the chat has a key and the folder
    /// allows chat, and the composer still holds it; else leaves the draft
    /// and toasts why.
    pub(super) fn finish_ask(
        &mut self,
        prompt: &str,
        trusted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.view.clone();
        if view.read(cx).composer_text(cx) != prompt {
            return;
        }
        if self.current.is_none() {
            toast::show(
                cx,
                Toast::info(tr!("workbench-chat-ask-no-key").to_string()),
            );
        } else if !trusted {
            toast::show(
                cx,
                Toast::info(tr!("workbench-chat-ask-untrusted").to_string()),
            );
        } else {
            view.update(cx, |view, cx| {
                kiln::with_cx(window, cx, |cx| view.send_message(cx))
            });
        }
    }
}

impl Workbench {
    /// Shows the Chat panel with `ask`'s items attached (a file's rows
    /// the deny list, the size limit or a binary file refuse are left off,
    /// a toast saying so) and its prompt in the composer, focused. Sends
    /// only if `ask.send` and the chat has a key (waiting for the
    /// keychain's) and the folder allows chat; else the draft stays, a
    /// toast saying why.
    pub(crate) fn ask_agent(&mut self, ask: AgentAsk, window: &mut Window, cx: &mut Context<Self>) {
        self.open_chat(window, cx);
        let Some(panel) = self.dock().read(cx).chat().cloned() else {
            return;
        };
        let root = self.root().map(Path::to_owned);
        let trusted = root.is_none() || self.root_trusted();
        panel.update(cx, |panel, cx| {
            // The key form, History and the picker stand in for the chat
            // while open; a listing under way would bring History back.
            panel.close_key_form(window, cx);
            (panel.history, panel.listing, panel.picker) = (None, None, false);
            panel.pending_send = None;
            // Reads the key if the chat never drew.
            if !panel.key_read {
                panel.reload_key(cx);
            }
            let view = panel.view.clone();
            let output = ask.output.into_iter().map(|item| (item, false));
            for (item, vetted) in ask
                .context
                .into_iter()
                .map(|item| (item, true))
                .chain(output)
            {
                if let Some(path) = file_of(&item).filter(|_| vetted)
                    && let Some(reason) = refusal(root.as_deref(), path, &item.text)
                {
                    chat_context::refused(path, &reason, cx);
                    continue;
                }
                if !vetted
                    && let Some(reason) =
                        chat_context::unattachable(item.text.as_bytes(), item.text.len())
                {
                    chat_context::refused(&item.label, &reason, cx);
                    continue;
                }
                view.update(cx, |view, cx| view.attach(item, cx));
            }
            view.update(cx, |view, cx| view.set_composer_text(&ask.prompt, cx));
            let focus = panel.focus_handle(cx);
            window.focus(&focus, cx);
            if !ask.send {
                return;
            }
            if panel.reading.is_some() {
                panel.pending_send = Some((panel.views_made, ask.prompt));
            } else {
                panel.finish_ask(&ask.prompt, trusted, window, cx);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[path = "../tests/agent_actions.rs"]
    mod agent_actions;
}
