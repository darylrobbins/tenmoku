// The E2E-1 suite (T1682): what reaches the bundled Tenmoku.app through real OS input and
// comes back through the disk, the process, the window title, the native menus, panels
// and alerts, and kiln's labelled icon buttons. Each test runs on a scratch set of its own,
// made from the fixture it declares in Fixtures.swift, and launches the app itself (some
// twice, on the same set, to check what a relaunch restores). Support.swift has the
// shared setup and helpers; docs/decisions/n5krsav71f-xcuitest-suite.md maps each test to
// the human check task it shortens.

import AppKit
import XCTest

final class SmokeTests: E2ETestCase {
    // MARK: Save paths (T251's disk half)

    /// cmd-S, then File > Save, each checked on disk.
    func testSaveWithCmdSAndFileMenu() throws {
        let notes = file("notes.txt")
        launchAndSettle(["notes.txt"], title: titled("notes.txt"))
        app.typeText("saved with cmd-S")
        expectTitle(titled("notes.txt", dirty: true))
        app.typeKey("s", modifierFlags: .command)
        expectText(notes, "saved with cmd-S")
        expectTitle(titled("notes.txt"))

        app.typeText(", then File > Save")
        expectTitle(titled("notes.txt", dirty: true))
        let titles = menuTitles("File")
        record("file-menu", titles.joined(separator: "\n"))
        for item in ["Save", "Save As\u{2026}", "Save All", "Open Recent\u{2026}", "Trust This Folder"] {
            XCTAssertTrue(titles.contains(item), "File menu has no \(item): \(titles)")
        }
        menu("File", "Save")
        expectText(notes, "saved with cmd-S, then File > Save")
        expectTitle(titled("notes.txt"))
    }

    /// File > Save All writes both dirty tabs; File > Save As… through the native panel
    /// writes a new file and retitles the tab, leaving the old file as it was.
    func testSaveAllAndSaveAs() throws {
        let a = file("a.txt"), b = file("b.txt"), copy = file("a-copy.txt")
        launchAndSettle(["a.txt", "b.txt"], title: titled("b.txt"))
        app.typeText("B ")
        expectTitle(titled("b.txt", dirty: true))
        app.typeKey("[", modifierFlags: [.command, .shift])
        expectTitle(titled("a.txt"))
        settle()
        app.typeText("A ")
        expectTitle(titled("a.txt", dirty: true))

        menu("File", "Save All")
        expectText(a, "A first file\n")
        expectText(b, "B second file\n")
        expectTitle(titled("a.txt"))

        app.typeText("copy ")
        expectTitle(titled("a.txt", dirty: true))
        menu("File", "Save As\u{2026}")
        // NSSavePanel, in the app's process: its name field holds the file's name.
        let name = app.textFields.matching(
            NSPredicate(format: "identifier == %@ OR value == %@", "saveAsNameTextField", "a.txt")
        ).firstMatch
        XCTAssertTrue(name.waitForExistence(timeout: Self.uiTimeout), "no save panel: \(app.debugDescription)")
        record("save-panel", "name field value: \(String(describing: name.value))")
        app.typeKey("a", modifierFlags: .command)
        app.typeText("a-copy.txt")
        app.typeKey(.return, modifierFlags: [])
        // Typed where the caret was left, after "A ".
        expectText(copy, "A copy first file\n")
        expectText(a, "A first file\n")
        expectTitle(titled("a-copy.txt"))
    }

