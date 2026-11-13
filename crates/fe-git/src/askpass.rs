//! Asking the user for what a remote wants and git can't find: a username
//! or password for an https remote, or an ssh key's passphrase or an ssh
//! password, when no credential helper or ssh-agent key answers.
//!
//! A remote command run with an [`Askpass`] (see [`Repo::set_askpass`])
//! gets a private directory of its own under the askpass's directory
//! (mode 700), removed once git exits, with a unix socket in it (mode 600)
//! and a two-line script that execs the askpass's helper with the socket's
//! path and the prompt. `GIT_ASKPASS` points git at that script, and, unless
//! the user has an ssh command of their own (which then runs as it did
//! before), `SSH_ASKPASS` with `SSH_ASKPASS_REQUIRE=force` points ssh at it
//! in place of batch mode. The helper ([`helper_main`]) sends the prompt
//! over the socket and prints what comes back, and the askpass's answer
//! function, called on a thread of the command's, says what that is.
//!
//! Git and ssh run `GIT_ASKPASS` and `SSH_ASKPASS` as a bare program with
//! the prompt as its only argument, so the script is what passes the
//! helper its mode (an app's own executable, run as `<app> --askpass`) and
//! the socket. The helper isn't a script that talks to the socket itself:
//! no tool for unix sockets is on every Mac and Linux.
//!
//! The answer goes from the socket to the helper's stdout and on to git,
//! and is never written to a file or logged here. Nor do the user's
//! credential helpers store it: the command's only helper is a script of
//! the session's that looks credentials up with the user's helpers (`git
//! credential fill`, prompts off) and passes on their erasing, but never
//! their storing.
//!
//! Cancelling (an answer function returning `None`) is for the whole
//! command: git, and all it runs, ssh included, is stopped, so nothing
//! retries or tries another key, any later prompt is cancelled unasked,
//! and the command fails with [`RemoteError::Auth`], as it did when it
//! couldn't prompt. A prompt can also be withdrawn before it's answered
//! ([`Withdrawn`]): its helper went (ssh stops the one it starts to show a
//! notice, say), or the command ended. Ssh's notices themselves
//! (`SSH_ASKPASS_PROMPT=none`, as for touching a security key) ask nothing,
//! and the helper ends at once.
//!
//! Without an askpass, a remote command never prompts (see
//! [`crate::remote`]). Windows isn't covered: there the askpass is ignored
//! and every prompt stays off, as unix sockets and `#!/bin/sh` scripts are
//! what the helper is reached by.
//!
//! [`Repo::set_askpass`]: crate::Repo::set_askpass
//! [`RemoteError::Auth`]: crate::RemoteError::Auth

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What git or ssh asks the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskpassRequest {
    /// The remote the command talks to, as `origin`.
    pub remote: String,
    /// The prompt git or ssh gave, as `Password for 'https://u@host': `.
    pub prompt: String,
}

type Answer = dyn Fn(&AskpassRequest, &Withdrawn) -> Option<String> + Send + Sync;

/// How a remote command asks the user: see the [module docs](self).
#[derive(Clone)]
pub struct Askpass {
    helper: Vec<OsString>,
    dir: PathBuf,
    answer: Arc<Answer>,
}

impl Askpass {
    /// `helper` is the program, then any arguments, that git's prompts run
    /// with the socket's path and the prompt after them, as
    /// [`helper_main`] takes them. Each command's private directory is made
    /// in `dir`. `answer` is called on a thread of the command's, blocking
    /// git until it returns the answer, or `None` to cancel the command; it
    /// should return as soon as it can once its request is [`Withdrawn`],
    /// whatever it returns then being dropped.
    pub fn new(
        helper: Vec<OsString>,
        dir: PathBuf,
        answer: impl Fn(&AskpassRequest, &Withdrawn) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            helper,
            dir,
            answer: Arc::new(answer),
        }
    }
}

impl fmt::Debug for Askpass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Askpass")
            .field("helper", &self.helper)
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

/// Whether a request no longer wants an answer: its helper went, or its
/// command ended, before it was answered.
#[derive(Clone, Default)]
pub struct Withdrawn(Arc<Mutex<Withdrawal>>);

#[derive(Default)]
struct Withdrawal {
    withdrawn: bool,
    /// Answered: it's too late to withdraw.
    settled: bool,
    waiting: Vec<Box<dyn FnOnce() + Send>>,
}

impl Withdrawn {
    pub fn is_withdrawn(&self) -> bool {
        self.0.lock().is_ok_and(|state| state.withdrawn)
    }

