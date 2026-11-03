# Offering to install syntax highlighting (T426)

Card 5 of `docs/grammar-size-audit/README.md`, second half: what the user sees
opening a file whose grammar isn't installed. The fetch and install are T425's
(`docs/decisions/c1pt00h80c-grammar-fetch.md`).

## A toast, not a banner

The audit describes a quiet banner in the editor. The only editor banner today
(`viewer.set_banner` in `pane.rs`) is text with no actions, and a new pane-chrome
widget can't be pixel-checked on the Linux host. Toasts already carry actions and
run under `TestAppContext`. A banner can replace this later without touching the
catalog or installer, since `grammar_prompt.rs` only asks and reports.

An offer stays until it's answered, as any toast with choices does.

## The workbench is told what can be installed

`fe_workbench::GrammarOffers` has the catalog, `installed`, `fetch`, `install` and
the Never file, set by the app with `set_grammar_offers`. fe-workbench gains no
network code and no dependency on the installer. Without offers, nothing is ever
offered, so no test reaches the network.

The spec has one `install`. It's two steps here because a background write can't
be called back once it's running, so only a download can honestly be cancelled:
see below. It also matches C3's "the fetcher, then the installer".

The real ones are `grammar_install::offers`, with the catalog from `pack.toml` and
each language's embedded `extension.toml`. The name shown is the manifest's, less
its " syntax". The steps are `fetch_first_party` and `write_first_party`, so they
use T425's `GRANTED_HOSTS` rather than the two hosts this card's spec lists, which
T425 found out of date.

**This never fires in the app yet.** All eight languages are built in, and none
has a recorded hash. It starts to with the default-features card and a grammar
release, and T425's notes on the private repository still apply before that.

## When it offers

The workbench looks at its editors whenever its workspace or itself notifies, and
acts only on an editor whose path it hasn't seen: a tab that opened, or one whose
path changed with a Save As or a rename. So a failed install asks again the next
time a file opens, and not at once. Looking costs a hash lookup per tab, and
nothing at all with no catalog.

It's hooked up with `observe_new`, as `chat_panel.rs` does, and needs no change to
`workbench.rs`.

A file is offered for if a released catalog entry has its extension, in any case,
and all of these hold:

- `installed` says no. In the app that means fe-syntax doesn't know the language
  and `<extensions>/tenmoku.grammar_<language>/` doesn't exist. The directory
  counts because at launch the loader may not have registered it yet, and because
  a disabled extension wouldn't be helped by downloading it again.
- fe-syntax doesn't give the file to a *different* language, as another
  extension's grammar might.
- Its language wasn't chosen by hand.

`installed` alone speaks for the entry's own language, and fe-syntax only for
other languages. The spec has the trigger as `language_for_path` finding nothing,
which comes to the same in the app, where `installed` also asks fe-syntax. It's
split this way because Kotlin is built into test builds: `language_for_path` could
never find nothing for `a.kt` there, and registering a fixture as Kotlin is a
`Clash`. It also uses `language_name_for_path`, which doesn't compile a query.

Only extensions are matched, as the spec says. A `Gemfile` isn't offered for,
though Ruby's manifest lists it: "Never for .ext" has nothing to name.

## Answers

- **Not now** silences the language in every window until the next launch.
- **Closing the toast**, or losing it from a full queue, counts as Not now, so that
  ten more files don't ask ten more times.
- **Never** is per extension, so Never for `.kt` leaves `.kts` asking. It's kept in
  `<state dir>/grammar_prompt.json` as `{"never": ["kt"]}`, and in memory for the
  session in case the file can't be written. The file is read off the main thread
  before each offer, so another window or process's answer counts, and written
  under a lock through a rename, so two writers both land. A file that doesn't read
  only means being asked again. The write starts from the answer itself, on no
  window's task, so closing the window straight after can't lose it. A failed write
  is logged.

## One prompt per language, owned by a window

