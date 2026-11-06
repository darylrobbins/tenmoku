//! Check for Updates…, run as a command with a fake fetcher, so no network:
//! how each reply is toasted, what Download opens, and one check at a time.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use futures::{FutureExt as _, channel::oneshot};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use serde_json::json;

use crate::{
    Launch, Workbench,
    update_check::{
        ALREADY_CHECKING, DOWNLOAD, Fetched, MAX_REPLY, NO_RELEASE, UpdateSource, read_reply,
    },
    workbench::toast::{Choices, Level},
};

const PAGE: &str = "https://github.com/darylrobbins/tenmoku/releases/tag/v0.2.0";

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

/// Has every check from now on get `reply` at version `running`.
fn fake(running: &str, reply: Result<Fetched, String>, cx: &mut VisualTestContext) {
    let source = UpdateSource {
        fetch: Arc::new(move || futures::future::ready(reply.clone()).boxed()),
        running: running.to_owned(),
    };
    cx.update(|_, cx| cx.set_global(source));
}

/// A 200 with `release` as its body.
fn ok(release: serde_json::Value) -> Result<Fetched, String> {
    Ok(Fetched {
        status: 200,
        body: release.to_string().into_bytes(),
    })
}

/// A published release tagged `tag`, on its page.
fn release(tag: &str) -> serde_json::Value {
    json!({
        "tag_name": tag,
        "html_url": format!("https://github.com/darylrobbins/tenmoku/releases/tag/{tag}"),
        "draft": false,
        "prerelease": false,
    })
}

/// Runs the command as the palette or menu would, without waiting for it.
fn start(cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(
        Invocation::of("app.check_for_updates")
            .unwrap()
            .context(stack),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

/// Runs the command to its end.
fn check(cx: &mut VisualTestContext) {
    start(cx);
    cx.run_until_parked();
}

/// A toast, as far as these tests tell them apart.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    level: Level,
    message: String,
    detail: Option<String>,
    choices: Option<Choices>,
}

/// The toasts shown, leaving them there.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Shown> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| Shown {
                level: entry.toast.level,
                message: entry.toast.message.clone(),
                detail: entry.toast.detail.clone(),
                choices: entry.toast.choices.clone(),
            })
            .collect()
    })
}

/// Dismisses every toast, each a question's no.
fn dismiss(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    cx.update(|_, cx| layer.update(cx, |layer, cx| layer.dismiss_all(cx)));
    cx.run_until_parked();
}

