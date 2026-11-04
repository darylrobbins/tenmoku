//! The app binary's updater hook: a hook that takes the check skips GitHub,
//! one that declines falls back to it, and none is as before.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use futures::FutureExt as _;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    update_check::{Fetched, UpdateSource, UpdaterHook, set_updater_hook},
};

/// A window with its workbench focused, as the command runs from.
fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: None,
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let focus = workbench.read_with(&vcx, |workbench, cx| workbench.focus_handle(cx));
    vcx.update(|window, cx| window.focus(&focus, cx));
    vcx.run_until_parked();
    (workbench, vcx)
}

/// Has every GitHub check count itself in the returned counter, saying the
/// running version is the latest.
fn fake(cx: &mut VisualTestContext) -> Arc<AtomicUsize> {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let source = UpdateSource {
        fetch: Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let body = serde_json::json!({
                "tag_name": "v0.1.0",
                "html_url": "https://github.com/darylrobbins/fastedit/releases/tag/v0.1.0",
                "draft": false,
                "prerelease": false,
            });
            let body = body.to_string().into_bytes();
            futures::future::ready(Ok(Fetched { status: 200, body })).boxed()
        }),
        running: "0.1.0".to_owned(),
    };
    cx.update(|_, cx| cx.set_global(source));
    asked
}

/// Sets a hook that counts its calls and answers `takes`.
fn hook(takes: bool, cx: &mut VisualTestContext) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let hook = UpdaterHook(Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        takes
    }));
    cx.update(|_, cx| set_updater_hook(cx, hook));
    calls
}

/// Runs the command to its end.
fn check(cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(
        Invocation::of("app.check_for_updates")
            .unwrap()
            .context(stack),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

/// How many toasts are shown.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| layer.queue().shown().len())
}

#[gpui::test]
fn hook_taking_the_check_skips_github(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;
    let asked = fake(cx);
    let calls = hook(true, cx);
    check(cx);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(asked.load(Ordering::SeqCst), 0);
    assert_eq!(toasts(&workbench, cx), 0);
}

#[gpui::test]
fn hook_declining_falls_back_to_github(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;
    let asked = fake(cx);
    let calls = hook(false, cx);
    check(cx);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(toasts(&workbench, cx), 1);
}

#[gpui::test]
fn no_hook_is_todays_behaviour(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;
    let asked = fake(cx);
    check(cx);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(toasts(&workbench, cx), 1);
}