    /// Each fixture file, edited at its start and saved, is its original bytes with the
    /// typed text inserted in its own encoding: the BOM, the encoding and CRLF line
    /// endings all kept.
    func testSaveEncodingsByteForByte() throws {
        struct Case {
            let bom: Int
            let typed: String
            /// Return after the typed text, as the file's line ending.
            let newline: Bool
            let encoding: String.Encoding
        }
        let cases: [String: Case] = [
            "windows-1252.txt": Case(bom: 0, typed: "e2e ", newline: false, encoding: .windowsCP1252),
            "utf16le-bom.txt": Case(bom: 2, typed: "e2e ", newline: false, encoding: .utf16LittleEndian),
            "utf8-bom.txt": Case(bom: 3, typed: "e2e ", newline: false, encoding: .utf8),
            "crlf.txt": Case(bom: 0, typed: "e2e", newline: true, encoding: .utf8),
        ]
        let order = ["crlf.txt", "utf16le-bom.txt", "utf8-bom.txt", "windows-1252.txt"]
        var originals: [String: Data] = [:]
        for name in order { originals[name] = try Data(contentsOf: file(name)) }
        launchAndSettle(order, title: titled(order.last!))

        var done: [String] = []
        while done.count < order.count {
            let current = title
            guard let name = order.first(where: { current == titled($0) }), !done.contains(name) else {
                XCTFail("the active tab \(current.debugDescription) is not one still to check (done: \(done))")
                return
            }
            let test = cases[name]!
            let original = originals[name]!
            var inserted = test.typed.data(using: test.encoding)!
            app.typeText(test.typed)
            if test.newline {
                app.typeKey(.return, modifierFlags: [])
                inserted.append("\r\n".data(using: test.encoding)!)
            }
            expectTitle(titled(name, dirty: true))
            app.typeKey("s", modifierFlags: .command)
            expectBytes(file(name), original.prefix(test.bom) + inserted + original.dropFirst(test.bom))
            expectTitle(titled(name))
            done.append(name)
            app.typeKey("w", modifierFlags: .command)
            if done.count < order.count {
                // The next tab, once loaded (its title reads "<name> — loading" before).
                let left = order.filter { !done.contains($0) }.map { titled($0) }
                let next = app.windows.matching(NSPredicate(format: "title IN %@", left)).firstMatch
                XCTAssertTrue(next.waitForExistence(timeout: Self.uiTimeout), "after closing \(name): \(windowTitles())")
                settle()
            }
        }
    }

    // MARK: The window title as the dirty and active-tab signal

    /// Edits mark the title, undo back to the saved text clears it, switching tabs
    /// shows the new tab's state, closing a dirty tab asks first, and closing the last
    /// tab leaves no file in the title.
    func testWindowTitleDirtyAndActive() throws {
        let a = file("a.txt"), b = file("b.txt")
        launchAndSettle(["a.txt", "b.txt"], title: titled("b.txt"))
        app.typeText("x")
        expectTitle(titled("b.txt", dirty: true))
        app.typeKey("z", modifierFlags: .command)
        expectTitle(titled("b.txt"))

        app.typeText("y")
        expectTitle(titled("b.txt", dirty: true))
        app.typeKey("[", modifierFlags: [.command, .shift])
        expectTitle(titled("a.txt"))
        app.typeKey("]", modifierFlags: [.command, .shift])
        expectTitle(titled("b.txt", dirty: true))
        app.typeKey("s", modifierFlags: .command)
        expectText(b, "ysecond file\n")
        expectTitle(titled("b.txt"))

        app.typeKey("[", modifierFlags: [.command, .shift])
        expectTitle(titled("a.txt"))
        settle()
        app.typeText("z")
        expectTitle(titled("a.txt", dirty: true))
        // A dirty tab asks before it closes (a native alert); Cancel keeps it.
        app.typeKey("w", modifierFlags: .command)
        let cancel = nativeButton("Cancel")
        XCTAssertTrue(cancel.waitForExistence(timeout: Self.uiTimeout), "closing a dirty tab didn't ask")
        XCTAssertTrue(nativeButton("Discard").exists, "the close alert has no Discard")
        cancel.click()
        expectTitle(titled("a.txt", dirty: true))
        app.typeKey("z", modifierFlags: .command)
        expectTitle(titled("a.txt"))
        XCTAssertEqual(text(a), "first file\n")

        app.typeKey("w", modifierFlags: .command)
        expectTitle(titled("b.txt"))
        app.typeKey("w", modifierFlags: .command)
        let empty = app.windows.matching(NSPredicate(format: "NOT (title CONTAINS %@)", ".txt")).firstMatch
        XCTAssertTrue(empty.waitForExistence(timeout: Self.uiTimeout), "a file is still titled: \(windowTitles())")
        record("window-title-empty", "\(windowTitles())")
    }

    // MARK: Relaunch and hot exit (T526's disk half)

