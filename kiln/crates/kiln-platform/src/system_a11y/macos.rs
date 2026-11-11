// SPDX-License-Identifier: Apache-2.0

//! The macOS reader: `NSWorkspace.sharedWorkspace`'s accessibility display
//! options, and a block observer on its own notification centre (not the
//! default one) for `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`.

use std::ptr::NonNull;

use block2::RcBlock;
use gpui::App;
use objc2::{class, msg_send, rc::Retained, rc::autoreleasepool, runtime::AnyObject};

use super::{SystemA11y, set_system_a11y};

/// The workspace's options now.
pub(super) fn read() -> SystemA11y {
    autoreleasepool(|_| {
        // SAFETY: `sharedWorkspace` is an `NSWorkspace`, which answers these
        // BOOL getters on any thread; each is a plain read.
        unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let Some(workspace) = workspace.as_ref() else {
                return SystemA11y::default();
            };
            SystemA11y {
                reduce_motion: msg_send![workspace, accessibilityDisplayShouldReduceMotion],
                increase_contrast: msg_send![workspace, accessibilityDisplayShouldIncreaseContrast],
                reduce_transparency: msg_send![
                    workspace,
                    accessibilityDisplayShouldReduceTransparency
                ],
                differentiate_without_color: msg_send![
                    workspace,
                    accessibilityDisplayShouldDifferentiateWithoutColor
                ],
            }
        }
    })
}

/// Reads again on every change notification, on the main thread, and updates
/// the global from the foreground executor, never inside the notification's
/// own call.
pub(super) fn watch(cx: &mut App) {
    let app = cx.to_async();
    let block = RcBlock::new(move |_notification: NonNull<AnyObject>| {
        let app = app.clone();
        let executor = app.foreground_executor().clone();
        executor
            .spawn(async move { app.update(|cx| set_system_a11y(cx, read())) })
            .detach();
    });
    autoreleasepool(|_| {
        // SAFETY: the workspace, its centre and the name are live objects on
        // the main thread; `addObserverForName:object:queue:usingBlock:`
        // copies the block, and the returned token is kept for the process's
        // life (leaked on purpose: the observer is never removed).
        unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let Some(workspace) = workspace.as_ref() else {
                return;
            };
            let center: *mut AnyObject = msg_send![workspace, notificationCenter];
            let Some(center) = center.as_ref() else {
                return;
            };
            // The value of `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`.
            let name: *mut AnyObject = msg_send![
                class!(NSString),
                stringWithUTF8String: c"NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification"
                    .as_ptr()
            ];
            let token: Option<Retained<AnyObject>> = msg_send![
                center,
                addObserverForName: name,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ];
            std::mem::forget(token);
        }
    });
}
