// What every test in the suite shares (T1682): claiming its scratch set, launching the
// bundle under test by URL, waiting for gpui-pre's accessibility tree, the window title,
// the menus and the disk, and recording what it saw as attachments.
//
// Run through scripts/e2e-macos.sh, which bundles the app and passes, as
// TEST_RUNNER_<NAME> variables that the runner sees as <NAME>:
//   E2E_APP         the Tenmoku.app under test (re-identified as dev.tenmoku.tenmoku.e2e)
//   E2E_SCRATCH     the scratch sets, <E2E_SCRATCH>/<test>/<iteration>/ (see Fixtures.swift)
//   E2E_ITERATIONS  how many sets each test has
//   E2E_FIXTURES    tools/e2e-macos/fixtures, which each set's project is checked against
//
// Xcode 27's UI-test runner is sandboxed: it may read anywhere but write only in its
// container and, by the one read-write exception the script signs it with, this run's
// scratch folder (testRunnerWritesOnlyItsScratch). So the script makes the scratch sets
// from the fixtures, each test claims its next one with a counter in the runner's own
// defaults, and everything recorded (AX dumps, launch, disk and pasteboard records, the
// app's log) is an attachment, which the script exports and copies to its results folder.
//
// The app is launched by URL, never by bundle id: the installed copies in /Applications
// and ~/Applications share the shipped id. The app's HOME, config and state dirs are the
// set's, and TENMOKU_NO_IPC keeps it from handing off to the user's running Tenmoku.
//
// Editor text has no accessibility node yet (A11Y-5, A11Y-7; E2E-4 adds those checks), so
// what the editor holds is checked on disk after a save, and its dirty state through the
// window title.

import AppKit
import XCTest

class E2ETestCase: XCTestCase {
    /// The bundle id scripts/e2e-macos.sh gives the bundle under test.
    static let bundleID = "dev.tenmoku.tenmoku.e2e"
    static let appName = "Tenmoku"
    /// The one fixed settle delay. The window and its title are up before the
    /// workbench has read the active file and focused its editor, and nothing in the
    /// accessibility tree says when that has happened, so there is nothing to wait
    /// on. Keys sent earlier can land before the editor has focus.
    static let editorSettle: TimeInterval = 1.0
    /// How long gpui-pre's tree may take to show its first labelled button.
    static let treeTimeout: TimeInterval = 15.0
    /// How long the window, a title, a native panel or a gpui node may take.
    static let uiTimeout: TimeInterval = 10.0
    /// How long the disk or the pasteboard may take to show a change.
    static let diskTimeout: TimeInterval = 5.0
    /// How long a quit may take, backups and layout included.
    static let quitTimeout: TimeInterval = 20.0
    /// The labelled button whose presence says gpui-pre's tree is live: the pane's
    /// tab-row button, there in every pane whatever its tab.
    static let treeMarker = "Split Right"

    var app: XCUIApplication!
    var appURL: URL!
    /// The claimed scratch set's iteration, "001" and so on.
    var slot = ""
    var fixture: Fixture!
    var project: URL!
    var config: URL!
    var state: URL!
    var home: URL!
    /// This run's scratch folder, the one place the runner may write.
    var scratch: URL!

    override func setUpWithError() throws {
        continueAfterFailure = false
        let env = ProcessInfo.processInfo.environment
        guard let appPath = env["E2E_APP"], let scratch = env["E2E_SCRATCH"],
              let fixtures = env["E2E_FIXTURES"],
              let iterations = env["E2E_ITERATIONS"].flatMap(Int.init)
        else {
            XCTFail("E2E_APP, E2E_SCRATCH, E2E_ITERATIONS and E2E_FIXTURES must be set: run scripts/e2e-macos.sh")
            return
        }
        guard let declared = Fixtures.declared[shortName] else {
            XCTFail("\(shortName) has no entry in Fixtures.declared")
            return
        }
        fixture = declared
        appURL = URL(fileURLWithPath: appPath)
        self.scratch = URL(fileURLWithPath: scratch)
        // This test's next unused set. The runner is relaunched between iterations,
        // so the count lives in its defaults, keyed by this run's scratch folder.
        let defaults = UserDefaults.standard
        let key = "claimed:\(scratch):\(shortName)"
        let index = defaults.integer(forKey: key) + 1
        defaults.set(index, forKey: key)
        defaults.synchronize()
        guard index <= iterations else {
            XCTFail("\(shortName) has \(iterations) scratch sets in \(scratch), all used")
            return
        }
        slot = String(format: "%03d", index)
        let set = URL(fileURLWithPath: scratch)
            .appendingPathComponent(shortName).appendingPathComponent(slot)
        project = set.appendingPathComponent("project")
        config = set.appendingPathComponent("config")
        state = set.appendingPathComponent("state")
        home = set.appendingPathComponent("home")
        record("scratch", "\(set.path)\nfixture \(declared.folder) settings \(declared.settings ?? "-") fresh \(declared.fresh)")
        try checkFresh(fixtures: URL(fileURLWithPath: fixtures).appendingPathComponent(declared.folder))

        app = XCUIApplication(url: appURL)
        app.launchEnvironment = [
            "TENMOKU_CONFIG_DIR": config.path,
            "TENMOKU_STATE_DIR": state.path,
            "TENMOKU_NO_IPC": "1",
            // The macOS log goes to $HOME/Library/Logs/Tenmoku/tenmoku.log.
            "HOME": home.path,
        ]
    }