    /// Quit by the app menu with unsaved text: nothing is asked, the file on disk stays
    /// as it was, the text is backed up in files of mode 0600, and a relaunch on the same
    /// state brings the tab back dirty, to be saved.
    func testQuitByMenuHotExitAndRelaunch() throws {
        let notes = file("notes.txt")
        let typed = "kept by hot exit"
        launchAndSettle(["notes.txt"], title: titled("notes.txt"))
        app.typeText(typed)
        expectTitle(titled("notes.txt", dirty: true))
        quitByMenu()
        XCTAssertEqual(bytes(notes), Data(), "hot exit wrote notes.txt")

        let backups = state.appendingPathComponent("backups")
        let files = try relativeFiles(under: backups)
        var listing: [String] = []
        var holding = 0
        for path in files {
            let url = backups.appendingPathComponent(path)
            let mode = (try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int) ?? -1
            listing.append(String(format: "%o %@", mode, path))
            guard path.hasSuffix(".txt") || path.hasSuffix(".toml") else { continue }
            XCTAssertEqual(mode & 0o777, 0o600, "backup \(path) has mode \(String(format: "%o", mode))")
            if path.hasSuffix(".txt"), text(url) == typed { holding += 1 }
        }
        record("backups", listing.joined(separator: "\n"))
        // A backup has two slots, written in turn: the newest holds the whole text.
        XCTAssertGreaterThanOrEqual(holding, 1, "no backup holds the typed text: \(listing)")

        // A folder launch reopens the root's saved windows, backups included; a file
        // launch opens just the file (fastedit main.rs). The folder is undecided, so
        // it asks about trust first.
        launch([])
        answerTrustPrompt("Don't Trust")
        expectTitle(titled("notes.txt", dirty: true))
        waitForTree()
        settle()
        app.typeKey("s", modifierFlags: .command)
        expectText(notes, typed)
        expectTitle(titled("notes.txt"))
    }

    // MARK: First-run wizard, trust, Open Recent (T1344, T197, T528 disk halves)