    /// Runs `then` once it's withdrawn: now if it is, never if it's
    /// answered first. On whatever thread withdraws it.
    pub fn on_withdrawn(&self, then: impl FnOnce() + Send + 'static) {
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        if state.withdrawn {
            drop(state);
            then();
        } else if !state.settled {
            state.waiting.push(Box::new(then));
        }
    }

    /// Withdraws it, unless it's been answered.
    #[cfg_attr(not(unix), allow(dead_code))]
    fn withdraw(&self) {
        let waiting = match self.0.lock() {
            Ok(mut state) if !state.withdrawn && !state.settled => {
                state.withdrawn = true;
                std::mem::take(&mut state.waiting)
            }
            _ => return,
        };
        for then in waiting {
            then();
        }
    }

    /// It's been answered, or the answer's being dropped: whether it was
    /// withdrawn first.
    #[cfg_attr(not(unix), allow(dead_code))]
    fn settle(&self) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return true;
        };
        state.settled = true;
        state.waiting.clear();
        state.withdrawn
    }
}

impl fmt::Debug for Withdrawn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Withdrawn")
            .field(&self.is_withdrawn())
            .finish()
    }
}

/// The helper's side, for an app's `--askpass` mode: `args` are the socket's
/// path and the prompt, as the script passes them. Prints the answer to
/// stdout; the exit code is 0 when there was one, 1 when it was cancelled or
/// couldn't be asked for. An ssh notice (`SSH_ASKPASS_PROMPT=none`) asks
/// nothing and is 0 at once.
pub fn helper_main(args: impl IntoIterator<Item = OsString>) -> i32 {
    helper(args, &mut std::io::stdout().lock())
}

/// [`helper_main`], printing the answer to `out`.
pub fn helper(args: impl IntoIterator<Item = OsString>, out: &mut dyn std::io::Write) -> i32 {
    #[cfg(unix)]
    {
        if std::env::var_os("SSH_ASKPASS_PROMPT").is_some_and(|hint| hint == "none") {
            return 0;
        }
        let mut args = args.into_iter();
        let (Some(socket), Some(prompt)) = (args.next(), args.next()) else {
            return 1;
        };
        let Some(answer) = ask(socket.as_ref(), &prompt.to_string_lossy()) else {
            return 1;
        };
        match writeln!(out, "{answer}").and_then(|()| out.flush()) {
            Ok(()) => 0,
            Err(_) => 1,
        }
    }
    #[cfg(not(unix))]
    {
        drop((args.into_iter(), out));
        1
    }
}

#[cfg(unix)]
pub(crate) use unix::Session;
#[cfg(unix)]
pub use unix::ask;

#[cfg(unix)]
mod unix {
    use std::ffi::OsStr;
    use std::fs::{self, DirBuilder, OpenOptions, Permissions};
    use std::io::{self, Read as _, Write as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{Answer, Askpass, AskpassRequest, Withdrawn};

    /// The longest prompt read; git's and ssh's are a line.
    const MAX_PROMPT: u32 = 4096;

    /// How long the helper or the socket's thread waits on the other end
    /// to send the prompt, once connected. Not the user's answer, which
    /// takes as long as it takes.
    const SEND_TIMEOUT: Duration = Duration::from_secs(10);

    /// What a reply starts with: an answer follows, or there's none.
    const ANSWERED: u8 = b'+';
    const CANCELLED: u8 = b'-';

    /// The script the command's only credential helper is: it looks
    /// credentials up with the user's helpers (`git credential fill`, the
    /// session's own config, `GIT_CONFIG_PARAMETERS`, left out), passes
    /// erasing on, and never stores. Fill's prompts answer empty
    /// (`GIT_ASKPASS=true`) and those empty fields are dropped, so what a
    /// helper gave of a credential is passed on and git asks for the rest;
    /// a helper's `quit` is passed on as fill reports it.
    const CREDENTIAL_SCRIPT: &str = "#!/bin/sh\n\
        unset GIT_CONFIG_PARAMETERS SSH_ASKPASS SSH_ASKPASS_REQUIRE\n\
        GIT_ASKPASS=true GIT_TERMINAL_PROMPT=0 GCM_INTERACTIVE=never\n\
        export GIT_ASKPASS GIT_TERMINAL_PROMPT GCM_INTERACTIVE\n\
        case $1 in\n\
        get)\n\
        \x20 if found=$(git credential fill 2>&1); then\n\
        \x20   printf '%s\\n' \"$found\" | grep -E '^[a-z_]+(\\[\\])?=.'\n\
        \x20 else\n\
        \x20   case $found in *'told us to quit'*) echo quit=1;; esac\n\
        \x20 fi;;\n\
        erase) git credential reject 2>/dev/null;;\n\
        esac\n\
        exit 0\n";

    /// Asks the app listening on `socket` about `prompt`: its answer, or
    /// `None` if it cancelled or couldn't be reached. The connection stays
    /// open until the answer comes, so the app knows the helper's still
    /// waiting.
    pub fn ask(socket: &Path, prompt: &str) -> Option<String> {
        let mut stream = UnixStream::connect(socket).ok()?;
        stream.set_write_timeout(Some(SEND_TIMEOUT)).ok()?;
        let prompt = &prompt.as_bytes()[..prompt.len().min(MAX_PROMPT as usize)];
        let len = u32::try_from(prompt.len()).ok()?;
        stream.write_all(&len.to_be_bytes()).ok()?;
        stream.write_all(prompt).ok()?;
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply).ok()?;
        match reply.split_first() {
            Some((&ANSWERED, answer)) => String::from_utf8(answer.to_vec()).ok(),
            _ => None,
        }
    }

