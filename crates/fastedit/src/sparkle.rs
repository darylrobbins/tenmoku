//! Sparkle, the macOS updater: found in the bundle at launch, loaded at run
//! time rather than linked, so that `cargo run`, tests, Linux and bundles
//! without it stay as they were. With it running, Check for Updates… is
//! Sparkle's and `update.automatic_checks` turns its daily check on and off;
//! otherwise Check for Updates… asks GitHub (`fe_workbench::update_check`).

use std::path::{Path, PathBuf};

use gpui::App;

/// Where `Sparkle.framework` is when the executable `exe` sits in a bundle's
/// `X.app/Contents/MacOS/`, and the framework is there.
pub(crate) fn framework_path(exe: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let macos = exe.parent().filter(|dir| dir.ends_with("MacOS"))?;
    let contents = macos.parent().filter(|dir| dir.ends_with("Contents"))?;
    contents
        .parent()
        .filter(|app| app.extension().is_some_and(|ext| ext == "app"))?;
    let framework = contents.join("Frameworks/Sparkle.framework");
    framework.is_dir().then_some(framework)
}

/// What Sparkle does in this process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Policy {
    /// Not loaded: Check for Updates… asks GitHub.
    Off,
    /// Running for Check for Updates…, with its daily check off.
    Manual,
    /// Running, with its daily check on.
    Scheduled,
}

impl Policy {
    /// Off for a `--perf` or `--wait` launch and under test, or without a
    /// bundled framework and the feed keys it needs. Otherwise, the
    /// `update.automatic_checks` setting picks.
    pub(crate) fn decide(
        bundled_framework: bool,
        has_feed_keys: bool,
        perf: bool,
        wait_launch: bool,
        under_test: bool,
        automatic_checks: bool,
    ) -> Policy {
        if !bundled_framework || !has_feed_keys || perf || wait_launch || under_test {
            Policy::Off
        } else if automatic_checks {
            Policy::Scheduled
        } else {
            Policy::Manual
        }
    }
}

/// The `SULastCheckTime` to write before Sparkle starts, in seconds since the
/// Unix epoch, so that its first automatic check comes one interval after the
/// first launch rather than at once. A fresh install has no last check, which
/// Sparkle reads as "never" and checks for at once; a last check in the future
/// (the clock was set back) would delay checks for as long. A past last check is
/// Sparkle's own record, and is left alone.
///
/// Only the macOS platform module calls it; the tests run it everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn first_check_seed(last_check: Option<f64>, now: f64) -> Option<f64> {
    match last_check {
        Some(last) if last <= now => None,
        _ => Some(now),
    }
}