    override func tearDownWithError() throws {
        if let run = testRun, run.failureCount > 0, let app, app.state == .runningForeground {
            let shot = XCTAttachment(screenshot: app.screenshot())
            shot.name = "failure-\(shortName)-\(slot)"
            shot.lifetime = .keepAlways
            add(shot)
            record("ax-tree-failure", app.debugDescription)
        }
        if let home {
            let log = home.appendingPathComponent("Library/Logs/Tenmoku/tenmoku.log")
            if FileManager.default.fileExists(atPath: log.path) {
                let attachment = XCTAttachment(contentsOfFile: log)
                attachment.name = "tenmoku-log-\(shortName)-\(slot)"
                attachment.lifetime = .keepAlways
                add(attachment)
            }
        }
        app?.terminate()
    }

    /// Fails unless the set is as the script made it from the fixture: the project a
    /// byte-for-byte copy, settings.toml there only if declared, setup.toml only if the
    /// state isn't declared fresh.
    func checkFresh(fixtures folder: URL) throws {
        let fm = FileManager.default
        let source = folder.appendingPathComponent("project")
        let files = try relativeFiles(under: source)
        XCTAssertFalse(files.isEmpty, "fixture \(source.path) has no files")
        XCTAssertEqual(try relativeFiles(under: project), files, "scratch project \(project.path) is not a copy of \(source.path)")
        for file in files {
            XCTAssertEqual(
                try Data(contentsOf: project.appendingPathComponent(file)),
                try Data(contentsOf: source.appendingPathComponent(file)),
                "scratch \(file) differs from the fixture's: the set is not fresh")
        }
        let settings = config.appendingPathComponent("settings.toml")
        if let name = fixture.settings {
            XCTAssertEqual(
                try Data(contentsOf: settings),
                try Data(contentsOf: folder.appendingPathComponent("settings/\(name).toml")),
                "settings.toml is not \(name).toml")
        } else {
            XCTAssertFalse(fm.fileExists(atPath: settings.path), "settings.toml in a set that declares none")
        }
        let marker = state.appendingPathComponent("setup.toml")
        if fixture.fresh {
            XCTAssertEqual(try fm.contentsOfDirectory(atPath: state.path), [], "a fresh state folder is not empty")
        } else {
            // The setup wizard shows on a state dir without it (fe-config first_run.rs).
            XCTAssertTrue(fm.fileExists(atPath: marker.path), "no setup.toml in \(state.path)")
        }
    }

    /// The regular files under `root`, as paths relative to it, sorted.
    func relativeFiles(under root: URL) throws -> [String] {
        let fm = FileManager.default
        var files: [String] = []
        guard let walk = fm.enumerator(atPath: root.path) else { return [] }
        while let path = walk.nextObject() as? String {
            var directory: ObjCBool = false
            if fm.fileExists(atPath: root.appendingPathComponent(path).path, isDirectory: &directory),
               !directory.boolValue
            {
                files.append(path)
            }
        }
        return files.sorted()
    }

    // MARK: Launching

    /// "testSaveAllAndSaveAs" from "-[SmokeTests testSaveAllAndSaveAs]".
    var shortName: String {
        let parts = name.split(separator: " ")
        return parts.count == 2 ? String(parts[1].dropLast()) : name
    }