    /// What a session's thread and its owner share.
    #[derive(Default)]
    struct Shared {
        /// Its command's done.
        done: AtomicBool,
        /// A prompt was cancelled: so is the command, and every prompt after.
        cancelled: AtomicBool,
        /// Git's process group, once it's started; 0 till then.
        group: AtomicU32,
        /// The request being asked, if one is.
        asking: Mutex<Option<Withdrawn>>,
    }

    impl Shared {
        /// Cancels the command: stops git's process group, ssh and the
        /// helper with it, now if it's started, else as it starts.
        fn cancel(&self) {
            // Stored before the group's read, as `started` stores the group
            // before it reads this: one of them sees the other.
            self.cancelled.store(true, Ordering::SeqCst);
            stop(self.group.load(Ordering::SeqCst));
        }

        /// Git's started as its process group's leader, `group`: stopped
        /// at once if a prompt was cancelled meanwhile.
        fn started(&self, group: u32) {
            self.group.store(group, Ordering::SeqCst);
            if self.cancelled.load(Ordering::SeqCst) {
                stop(group);
            }
        }
    }

    /// Stops process group `group`, unless it's 0, for not started.
    fn stop(group: u32) {
        let pid = i32::try_from(group)
            .ok()
            .and_then(rustix::process::Pid::from_raw);
        if let Some(pid) = pid {
            // Already gone, it has nothing left to stop.
            rustix::process::kill_process_group(pid, rustix::process::Signal::TERM).ok();
        }
    }

    /// One remote command's private directory, socket and scripts, and the
    /// thread answering on the socket, for as long as it lives.
    pub(crate) struct Session {
        dir: PathBuf,
        socket: PathBuf,
        script: PathBuf,
        credential: PathBuf,
        shared: Arc<Shared>,
    }

    impl Session {
        /// Makes the directory, socket and scripts for a command that talks
        /// to `remote`, and starts answering.
        pub(crate) fn start(askpass: &Askpass, remote: &str) -> io::Result<Session> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            // Git runs the scripts from the working tree, not from here.
            let parent = std::path::absolute(&askpass.dir)?;
            fs::create_dir_all(&parent)?;
            let name = format!(
                "askpass-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let dir = parent.join(name);
            // Fails if it's there, so nothing else's is reused.
            DirBuilder::new().mode(0o700).create(&dir)?;
            let session = Session {
                socket: dir.join("s"),
                script: dir.join("askpass"),
                credential: dir.join("credential"),
                dir,
                shared: Arc::default(),
            };
            let listener = match session.listen(askpass) {
                Ok(listener) => listener,
                Err(error) => {
                    // Nothing to wake as it drops: just the directory to
                    // remove.
                    session.shared.done.store(true, Ordering::SeqCst);
                    return Err(error);
                }
            };
            let (shared, answer) = (session.shared.clone(), askpass.answer.clone());
            let remote = remote.to_owned();
            std::thread::Builder::new()
                .name("fe-git askpass".into())
                .spawn(move || serve(listener, &shared, &remote, &*answer))?;
            Ok(session)
        }

        /// Binds the socket and writes the scripts.
        fn listen(&self, askpass: &Askpass) -> io::Result<UnixListener> {
            let listener = UnixListener::bind(&self.socket)?;
            fs::set_permissions(&self.socket, Permissions::from_mode(0o600))?;
            let mut text = b"#!/bin/sh\nexec".to_vec();
            let words = (askpass.helper.iter().map(|word| word.as_os_str()))
                .chain([self.socket.as_os_str()]);
            for word in words {
                text.push(b' ');
                text.extend(quoted(word));
            }
            text.extend(b" \"$1\"\n");
            write_script(&self.script, &text)?;
            write_script(&self.credential, CREDENTIAL_SCRIPT.as_bytes())?;
            Ok(listener)
        }

