// XCUITest spike (T1661): launch a bundled Tenmoku.app on a scratch project, type,
// save with cmd-S and with File > Save, and dump the accessibility tree XCUITest sees.
//
// Run through scripts/e2e-macos.sh, which bundles the app and passes, as
// TEST_RUNNER_<NAME> variables that the runner sees as <NAME>:
//   E2E_APP      the Tenmoku.app under test (re-identified as dev.tenmoku.tenmoku.e2e)
//   E2E_SCRATCH  a pool of scratch sets, <E2E_SCRATCH>/001 to /<E2E_POOL>, each with
//                project/notes.txt (empty), config/, state/setup.toml and home/
//   E2E_POOL     how many sets the pool has
//
// Xcode 27's UI-test runner is sandboxed: it may read anywhere but write only in its
// container. So the script makes the scratch sets, each test claims the next one with
// a counter in the runner's own defaults, and everything recorded (AX dumps, launch
// records) is an attachment, which the script exports and copies to its results folder.
//
// The app is launched by URL, never by bundle id: the installed copies in
// /Applications and ~/Applications share the shipped id.

import AppKit
import XCTest

final class SmokeTests: XCTestCase {
    /// The bundle id scripts/e2e-macos.sh gives the bundle under test.
    static let bundleID = "dev.tenmoku.tenmoku.e2e"
    /// The one fixed settle delay. The window and its title are up before the
    /// workbench has read notes.txt and focused its editor, and nothing in the
    /// accessibility tree says when that has happened, so there is nothing to
    /// wait on. Keys sent earlier can land before the editor has focus.
    static let editorSettle: TimeInterval = 1.0
    /// How long the file on disk may take to show a save.
    static let diskTimeout: TimeInterval = 5.0

    var app: XCUIApplication!
    var appURL: URL!
    /// The claimed scratch set's number, "001" and so on.
    var slot = ""
    var project: URL!
    var notes: URL!
    var home: URL!

    override func setUpWithError() throws {
        continueAfterFailure = false
        let env = ProcessInfo.processInfo.environment
        guard let appPath = env["E2E_APP"], let scratch = env["E2E_SCRATCH"],
              let pool = env["E2E_POOL"].flatMap(Int.init)
        else {
            XCTFail("E2E_APP, E2E_SCRATCH and E2E_POOL must be set: run scripts/e2e-macos.sh")
            return
        }
        appURL = URL(fileURLWithPath: appPath)
        // The next unused set. The runner is relaunched between iterations, so
        // the count lives in its defaults, keyed by this run's pool.
        let defaults = UserDefaults.standard
        let key = "claimed:\(scratch)"
        let index = defaults.integer(forKey: key) + 1
        defaults.set(index, forKey: key)
        defaults.synchronize()
        guard index <= pool else {
            XCTFail("scratch pool \(scratch) has \(pool) sets, all used")
            return
        }
        slot = String(format: "%03d", index)
        let run = URL(fileURLWithPath: scratch).appendingPathComponent(slot)
        project = run.appendingPathComponent("project")
        home = run.appendingPathComponent("home")
        let config = run.appendingPathComponent("config")
        let state = run.appendingPathComponent("state")
        notes = project.appendingPathComponent("notes.txt")
        let fm = FileManager.default
        XCTAssertEqual(
            try? String(contentsOf: notes, encoding: .utf8), "",
            "scratch set \(slot) is not fresh: notes.txt should exist and be empty")
        // The setup wizard shows on an empty state dir (fe-config first_run.rs).
        XCTAssertTrue(fm.fileExists(atPath: state.appendingPathComponent("setup.toml").path))

        app = XCUIApplication(url: appURL)
        // The first folder is the project root, the last path the active tab.
        app.launchArguments = [project.path, notes.path]
        app.launchEnvironment = [
            "TENMOKU_CONFIG_DIR": config.path,
            "TENMOKU_STATE_DIR": state.path,
            "TENMOKU_NO_IPC": "1",
            // The macOS log goes to $HOME/Library/Logs/Tenmoku/tenmoku.log.
            "HOME": home.path,
        ]
        record("scratch", run.path)
    }

    override func tearDownWithError() throws {
        if let run = testRun, run.failureCount > 0, let app {
            let shot = XCTAttachment(screenshot: app.screenshot())
            shot.name = "failure-\(shortName)-\(slot)"
            shot.lifetime = .keepAlways
            add(shot)
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

    // MARK: Tests

    func testTypeAndSaveWithCmdS() throws {
        let window = try launchAndWait()
        _ = window
        let text = "hello from xcuitest"
        app.typeText(text)
        app.typeKey("s", modifierFlags: .command)
        try expectOnDisk(text)
    }

    func testMenuFileSave() throws {
        _ = try launchAndWait()
        let text = "saved from the File menu"
        app.typeText(text)

        let file = app.menuBars.menuBarItems["File"]
        XCTAssertTrue(file.waitForExistence(timeout: 5), "no File menu in the menu bar")
        file.click()
        let items = file.menus.menuItems
        XCTAssertTrue(items["Save"].waitForExistence(timeout: 5), "File menu has no Save")
        let titles = items.allElementsBoundByIndex.map(\.title)
        record("file-menu", titles.joined(separator: "\n"))
        XCTAssertTrue(titles.contains("Save"), "File menu items: \(titles)")
        XCTAssertTrue(titles.contains("New File"), "File menu items: \(titles)")
        items["Save"].click()
        try expectOnDisk(text)
    }

    func testDumpAccessibilityTree() throws {
        app.launch()
        recordLaunch()
        // Right after launch, before any query of the test's own. launch() itself
        // waits for the app to settle, and may query it while it does.
        dump("1-launch")

        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 20), "no window appeared")
        let count = app.descendants(matching: .any).count
        dump("2-first-query", note: "descendants of the app after the first query: \(count)")

        waitForNotesTitle()
        Thread.sleep(forTimeInterval: Self.editorSettle)
        app.typeText("typed for the tree dump")
        dump("3-typed")

        // Ten seconds idle: does a late assistive-technology activation build a tree?
        Thread.sleep(forTimeInterval: 10)
        dump("4-idle-10s")
    }