A language being asked about or installed is in one app-wide map, so two windows
opening `.kt` files get one toast and one fetch. Each entry holds its task and
the window that showed it.

An extension that opens while its language's prompt is under way is kept with it.
Never is per extension, so when the prompt ends with Never for `.kt`, whether just
clicked or read from the file, it goes on to ask about a waiting `.kts`. Not now
and Install are for the language, so they drop what's waiting.

**Closing that window stops a download**, as `on_window_closed` drops the task.
This isn't tied to the workbench entity's release: the first version was, and the
integration test showed the install outliving the window. Nothing has been written
then, so the language can be offered again.

**From the moment the download is done, the prompt belongs to no window.** The
main thread gives up the window and starts the write in one step, so a write never
starts under a prompt that can still be dropped, and no window closing can orphan
what it publishes. An earlier version dropped the task whatever it was doing and
checked for the extension's directory on the way out, which missed a write still
running on its thread. The prompt stays under way until it registers, so nothing
is fetched twice meanwhile.

The offer and its results show in the owning window, even if it isn't the active
one, rather than through `toast::show`.

## After the install

Nothing says when the loader has registered an extension, and fe-syntax has no
generation counter. So the install polls `fe_syntax::language_names` every 250 ms.
After 60 s, long enough for the watcher's re-stat of a directory that didn't exist
(3 s) and for compiling a module of a few MB on a busy machine, a warning says it
hasn't loaded, the reason being in the log. It goes on looking every 2 s while a
workbench is open, since a late registration still needs its tabs highlighted, and
then says it's installed. fe-syntax is unchanged, as the spec asked.

Then each tab of the language, in every window, detects its language again:

- Tabs whose language was chosen by hand stay as they are.
- Hidden tabs are included.
- Every tab or diff side with a path saying it's the language detects it again.
  That's its model's path, or else its **language source**.
- A diff side with neither, git's text, is told by `DiffView::detect_languages_again`,
  from the file's name and failing that the right side's, as `DiffView::new` did.
  That beats what its first lines said when no language had the name yet, so a `.rb`
  with a bash shebang moves from Bash to Ruby. It's only reset if the answer differs.

A side or tab with no path of its own used to be given its language through
`set_language`, which is what the language picker calls, so it counted as picked by
hand and could never be detected again. There were three such callers, all now
recording detection as detection:

- `DiffView::new` clears `language_chosen` after a pathless side's language.
- Compare With File's left side and git log's file at a revision call the new
  `EditorView::detect_language_from(path)`, which keeps the path in the view. Its
  splits keep it too. A left side therefore goes by its own file, whatever the
  right's extension.

That widened this card's scope to `view.rs`, `pane_item.rs`, `diff_view.rs`,
`git_diff.rs` and `git_log_menu.rs`. Nothing else reads the flag for a view with no
path: `routed_language` needs a model path, and a `DiffView` has no split.

`highlight_epoch`, the view's own generation, is private to fe-gpui. So the tests
for criterion C3 see the generation advance as the view's `syntax()` going from
none to the language, then check the colour of a highlighted span.

## What a failure says

One line, whatever the reason holds: "Couldn't install Kotlin syntax: *reason*."
A lost connection, a timeout or a broken transfer count as offline and add a
mention of the language pack. They say "no connection to the download", and a hash
mismatch says so without the hashes. TLS failures don't count as offline. The toast
is an error, so it stays until dismissed, and the tab stays plain.

The language pack's own download is card 6 of the audit, which doesn't exist yet,
so the hint doesn't say where to get it.

## How the tests stand in for Kotlin

Kotlin is built in, so a test `a.kt` is never plain. Tests of what's asked and
answered use `a.kt`, with a fake `installed`. Tests needing a plain tab that then
highlights use a catalog entry named Kotlin with the language `ktfixture` and the
extension `ktfix`, which registers the JSON fixture grammar.