    /// On an empty state folder the setup wizard shows once and its marker says so;
    /// Skip setup records that; a relaunch shows nothing and leaves the marker; Set Up
    /// Tenmoku… shows it again.
    func testFirstRunWizardOnce() throws {
        let marker = state.appendingPathComponent("setup.toml")
        func outcome() -> String? {
            text(marker)?.split(separator: "\n").first { $0.hasPrefix("outcome = ") }.map(String.init)
        }
        launch(["notes.txt"])
        expectTitle(titled("notes.txt"))
        expectButton("Skip setup", enabled: true)
        XCTAssertTrue(eventually { outcome() == #"outcome = "shown""# }, "marker: \(text(marker) ?? "none")")
        record("setup-shown", text(marker) ?? "none")
        clickButton("Skip setup")
        expectNoButton("Skip setup")
        XCTAssertTrue(eventually { outcome() == #"outcome = "skipped""# }, "marker: \(text(marker) ?? "none")")
        let skipped = bytes(marker)
        record("setup-skipped", text(marker) ?? "none")
        quitByKey()

        launchAndSettle(["notes.txt"], title: titled("notes.txt"))
        // The launch's decision is a stat or two, made off the main thread at its start,
        // and an offer would first rewrite the marker as shown: settled, neither happened.
        expectNoButton("Skip setup")
        XCTAssertEqual(bytes(marker), skipped, "the relaunch rewrote the marker")

        menu(Self.appName, "Set Up \(Self.appName)\u{2026}")
        expectButton("Skip setup", enabled: true)
        app.typeKey(.escape, modifierFlags: [])
        expectNoButton("Skip setup")
        XCTAssertTrue(eventually { outcome() == #"outcome = "skipped""# }, "marker: \(text(marker) ?? "none")")
    }

    /// A folder launch asks whether to trust the folder (a native alert); the answer,
    /// then File > Trust This Folder and Untrust This Folder, are written to trust.toml
    /// as the canonical folder's decision; a relaunch doesn't ask again.
    func testTrustByMenu() throws {
        let trustFile = state.appendingPathComponent("trust.toml")
        let folder = canonical(project)
        let key = "\"\(folder)\" = "
        func decision() -> String? {
            text(trustFile)?.split(separator: "\n").first { $0.hasPrefix(key) }.map { String($0.dropFirst(key.count)) }
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: trustFile.path))
        launch([])
        let dontTrust = nativeButton("Don't Trust")
        XCTAssertTrue(dontTrust.waitForExistence(timeout: Self.uiTimeout * 2), "a folder launch didn't ask about trust")
        XCTAssertTrue(nativeButton("Trust Folder").exists, "the trust alert has no Trust Folder")
        record("trust-alert", app.staticTexts.allElementsBoundByIndex.map(\.value).map { "\($0 ?? "")" }.joined(separator: "\n"))
        dontTrust.click()
        XCTAssertTrue(eventually { decision() == "false" }, "trust.toml: \(text(trustFile) ?? "none")")

        menu("File", "Trust This Folder")
        XCTAssertTrue(eventually { decision() == "true" }, "trust.toml: \(text(trustFile) ?? "none")")
        menu("File", "Untrust This Folder")
        XCTAssertTrue(eventually { decision() == "false" }, "trust.toml: \(text(trustFile) ?? "none")")
        menu("File", "Trust This Folder")
        XCTAssertTrue(eventually { decision() == "true" }, "trust.toml: \(text(trustFile) ?? "none")")
        let trusted = bytes(trustFile)
        record("trust", text(trustFile) ?? "none")
        quitByKey()

        launch([])
        settle()
        XCTAssertFalse(nativeButton("Don't Trust").exists, "a decided folder was asked about again")
        XCTAssertEqual(bytes(trustFile), trusted, "the relaunch changed trust.toml")
    }

    /// recent.toml records each file opened, most recent first, across relaunches;
    /// File > Open Recent… opens one by name.
    func testOpenRecentAndRecentToml() throws {
        let recent = state.appendingPathComponent("recent.toml")
        // A file launch records the file, not its folder ("a file's folder is only the
        // root it opens in", workbench.rs); folders come from folder launches.
        let quiz = file("quiz.txt").path, notes = file("notes.txt").path
        func lists(_ paths: String...) -> Bool {
            guard let toml = text(recent) else { return false }
            return paths.allSatisfy { toml.contains("\"\($0)\"") }
        }
        launchAndSettle(["quiz.txt"], title: titled("quiz.txt"))
        XCTAssertTrue(eventually { lists(quiz) }, "recent.toml: \(text(recent) ?? "none")")
        record("recent-first", text(recent) ?? "none")
        quitByKey()

        launchAndSettle(["notes.txt"], title: titled("notes.txt"))
        XCTAssertTrue(
            eventually {
                guard lists(quiz, notes), let toml = text(recent),
                      let n = toml.range(of: "\"\(notes)\""), let q = toml.range(of: "\"\(quiz)\"")
                else { return false }
                return n.lowerBound < q.lowerBound
            }, "recent.toml doesn't list notes.txt before quiz.txt: \(text(recent) ?? "none")")
        record("recent-second", text(recent) ?? "none")

        // The picker's rows have no nodes yet (A11Y-6): type a query only quiz.txt
        // matches (no letter q in any scratch path) and take the first row.
        menu("File", "Open Recent\u{2026}")
        app.typeText("quiz")
        app.typeKey(.return, modifierFlags: [])
        expectTitle(titled("quiz.txt"))
    }

    // MARK: The vim clipboard against NSPasteboard (T361)

    /// clipboard = "": yy and p keep to vim's own register and leave the pasteboard as
    /// it was; "+yy writes it.
    func testVimClipboardOff() throws {
        preservePasteboard()
        let notes = file("notes.txt")
        setPasteboard("set by the test\n")
        launchAndSettle(["notes.txt"], title: titled("notes.txt"))
        app.typeText("yyP")
        expectTitle(titled("notes.txt", dirty: true))
        app.typeText(":w")
        app.typeKey(.return, modifierFlags: [])
        expectText(notes, "alpha\nalpha\nbeta\n")
        record("pasteboard-after-yy", pasteboard ?? "nil")
        XCTAssertEqual(pasteboard, "set by the test\n", "yy with clipboard off changed the pasteboard")

        app.typeText("j\"+yy")
        XCTAssertTrue(eventually { pasteboard == "alpha\n" }, "\"+yy left the pasteboard \(pasteboard.debugDescription)")
        record("pasteboard-after-plus-yy", pasteboard ?? "nil")
    }

    /// clipboard = "unnamedplus": yy writes the pasteboard, and P puts what another app
    /// left there.
    func testVimClipboardUnnamedplus() throws {
        preservePasteboard()
        let notes = file("notes.txt")
        setPasteboard("set by the test\n")
        launchAndSettle(["notes.txt"], title: titled("notes.txt"))
        app.typeText("yy")
        XCTAssertTrue(eventually { pasteboard == "alpha\n" }, "yy left the pasteboard \(pasteboard.debugDescription)")
        record("pasteboard-after-yy", pasteboard ?? "nil")

        setPasteboard("from another app\n")
        app.typeText("P")
        expectTitle(titled("notes.txt", dirty: true))
        app.typeText(":w")
        app.typeKey(.return, modifierFlags: [])
        expectText(notes, "from another app\nalpha\nbeta\n")
    }

    // MARK: Labelled buttons: markdown preview (T1008) and the pane toolbar

    /// The preview buttons show on a markdown tab only; Open Preview swaps the tab to
    /// the preview (Show Source) and back, the title's unsaved mark kept throughout.
    func testMarkdownPreviewButtons() throws {
        let md = file("notes.md")
        let original = try Data(contentsOf: md)
        launchAndSettle(["notes.txt", "notes.md"], title: titled("notes.md"))
        expectButton("Open Preview", enabled: true)
        expectButton("Open Preview to the Side", enabled: true)

        app.typeKey("[", modifierFlags: [.command, .shift])
        expectTitle(titled("notes.txt"))
        expectNoButton("Open Preview")
        expectNoButton("Open Preview to the Side")
        expectButton(Self.treeMarker, enabled: true)

        app.typeKey("]", modifierFlags: [.command, .shift])
        expectTitle(titled("notes.md"))
        expectButton("Open Preview", enabled: true)
        settle()
        app.typeText("x")
        expectTitle(titled("notes.md", dirty: true))

        // The preview shows in the tab's place, which keeps the source tab's title.
        clickButton("Open Preview")
        expectButton("Show Source", enabled: true)
        expectNoButton("Open Preview")
        expectNoButton("Open Preview to the Side")
        expectTitle(titled("notes.md", dirty: true))
        clickButton("Show Source")
        expectTitle(titled("notes.md", dirty: true))
        expectButton("Open Preview", enabled: true)
        XCTAssertEqual(bytes(md), original, "the preview round trip wrote notes.md")
    }

    /// Back and forward are disabled with no history, follow a go-to-line jump and
    /// each other, and Split Right makes a second pane with buttons of its own.
    func testPaneToolbarEnabledState() throws {
        launchAndSettle(["lines.txt"], title: titled("lines.txt"))
        expectButton("Go Back", enabled: false)
        expectButton("Go Forward", enabled: false)
        expectButton("Split Right", enabled: true)
        expectButton("Split Down", enabled: true)
        expectButton("More Actions", enabled: true)

        app.typeKey("g", modifierFlags: .control)
        app.typeText("30")
        app.typeKey(.return, modifierFlags: [])
        expectButton("Go Back", enabled: true)
        expectButton("Go Forward", enabled: false)

        clickButton("Go Back")
        expectButton("Go Back", enabled: false)
        expectButton("Go Forward", enabled: true)
        clickButton("Go Forward")
        expectButton("Go Back", enabled: true)
        expectButton("Go Forward", enabled: false)

        expectButtonCount("Split Right", 1)
        clickButton("Split Right")
        expectButtonCount("Split Right", 2)
        expectButtonCount("Split Down", 2)
        expectTitle(titled("lines.txt"))
    }

    // MARK: The runner's sandbox

    /// The runner may write its run's scratch folder (the read-write exception the
    /// script signs it with), the way a test would change a file behind the app's back
    /// (T400, T526), and nowhere beside it. Launches nothing.
    func testRunnerWritesOnlyItsScratch() throws {
        let fm = FileManager.default
        let inside = home.appendingPathComponent("runner-write-probe.txt")
        var report = ""
        do {
            try Data("written by the runner\n".utf8).write(to: inside)
            report += "inside \(inside.path): written\n"
        } catch {
            report += "inside \(inside.path): \(error)\n"
            XCTFail("the runner can't write its scratch set: \(error)")
        }
        XCTAssertEqual(text(inside), "written by the runner\n")
        try? fm.removeItem(at: inside)

        let outside = scratch.deletingLastPathComponent()
            .appendingPathComponent("runner-write-probe-\(slot).txt")
        do {
            try Data("must not be written\n".utf8).write(to: outside)
            try? fm.removeItem(at: outside)
            report += "outside \(outside.path): written\n"
            XCTFail("the runner wrote outside its scratch folder: \(outside.path)")
        } catch {
            report += "outside \(outside.path): refused, \(error)\n"
        }
        record("sandbox-probe", report)
    }
}