/// The one toast shown, emptied.
fn only(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Shown {
    let mut toasts = shown(workbench, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    dismiss(workbench, cx);
    toasts.remove(0)
}

#[gpui::test]
fn newer_release_is_reported(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;

    // Going without an answer opens nothing.
    fake("0.1.0", ok(release("v0.2.0")), cx);
    check(cx);
    let offer = only(&workbench, cx);
    assert_eq!(offer.level, Level::Info);
    assert_eq!(offer.message, "Tenmoku 0.2.0 is available (you have 0.1.0)");
    assert_eq!(offer.choices.unwrap().titles(), [DOWNLOAD]);
    assert_eq!(cx.opened_url(), None);

    // Download opens the release's page.
    check(cx);
    let toasts = shown(&workbench, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    let choices = toasts[0].choices.as_ref().unwrap();
    assert_eq!(cx.opened_url(), None);
    choices.choose(0);
    cx.run_until_parked();
    assert_eq!(cx.opened_url().as_deref(), Some(PAGE));
    dismiss(&workbench, cx);

    // As numbers, not text, each part in turn.
    for (running, tag) in [
        ("0.9.0", "v0.10.0"),
        ("0.1.9", "v0.1.10"),
        ("0.99.99", "v1.0.0"),
    ] {
        fake(running, ok(release(tag)), cx);
        check(cx);
        let version = tag.strip_prefix('v').unwrap();
        assert_eq!(
            only(&workbench, cx).message,
            format!("Tenmoku {version} is available (you have {running})")
        );
    }
}

#[gpui::test]
fn same_or_older_is_up_to_date(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;
    for (running, tag) in [
        ("0.1.0", "v0.1.0"),
        ("0.1.0", "v0.0.9"),
        ("0.10.0", "v0.9.0"),
        ("1.0.0", "v0.99.99"),
    ] {
        fake(running, ok(release(tag)), cx);
        check(cx);
        let toast = only(&workbench, cx);
        assert_eq!(toast.level, Level::Info, "{tag}");
        assert_eq!(
            toast.message,
            format!("Tenmoku is up to date (you have {running})"),
            "{tag}"
        );
        assert_eq!(toast.choices, None, "{tag}");
    }
}

#[gpui::test]
fn pre_release_and_bad_tags(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;
    let with = |key: &str, value: serde_json::Value| {
        let mut release = release("v0.2.0");
        release[key] = value;
        release
    };
    let mut replies = vec![
        with("draft", json!(true)),
        with("prerelease", json!(true)),
        with("tag_name", json!(2)),
        // What's opened only ever comes from the project's releases.
        with("html_url", json!("file:///etc/passwd")),
        with("html_url", json!("https://example.com/releases/v0.2.0")),
        with("html_url", json!(null)),
        json!({}),
        json!([]),
        json!(null),
    ];
    for tag in [
        "0.2.0",
        "",
        "v",
        "v0.2",
        "v0.2.0.1",
        "v0.2.",
        "v.2.0",
        "v0.2.0-rc1",
        "v0.2.0+build",
        "v+0.2.0",
        "v-0.2.0",
        "v00.2.0",
        "va.b.c",
        "V0.2.0",
        "v٠.٢.٠",
        "v99999999999999999999.0.0",
    ] {
        replies.push(with("tag_name", json!(tag)));
    }
    for reply in replies {
        fake("0.1.0", ok(reply.clone()), cx);
        check(cx);
        let toast = only(&workbench, cx);
        assert_eq!(toast.level, Level::Info, "{reply}");
        assert_eq!(toast.message, NO_RELEASE, "{reply}");
        assert_eq!(toast.choices, None, "{reply}");
    }
}

#[gpui::test]
fn failure_is_a_toast(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;
    let status = |status: u16| {
        Ok(Fetched {
            status,
            body: b"{\"message\": \"nope\"}".to_vec(),
        })
    };
    let not_json = Ok(Fetched {
        status: 200,
        body: b"<html>".to_vec(),
    });
    for (reply, level, says) in [
        (
            Err("couldn't connect:\n  no route to host".to_owned()),
            Level::Error,
            "Couldn't check for updates: couldn't connect: no route to host",
        ),
        (
            status(403),
            Level::Error,
            "Couldn't check for updates: GitHub refused it, usually its rate limit, so try again \
             later (HTTP 403)",
        ),
        (
            status(429),
            Level::Error,
            "Couldn't check for updates: GitHub refused it, usually its rate limit, so try again \
             later (HTTP 429)",
        ),
        (
            status(500),
            Level::Error,
            "Couldn't check for updates: GitHub answered HTTP 500",
        ),
        (
            status(301),
            Level::Error,
            "Couldn't check for updates: GitHub answered HTTP 301",
        ),
        (
            not_json,
            Level::Error,
            "Couldn't check for updates: GitHub's reply isn't valid JSON (HTTP 200)",
        ),
        // GitHub's answer for a project with no releases.
        (
            status(404),
            Level::Info,
            "No release was found on GitHub (HTTP 404)",
        ),
    ] {
        fake("0.1.0", reply, cx);
        check(cx);
        let toast = only(&workbench, cx);
        assert_eq!(toast.message, says);
        assert_eq!(toast.level, level, "{says}");
        assert_eq!(toast.detail, None, "{says}");
        assert_eq!(toast.choices, None, "{says}");
    }

    // A body that stops short keeps the status; one that isn't read can't.
    let read = |status: u16, chunks: Vec<Result<Vec<u8>, &str>>| {
        futures::executor::block_on(read_reply(status, futures::stream::iter(chunks)))
    };
    let too_long = vec![0; MAX_REPLY + 1];
    for (reply, says) in [
        (
            read(200, vec![Ok(b"{".to_vec()), Err("timed out")]),
            "Couldn't check for updates: GitHub's reply broke off: timed out (HTTP 200)",
        ),
        (
            read(200, vec![Ok(too_long)]),
            "Couldn't check for updates: GitHub's reply is too long (HTTP 200)",
        ),
        (
            read(500, vec![Err("timed out")]),
            "Couldn't check for updates: GitHub answered HTTP 500",
        ),
    ] {
        fake("0.1.0", reply, cx);
        check(cx);
        let toast = only(&workbench, cx);
        assert_eq!(toast.message, says);
        assert_eq!(toast.level, Level::Error, "{says}");
    }

    // A whole body comes through, however it's split.
    let body = release("v0.2.0").to_string().into_bytes();
    let (first, rest) = body.split_at(5);
    let reply = read(200, vec![Ok(first.to_vec()), Ok(rest.to_vec())]);
    assert_eq!(reply, Ok(Fetched { status: 200, body }));

    // A build whose own version can't be compared says so.
    fake("0.1.0-dev", ok(release("v0.2.0")), cx);
    check(cx);
    let toast = only(&workbench, cx);
    assert_eq!(toast.level, Level::Error);
    assert_eq!(
        toast.message,
        "Couldn't check for updates: this build's version, 0.1.0-dev, can't be compared"
    );
}

#[gpui::test]
fn fetch_runs_off_main_thread(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;

    // A fetcher that notes where it's called and waits to be let go.
    let calls = Arc::new(AtomicUsize::new(0));
    let on_main = Arc::new(Mutex::new(Vec::new()));
    let gates = Arc::new(Mutex::new(Vec::new()));
    let executor = cx.executor();
    let source = UpdateSource {
        fetch: Arc::new({
            let (calls, on_main, gates) = (calls.clone(), on_main.clone(), gates.clone());
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                on_main.lock().unwrap().push(executor.is_main_thread());
                let (release_it, gate) = oneshot::channel::<()>();
                gates.lock().unwrap().push(release_it);
                async move {
                    gate.await.ok();
                    ok(release("v0.1.0"))
                }
                .boxed()
            }
        }),
        running: "0.1.0".to_owned(),
    };
    cx.update(|_, cx| cx.set_global(source));

    // The command returns with the fetcher not yet called.
    start(cx);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    cx.run_until_parked();
    assert_eq!(*on_main.lock().unwrap(), [false]);
    let messages = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        (shown(workbench, cx).into_iter())
            .map(|toast| toast.message)
            .collect::<Vec<_>>()
    };
    assert_eq!(messages(&workbench, cx), ["Checking for updates…"]);

    // Another, while it's in flight, only says so.
    check(cx);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        messages(&workbench, cx),
        ["Checking for updates…", ALREADY_CHECKING]
    );

    // Its end takes the going toast away, and frees the next.
    let release_first = gates.lock().unwrap().remove(0);
    release_first.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(
        messages(&workbench, cx),
        [ALREADY_CHECKING, "Tenmoku is up to date (you have 0.1.0)"]
    );
    dismiss(&workbench, cx);
    check(cx);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    gates.lock().unwrap().remove(0).send(()).unwrap();
    cx.run_until_parked();
}

/// With no fake set, the test scheduler's own fetcher answers: never GitHub.
#[gpui::test]
fn no_fake_means_no_network(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;
    check(cx);
    let toast = only(&workbench, cx);
    assert_eq!(toast.level, Level::Error);
    assert_eq!(
        toast.message,
        "Couldn't check for updates: no fake fetcher is set"
    );
}

#[gpui::test]
fn tenmoku_release_page_is_opened(cx: &mut TestAppContext) {
    let (workbench, mut cx) = window(cx);
    let cx = &mut cx;

    fake("0.1.0", ok(release("v0.2.0")), cx);
    check(cx);
    let toasts = shown(&workbench, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert_eq!(
        toasts[0].message,
        "Tenmoku 0.2.0 is available (you have 0.1.0)"
    );
    toasts[0].choices.as_ref().unwrap().choose(0);
    cx.run_until_parked();
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://github.com/darylrobbins/tenmoku/releases/tag/v0.2.0")
    );
}