        /// What `GIT_ASKPASS` and `SSH_ASKPASS` run.
        pub(crate) fn script(&self) -> &Path {
            &self.script
        }

        /// The config git runs with, before its command: the command's only
        /// credential helper is the session's.
        pub(crate) fn config_args(&self) -> Vec<std::ffi::OsString> {
            let mut helper = b"credential.helper=!".to_vec();
            helper.extend(quoted(self.credential.as_os_str()));
            vec![
                "-c".into(),
                "credential.helper=".into(),
                "-c".into(),
                OsStr::from_bytes(&helper).to_owned(),
            ]
        }

        /// Git's started as its process group's leader, `group`.
        pub(crate) fn started(&self, group: u32) {
            self.shared.started(group);
        }

        /// Whether a prompt was cancelled, and git with it.
        pub(crate) fn cancelled(&self) -> bool {
            self.shared.cancelled.load(Ordering::SeqCst)
        }
    }

    impl Drop for Session {
        /// Stops answering, withdraws the request being asked, if one is,
        /// and removes the directory.
        fn drop(&mut self) {
            if !self.shared.done.swap(true, Ordering::SeqCst) {
                // Wakes the thread from `accept`, to see it's done.
                drop(UnixStream::connect(&self.socket));
            }
            // Done's set first, and the thread checks it as it registers a
            // request, under the same lock: it's this one's to withdraw, or
            // the thread sees it's done.
            let asking = self
                .shared
                .asking
                .lock()
                .ok()
                .and_then(|mut asking| asking.take());
            if let Some(withdrawn) = asking {
                withdrawn.withdraw();
            }
            // Nothing secret's in it if this fails, and the next command
            // uses another name.
            fs::remove_dir_all(&self.dir).ok();
        }
    }

    fn write_script(path: &Path, text: &[u8]) -> io::Result<()> {
        let mut script = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(path)?;
        script.write_all(text)
    }

    /// Answers each prompt sent on `listener`, one at a time, until done.
    fn serve(listener: UnixListener, shared: &Shared, remote: &str, answer: &Answer) {
        for stream in listener.incoming() {
            if shared.done.load(Ordering::SeqCst) {
                return;
            }
            let Ok(mut stream) = stream else { continue };
            if let Some(reply) = ask_on(&mut stream, shared, remote, answer) {
                // A helper gone meanwhile (git stopped) has nothing to tell.
                stream.write_all(&reply).ok();
            }
            // Ends the stream for the helper, which then fails unless it
            // was told an answer, and for the watch, whose copy of it is
            // open.
            stream.shutdown(std::net::Shutdown::Both).ok();
        }
    }

    /// Asks about the prompt sent on `stream`: what to reply, if anything.
    fn ask_on(
        stream: &mut UnixStream,
        shared: &Shared,
        remote: &str,
        answer: &Answer,
    ) -> Option<Vec<u8>> {
        if shared.cancelled.load(Ordering::SeqCst) {
            return Some(vec![CANCELLED]);
        }
        let prompt = read_prompt(stream)?;
        let withdrawn = Withdrawn::default();
        {
            let mut asking = shared.asking.lock().ok()?;
            // The session ended as the prompt came.
            if shared.done.load(Ordering::SeqCst) {
                return None;
            }
            *asking = Some(withdrawn.clone());
        }
        // The helper sends nothing more: the stream's end is it gone.
        if let Ok(mut watched) = stream.try_clone() {
            let withdrawn = withdrawn.clone();
            let watch = move || {
                let _ = watched.set_read_timeout(None);
                let _ = watched.read(&mut [0]);
                withdrawn.withdraw();
            };
            std::thread::Builder::new()
                .name("fe-git askpass watch".into())
                .spawn(watch)
                .ok();
        }
        let request = AskpassRequest {
            remote: remote.to_owned(),
            prompt,
        };
        let answered = answer(&request, &withdrawn);
        if let Ok(mut asking) = shared.asking.lock() {
            *asking = None;
        }
        if withdrawn.settle() {
            return None;
        }
        Some(match answered {
            Some(answer) => [&[ANSWERED], answer.as_bytes()].concat(),
            None => {
                shared.cancel();
                vec![CANCELLED]
            }
        })
    }