/// Starts Sparkle as [`Policy::decide`] says, on the main thread. A symlink's
/// launch counts as its bundle's: 'tenmoku' in PATH is one.
pub(crate) fn start(
    perf: bool,
    wait_launch: bool,
    under_test: bool,
    automatic_checks: bool,
    cx: &mut App,
) {
    let framework = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| framework_path(&exe));
    let policy = Policy::decide(
        framework.is_some(),
        framework.is_some() && platform::has_feed_keys(),
        perf,
        wait_launch,
        under_test,
        automatic_checks,
    );
    if let (Some(framework), Policy::Manual | Policy::Scheduled) = (framework, policy) {
        platform::start(&framework, policy == Policy::Scheduled, cx);
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    use gpui::App;

    pub(super) fn has_feed_keys() -> bool {
        false
    }

    pub(super) fn start(_framework: &Path, _automatic_checks: bool, _cx: &mut App) {}
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{path::Path, ptr, sync::Arc};

    use fe_config::Settings;
    use fe_workbench::update_check::{UpdaterHook, set_updater_hook};
    use gpui::{App, Global};
    use kiln::GlobalSettings;
    use objc2::{
        ClassType, msg_send, msg_send_id,
        rc::{Allocated, Retained},
        runtime::AnyObject,
    };
    use objc2_foundation::{
        MainThreadMarker, NSBundle, NSClassFromString, NSDate, NSString, NSUserDefaults,
    };

    use super::first_check_seed;

    /// The running updater, kept for the life of the app.
    struct Updater {
        /// An `SPUStandardUpdaterController`.
        controller: Retained<AnyObject>,
        /// The setting as last given to Sparkle.
        automatic_checks: bool,
    }

    impl Global for Updater {}

    impl Updater {
        /// Gives Sparkle the setting, which it also keeps in its own user
        /// defaults: a launch can't assume they already agree.
        ///
        /// With `only_if_changed`, Sparkle isn't told what it already has:
        /// setting it resets its update cycle, and so its schedule.
        fn set_automatic_checks(&mut self, automatic_checks: bool, only_if_changed: bool) {
            self.automatic_checks = automatic_checks;
            // SAFETY: `updater` is a property of the controller, which returns
            // an `SPUUpdater`, whose `automaticallyChecksForUpdates` property
            // is a BOOL.
            unsafe {
                let updater: Retained<AnyObject> = msg_send_id![&*self.controller, updater];
                if only_if_changed {
                    let current: bool = msg_send![&*updater, automaticallyChecksForUpdates];
                    if current == automatic_checks {
                        return;
                    }
                }
                let () = msg_send![&*updater, setAutomaticallyChecksForUpdates: automatic_checks];
            }
        }

        /// Starts the updater, which the controller was made without starting
        /// so that a failure (an invalid public key, a missing XPC service)
        /// comes back here rather than as a controller that does nothing.
        fn start_updater(&self) -> Result<(), String> {
            // SAFETY: `startUpdater:` takes an NSError out-pointer and returns
            // a BOOL.
            unsafe {
                let updater: Retained<AnyObject> = msg_send_id![&*self.controller, updater];
                let mut error: *mut AnyObject = ptr::null_mut();
                let ok: bool = msg_send![&*updater, startUpdater: &mut error];
                if ok {
                    return Ok(());
                }
                let description = (!error.is_null())
                    .then(|| {
                        let text: Option<Retained<NSString>> =
                            msg_send_id![&*error, localizedDescription];
                        text.map(|text| text.to_string())
                    })
                    .flatten();
                Err(description.unwrap_or_else(|| "its updater did not start".into()))
            }
        }

        /// Starts Sparkle's user-driven check, in its own windows.
        fn check(&self) {
            let sender: *const AnyObject = ptr::null();
            // SAFETY: the controller's action takes a nullable sender.
            let () = unsafe { msg_send![&*self.controller, checkForUpdates: sender] };
        }
    }

    /// Whether the app's Info.plist has the keys Sparkle needs: without them,
    /// starting it would only show the user an error.
    pub(super) fn has_feed_keys() -> bool {
        let bundle = NSBundle::mainBundle();
        ["SUFeedURL", "SUPublicEDKey"].iter().all(|key| {
            // SAFETY: a read of the main bundle's Info.plist.
            unsafe { bundle.objectForInfoDictionaryKey(&NSString::from_str(key)) }.is_some()
        })
    }

    /// Seeds `SULastCheckTime` in the app's user defaults when
    /// [`first_check_seed`] says so. Sparkle reads it when the updater starts.
    fn seed_last_check() {
        // SAFETY: NSUserDefaults and NSDate calls with matching types; the key
        // holds an NSDate when Sparkle wrote it.
        unsafe {
            let defaults = NSUserDefaults::standardUserDefaults();
            let key = NSString::from_str("SULastCheckTime");
            let now = NSDate::now().timeIntervalSince1970();
            let last = defaults.objectForKey(&key).and_then(|value| {
                let is_date: bool = msg_send![&*value, isKindOfClass: NSDate::class()];
                // SAFETY: just checked to be an NSDate.
                is_date.then(|| {
                    let secs: f64 = msg_send![&*value, timeIntervalSince1970];
                    secs
                })
            });
            if let Some(seed) = first_check_seed(last, now) {
                defaults
                    .setObject_forKey(Some(&*NSDate::dateWithTimeIntervalSince1970(seed)), &key);
            }
        }
    }

    /// Loads the framework and creates its standard updater controller, not
    /// yet started. If that fails, as it does for a bundle whose signature the
    /// framework's doesn't match, no hook is set and Check for Updates… asks
    /// GitHub as before.
    fn load(framework: &Path) -> Result<Retained<AnyObject>, &'static str> {
        MainThreadMarker::new().ok_or("not on the main thread")?;
        let path = NSString::from_str(&framework.to_string_lossy());
        // SAFETY: on the main thread, as the controller's creator must be. The
        // framework is the one signed into this bundle, and the initializer
        // takes a BOOL and two nullable delegates.
        unsafe {
            let bundle = NSBundle::bundleWithPath(&path).ok_or("not a bundle")?;
            if !bundle.load() {
                return Err("it did not load");
            }
            let class = NSClassFromString(&NSString::from_str("SPUStandardUpdaterController"))
                .as_ref()
                .ok_or("it has no SPUStandardUpdaterController")?;
            let delegate: *const AnyObject = ptr::null();
            let controller: Allocated<AnyObject> = msg_send_id![class, alloc];
            let controller: Option<Retained<AnyObject>> = msg_send_id![
                controller,
                initWithStartingUpdater: false,
                updaterDelegate: delegate,
                userDriverDelegate: delegate
            ];
            controller.ok_or("its updater controller did not initialize")
        }
    }

    /// Starts the updater from `framework`, takes over Check for Updates… and
    /// follows the setting from here on.
    pub(super) fn start(framework: &Path, automatic_checks: bool, cx: &mut App) {
        if automatic_checks {
            seed_last_check();
        }
        let controller = match load(framework) {
            Ok(controller) => controller,
            Err(why) => {
                log::warn!(
                    target: "fastedit",
                    "Sparkle at {}: {why}; Check for Updates… will ask GitHub",
                    framework.display()
                );
                return;
            }
        };
        let mut updater = Updater {
            controller,
            automatic_checks,
        };
        // Before the updater starts, so before its first scheduled look. A
        // scheduled launch leaves a value Sparkle already has alone; a manual
        // one overrides the plist's SUEnableAutomaticChecks always.
        updater.set_automatic_checks(automatic_checks, automatic_checks);
        if let Err(why) = updater.start_updater() {
            log::warn!(
                target: "fastedit",
                "Sparkle at {}: {why}; Check for Updates… will ask GitHub",
                framework.display()
            );
            return;
        }
        cx.set_global(updater);
        set_updater_hook(
            cx,
            UpdaterHook(Arc::new(|cx| {
                cx.try_global::<Updater>().is_some_and(|updater| {
                    updater.check();
                    true
                })
            })),
        );
        cx.observe_global::<GlobalSettings<Settings>>(|cx| {
            let now = cx.global::<GlobalSettings<Settings>>().settings();
            let now = now.update.automatic_checks;
            if cx.global::<Updater>().automatic_checks != now {
                cx.global_mut::<Updater>().set_automatic_checks(now, false);
            }
        })
        .detach();
    }
}