    func file(_ name: String) -> URL { project.appendingPathComponent(name) }

    /// Launches the app on the set's project, opening `files` from it as tabs (the
    /// last is the active one), or on the folder alone when `files` is empty, and waits
    /// for its window.
    func launch(_ files: [String]) {
        // The first folder is the project root, the last path the active tab.
        app.launchArguments = [project.path] + files.map { file($0).path }
        app.launch()
        recordLaunch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: Self.uiTimeout * 2), "no window appeared")
    }

    /// Launches on `files`, waits for `title` and for the tree, then settles, so the
    /// editor has its buffer and focus.
    func launchAndSettle(_ files: [String], title: String) {
        launch(files)
        expectTitle(title)
        waitForTree()
        settle()
    }

    /// The one fixed delay (`editorSettle`): see there.
    func settle() {
        Thread.sleep(forTimeInterval: Self.editorSettle)
    }

    /// Quits through the app menu's Quit Tenmoku and waits for the process to end.
    func quitByMenu() {
        menu(Self.appName, "Quit \(Self.appName)")
        XCTAssertTrue(app.wait(for: .notRunning, timeout: Self.quitTimeout), "the app did not quit by its menu")
    }

    /// Quits with cmd-Q and waits for the process to end.
    func quitByKey() {
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: Self.quitTimeout), "the app did not quit on cmd-Q")
    }

    /// Writes which executable was launched, from NSWorkspace, and fails if it isn't
    /// the bundle under test.
    func recordLaunch() {
        let running = NSWorkspace.shared.runningApplications
            .filter { $0.bundleIdentifier == Self.bundleID }
        let lines = running.map {
            "pid \($0.processIdentifier) executable \($0.executableURL?.path ?? "?") bundle \($0.bundleURL?.path ?? "?")"
        }
        let shipped = NSWorkspace.shared.runningApplications
            .filter { $0.bundleIdentifier == "dev.tenmoku.tenmoku" }
            .map { "shipped id running: pid \($0.processIdentifier) \($0.bundleURL?.path ?? "?")" }
        record("launched", (lines + shipped).joined(separator: "\n"))
        let expected = appURL.resolvingSymlinksInPath().path
        XCTAssertTrue(
            running.contains { $0.bundleURL?.resolvingSymlinksInPath().path == expected },
            "the running \(Self.bundleID) is not \(expected): \(lines)")
    }

    // MARK: gpui-pre's accessibility tree

    /// Waits until gpui-pre's tree is live: it answers the first request empty and
    /// builds a frame later, at a time that varies (T1661), so every check of a gpui
    /// node goes through here first. The marker is a labelled kiln icon button.
    ///
    /// gpui-pre's nodes must be looked up with `button(_:)`, never `.firstMatch`: in
    /// this card's first runs `app.buttons[...].firstMatch` never found Split Right in
    /// 15 s, while `element(boundBy:)`, `element(matching:)`, `count` and the subscript
    /// itself all found it at the same moment (`.firstMatch` reported false). AppKit's
    /// own elements (windows, menus, alerts, panels) are found either way.
    @discardableResult
    func waitForTree(file: StaticString = #filePath, line: UInt = #line) -> Bool {
        let marker = button(Self.treeMarker)
        let live = marker.exists || marker.waitForExistence(timeout: Self.treeTimeout)
        if !live {
            let all = app.buttons.allElementsBoundByIndex.map {
                "title \($0.title.debugDescription) label \($0.label.debugDescription) identifier \($0.identifier.debugDescription)"
            }
            record("ax-tree-no-tree", "app.buttons: \(all.count)\n\(all.joined(separator: "\n"))\n\n\(app.debugDescription)")
        }
        XCTAssertTrue(live, "gpui's accessibility tree never showed \(Self.treeMarker)", file: file, line: line)
        return live
    }

    /// A gpui-drawn button, once the tree is live.
    func gpuiButton(_ title: String, file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        waitForTree(file: file, line: line)
        return button(title)
    }

    /// Fails unless the gpui button `title` shows, enabled or not as asked.
    func expectButton(
        _ title: String, enabled: Bool? = nil, file: StaticString = #filePath, line: UInt = #line
    ) {
        let button = gpuiButton(title, file: file, line: line)
        XCTAssertTrue(
            button.exists || button.waitForExistence(timeout: Self.uiTimeout),
            "no button \(title) in the tree: \(buttonTitles())", file: file, line: line)
        if let enabled {
            let met = holds(NSPredicate(format: "enabled == %@", NSNumber(value: enabled)), of: button)
            XCTAssertTrue(
                met, "button \(title) is \(button.isEnabled ? "enabled" : "disabled"), expected \(enabled ? "enabled" : "disabled")",
                file: file, line: line)
        }
    }

    /// Fails unless the gpui button `title` is gone (or never came), with the tree live.
    func expectNoButton(_ title: String, file: StaticString = #filePath, line: UInt = #line) {
        let button = gpuiButton(title, file: file, line: line)
        XCTAssertTrue(
            !button.exists || button.waitForNonExistence(timeout: Self.uiTimeout),
            "button \(title) is still in the tree: \(buttonTitles())", file: file, line: line)
    }

    /// Fails unless exactly `count` gpui buttons are titled `title`.
    func expectButtonCount(_ title: String, _ count: Int, file: StaticString = #filePath, line: UInt = #line) {
        waitForTree(file: file, line: line)
        let query = buttons(title)
        let met = holds(NSPredicate(format: "count == %d", count), of: query)
        XCTAssertTrue(met, "\(query.count) buttons titled \(title), expected \(count)", file: file, line: line)
    }

    /// Clicks the gpui button `title`, once the tree shows it.
    func clickButton(_ title: String, file: StaticString = #filePath, line: UInt = #line) {
        expectButton(title, file: file, line: line)
        button(title).click()
    }

    /// The buttons named `name`: by title (gpui-pre's nodes carry their label as the AX
    /// title, with no identifier), label or identifier (AppKit's alert buttons).
    func buttons(_ name: String) -> XCUIElementQuery {
        app.buttons.matching(
            NSPredicate(format: "title == %@ OR label == %@ OR identifier == %@", name, name, name))
    }

    /// The first button named `name`, resolved from the whole tree (see `waitForTree`).
    func button(_ name: String) -> XCUIElement {
        buttons(name).element(boundBy: 0)
    }

    func buttonTitles() -> [String] {
        app.buttons.allElementsBoundByIndex.map { $0.title.isEmpty ? $0.identifier : $0.title }
    }

    /// Waits up to `uiTimeout` for `predicate` to hold of `object`; at once if it holds.
    func holds(_ predicate: NSPredicate, of object: Any, timeout: TimeInterval = E2ETestCase.uiTimeout) -> Bool {
        if predicate.evaluate(with: object) { return true }
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: object)
        return XCTWaiter().wait(for: [expectation], timeout: timeout) == .completed
    }

    // MARK: The window, the menus and native panels

    func windowTitles() -> [String] {
        app.windows.allElementsBoundByIndex.map(\.title)
    }

    /// The first window's title: the active tab's name, ` ●` if it has unsaved changes,
    /// then ` — Tenmoku`.
    var title: String { app.windows.firstMatch.title }

    /// "notes.txt — Tenmoku", or "notes.txt ● — Tenmoku" when `dirty`.
    func titled(_ name: String, dirty: Bool = false) -> String {
        dirty ? "\(name) ● — \(Self.appName)" : "\(name) — \(Self.appName)"
    }

    /// Fails unless a window is titled `expected` within `uiTimeout`.
    func expectTitle(_ expected: String, file: StaticString = #filePath, line: UInt = #line) {
        let window = app.windows.matching(NSPredicate(format: "title == %@", expected)).firstMatch
        let found = window.waitForExistence(timeout: Self.uiTimeout)
        record("window-title", "expected \(expected.debugDescription): \(found)\ntitles: \(windowTitles())")
        XCTAssertTrue(found, "no window titled \(expected.debugDescription): \(windowTitles())", file: file, line: line)
    }

    /// Clicks `item` in the menu bar's `menu`.
    func menu(_ menu: String, _ item: String, file: StaticString = #filePath, line: UInt = #line) {
        let bar = app.menuBars.menuBarItems[menu]
        XCTAssertTrue(bar.waitForExistence(timeout: Self.uiTimeout), "no \(menu) menu", file: file, line: line)
        bar.click()
        let entry = bar.menus.menuItems[item]
        let found = entry.waitForExistence(timeout: Self.uiTimeout)
        if !found {
            record("menu-\(menu)", bar.menus.menuItems.allElementsBoundByIndex.map(\.title).joined(separator: "\n"))
        }
        XCTAssertTrue(found, "the \(menu) menu has no \(item)", file: file, line: line)
        XCTAssertTrue(entry.isEnabled, "\(menu) > \(item) is disabled", file: file, line: line)
        entry.click()
    }

    /// The titles of the menu bar's `menu`, opened and closed again.
    func menuTitles(_ menu: String) -> [String] {
        let bar = app.menuBars.menuBarItems[menu]
        XCTAssertTrue(bar.waitForExistence(timeout: Self.uiTimeout), "no \(menu) menu")
        bar.click()
        let items = bar.menus.menuItems
        XCTAssertTrue(items.firstMatch.waitForExistence(timeout: Self.uiTimeout), "the \(menu) menu is empty")
        let titles = items.allElementsBoundByIndex.map(\.title)
        app.typeKey(.escape, modifierFlags: [])
        return titles
    }

    /// Waits for the folder-trust alert a folder launch raises on an undecided folder,
    /// and answers it with `answer` ("Trust Folder" or "Don't Trust").
    func answerTrustPrompt(_ answer: String, file: StaticString = #filePath, line: UInt = #line) {
        let button = nativeButton(answer)
        XCTAssertTrue(
            button.waitForExistence(timeout: Self.uiTimeout * 2), "a folder launch didn't ask about trust",
            file: file, line: line)
        button.click()
    }

    /// A button of a native alert or sheet (AppKit's, not gpui's).
    func nativeButton(_ title: String) -> XCUIElement {
        button(title)
    }

    // MARK: Disk and pasteboard

    /// Polls `condition` every 50 ms for up to `timeout`; true once it holds.
    func eventually(timeout: TimeInterval = E2ETestCase.diskTimeout, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        } while Date() < deadline
        return condition()
    }

    func bytes(_ url: URL) -> Data? { try? Data(contentsOf: url) }

    func text(_ url: URL) -> String? { try? String(contentsOf: url, encoding: .utf8) }

    /// Fails unless `url` holds exactly `expected` within `diskTimeout`.
    func expectBytes(_ url: URL, _ expected: Data, file: StaticString = #filePath, line: UInt = #line) {
        let met = eventually { bytes(url) == expected }
        let last = bytes(url)
        record("disk-\(url.lastPathComponent.replacingOccurrences(of: ".", with: "-"))", "expected \(hex(expected))\nfound    \(last.map(hex) ?? "nothing")")
        XCTAssertTrue(
            met, "\(url.lastPathComponent) is \(last.map(hex) ?? "missing"), expected \(hex(expected))",
            file: file, line: line)
    }

    /// Fails unless `url` holds exactly `expected` as UTF-8 within `diskTimeout`.
    func expectText(_ url: URL, _ expected: String, file: StaticString = #filePath, line: UInt = #line) {
        expectBytes(url, Data(expected.utf8), file: file, line: line)
    }

    func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined(separator: " ")
    }

    /// `url`'s path with every symlink resolved, as Rust's canonicalize gives it
    /// (Foundation's resolvingSymlinksInPath drops /private, which this keeps).
    func canonical(_ url: URL) -> String {
        guard let resolved = realpath(url.path, nil) else { return url.path }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    /// The general pasteboard's string, if any.
    var pasteboard: String? { NSPasteboard.general.string(forType: .string) }

    func setPasteboard(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    /// Saves the general pasteboard's items now and puts them back when the test ends,
    /// so a test that writes it leaves the user's clipboard as it found it.
    func preservePasteboard() {
        let saved: [[NSPasteboard.PasteboardType: Data]] = (NSPasteboard.general.pasteboardItems ?? []).map { item in
            var types: [NSPasteboard.PasteboardType: Data] = [:]
            for type in item.types {
                if let data = item.data(forType: type) { types[type] = data }
            }
            return types
        }
        addTeardownBlock {
            NSPasteboard.general.clearContents()
            let items = saved.map { types -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (type, data) in types { item.setData(data, forType: type) }
                return item
            }
            if !items.isEmpty { NSPasteboard.general.writeObjects(items) }
        }
    }

    // MARK: Records

    /// An attachment named <kind>-<test>-<slot>; the script copies it to its results
    /// folder as <kind>-<test>-<slot>.txt.
    func record(_ kind: String, _ text: String) {
        let attachment = XCTAttachment(string: text)
        attachment.name = "\(kind)-\(shortName)-\(slot)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