    // MARK: Helpers

    /// "testTypeAndSaveWithCmdS" from "-[SmokeTests testTypeAndSaveWithCmdS]".
    var shortName: String {
        let parts = name.split(separator: " ")
        return parts.count == 2 ? String(parts[1].dropLast()) : name
    }

    /// Launches the app, waits for its window, then for notes.txt to be the
    /// active tab, then settles. Returns the window.
    @discardableResult
    func launchAndWait() throws -> XCUIElement {
        app.launch()
        recordLaunch()
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 20), "no window appeared")
        waitForNotesTitle()
        Thread.sleep(forTimeInterval: Self.editorSettle)
        return window
    }

    /// The workbench titles its window after the active tab. Recorded, not
    /// asserted: the spike is to find out what the title says.
    func waitForNotesTitle() {
        let titled = app.windows.matching(NSPredicate(format: "title CONTAINS %@", "notes.txt"))
            .firstMatch
        let found = titled.waitForExistence(timeout: 10)
        let titles = app.windows.allElementsBoundByIndex.map(\.title)
        record("window-title", "notes.txt in a window title: \(found)\ntitles: \(titles)")
    }

    /// Polls notes.txt until it holds `expected`, for up to `diskTimeout`.
    func expectOnDisk(_ expected: String) throws {
        let deadline = Date().addingTimeInterval(Self.diskTimeout)
        var last = ""
        repeat {
            last = (try? String(contentsOf: notes, encoding: .utf8)) ?? ""
            if last == expected { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        } while Date() < deadline
        XCTFail("notes.txt is \(last.debugDescription), expected \(expected.debugDescription)")
    }

    /// Writes which executable was launched, from NSWorkspace, and fails if it
    /// isn't the bundle under test.
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

    /// Records `app.debugDescription` and then a count of the window's descendants by
    /// type, as ax-tree-<stage>-<test>-<slot>. The snapshot comes first: the count's
    /// own queries could activate gpui-pre's tree, and the count is taken after it, so
    /// the two can differ when the tree arrives in between.
    func dump(_ stage: String, note: String = "") {
        let snapshot = app.debugDescription
        var text = "stage: \(stage)\n"
        if !note.isEmpty { text += note + "\n" }
        let window = app.windows.firstMatch
        if window.exists {
            var counts: [String: Int] = [:]
            for element in window.descendants(matching: .any).allElementsBoundByIndex {
                counts[typeName(element.elementType), default: 0] += 1
            }
            text += "below the first window, by type (counted after the snapshot): "
            text += counts.isEmpty
                ? "none\n"
                : counts.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }
                    .joined(separator: " ") + "\n"
        } else {
            text += "no window after the snapshot\n"
        }
        text += "\n" + snapshot
        record("ax-tree-\(stage)", text)
    }

    /// An attachment named <kind>-<test>-<slot>; the script copies it to its
    /// results folder as <kind>-<test>-<slot>.txt.
    func record(_ kind: String, _ text: String) {
        let attachment = XCTAttachment(string: text)
        attachment.name = "\(kind)-\(shortName)-\(slot)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func typeName(_ type: XCUIElement.ElementType) -> String {
        let names: [XCUIElement.ElementType: String] = [
            .any: "any", .other: "other", .application: "application", .group: "group",
            .window: "window", .sheet: "sheet", .drawer: "drawer", .alert: "alert",
            .dialog: "dialog", .button: "button", .radioButton: "radioButton",
            .radioGroup: "radioGroup", .checkBox: "checkBox", .disclosureTriangle:
                "disclosureTriangle", .popUpButton: "popUpButton", .comboBox: "comboBox",
            .menuButton: "menuButton", .toolbarButton: "toolbarButton", .popover: "popover",
            .keyboard: "keyboard", .link: "link", .image: "image", .icon: "icon",
            .searchField: "searchField", .scrollView: "scrollView", .scrollBar: "scrollBar",
            .staticText: "staticText", .textField: "textField", .secureTextField:
                "secureTextField", .textView: "textView", .menu: "menu", .menuItem: "menuItem",
            .menuBar: "menuBar", .menuBarItem: "menuBarItem", .map: "map", .webView: "webView",
            .toolbar: "toolbar", .statusBar: "statusBar", .table: "table", .tableRow: "tableRow",
            .tableColumn: "tableColumn", .outline: "outline", .outlineRow: "outlineRow",
            .browser: "browser", .collectionView: "collectionView", .slider: "slider",
            .valueIndicator: "valueIndicator", .splitGroup: "splitGroup", .splitter: "splitter",
            .layoutArea: "layoutArea", .layoutItem: "layoutItem", .tab: "tab", .tabGroup: "tabGroup",
            .cell: "cell", .handle: "handle", .progressIndicator: "progressIndicator",
            .ruler: "ruler", .rulerMarker: "rulerMarker", .grid: "grid", .levelIndicator:
                "levelIndicator", .matte: "matte", .colorWell: "colorWell",
        ]
        return names[type] ?? "type\(type.rawValue)"
    }
}