    /// The prompt a helper sent, its length first.
    fn read_prompt(stream: &mut UnixStream) -> Option<String> {
        stream.set_read_timeout(Some(SEND_TIMEOUT)).ok()?;
        let mut len = [0; 4];
        stream.read_exact(&mut len).ok()?;
        let len = u32::from_be_bytes(len);
        if len > MAX_PROMPT {
            return None;
        }
        let mut prompt = vec![0; len as usize];
        stream.read_exact(&mut prompt).ok()?;
        Some(String::from_utf8_lossy(&prompt).into_owned())
    }

    /// `word` single-quoted for `sh`.
    fn quoted(word: &OsStr) -> Vec<u8> {
        let mut quoted = vec![b'\''];
        for &byte in word.as_bytes() {
            if byte == b'\'' {
                quoted.extend(b"'\\''");
            } else {
                quoted.push(byte);
            }
        }
        quoted.push(b'\'');
        quoted
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quoting_keeps_quotes_and_spaces() {
            assert_eq!(quoted(OsStr::new("a b")), b"'a b'");
            assert_eq!(quoted(OsStr::new("it's")), b"'it'\\''s'");
        }

        /// A `sleep` in a process group of its own, as git runs.
        fn grouped_sleep() -> std::process::Child {
            let mut command = std::process::Command::new("sleep");
            command.arg("30");
            std::os::unix::process::CommandExt::process_group(&mut command, 0);
            command.spawn().unwrap()
        }

        /// Whether `child` ends within a few seconds.
        fn ends(child: &mut std::process::Child) -> bool {
            for _ in 0..300 {
                if child.try_wait().unwrap().is_some() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            child.kill().ok();
            false
        }

        #[test]
        fn a_cancel_before_git_starts_stops_it_as_it_starts() {
            let shared = Shared::default();
            shared.cancel();
            let mut git = grouped_sleep();
            shared.started(git.id());
            assert!(ends(&mut git));

            let shared = Shared::default();
            let mut git = grouped_sleep();
            shared.started(git.id());
            shared.cancel();
            assert!(ends(&mut git));
        }

        fn session(
            dir: &Path,
            answer: impl Fn(&AskpassRequest, &Withdrawn) -> Option<String> + Send + Sync + 'static,
        ) -> Session {
            let askpass = Askpass::new(Vec::new(), dir.to_owned(), answer);
            Session::start(&askpass, "origin").unwrap()
        }

        #[test]
        fn a_session_ending_as_a_prompt_comes_asks_nothing() {
            let dir = tempfile::tempdir().unwrap();
            let asked = Arc::new(AtomicBool::new(false));
            let seen = asked.clone();
            let session = session(dir.path(), move |_, _| {
                seen.store(true, Ordering::SeqCst);
                None
            });
            // Connected, the prompt not yet sent.
            let mut helper = UnixStream::connect(&session.socket).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            drop(session);
            helper.write_all(&4u32.to_be_bytes()).unwrap();
            helper.write_all(b"Pass").unwrap();
            let mut reply = Vec::new();
            helper.read_to_end(&mut reply).unwrap();
            assert_eq!(reply, b"");
            assert!(!asked.load(Ordering::SeqCst));
        }

        #[test]
        fn a_session_ending_mid_prompt_lets_its_helper_go() {
            let dir = tempfile::tempdir().unwrap();
            let (entered, asking) = std::sync::mpsc::channel();
            let session = session(dir.path(), move |_, withdrawn| {
                entered.send(()).ok();
                while !withdrawn.is_withdrawn() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                None
            });
            let socket = session.socket.clone();
            let (done, helper) = std::sync::mpsc::channel();
            std::thread::spawn(move || done.send(ask(&socket, "Password: ")).ok());
            asking.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(session);
            assert_eq!(helper.recv_timeout(Duration::from_secs(5)), Ok(None));
        }

        #[test]
        fn a_withdrawal_after_the_answer_is_nothing() {
            let withdrawn = Withdrawn::default();
            let told = Arc::new(AtomicBool::new(false));
            let heard = told.clone();
            withdrawn.on_withdrawn(move || heard.store(true, Ordering::SeqCst));
            assert!(!withdrawn.settle());
            withdrawn.withdraw();
            assert!(!withdrawn.is_withdrawn());
            assert!(!told.load(Ordering::SeqCst));

            let withdrawn = Withdrawn::default();
            withdrawn.withdraw();
            let heard = told.clone();
            withdrawn.on_withdrawn(move || heard.store(true, Ordering::SeqCst));
            assert!(told.load(Ordering::SeqCst));
            assert!(withdrawn.settle());
        }
    }
}
