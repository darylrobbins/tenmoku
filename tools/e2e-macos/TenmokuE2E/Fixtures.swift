// Which fixture each test runs on (T1682).
//
// scripts/e2e-macos.sh reads the table below with sed, so each entry stays on one line
// in exactly this form:
//     "testName": Fixture("folder"),
//     "testName": Fixture("folder", settings: "name"),
//     "testName": Fixture("folder", fresh: true),
// and every test method has exactly one entry (the script refuses to run otherwise).
//
// The script makes the scratch sets from the fixtures, one per test and iteration, before
// the run (the sandboxed runner may write only this run's scratch folder): <E2E_SCRATCH>/<test>/<iteration>/ holds
//   project/   a copy of tools/e2e-macos/fixtures/<folder>/project/
//   config/    empty, or with settings.toml copied from <folder>/settings/<name>.toml
//   state/     with setup.toml (`outcome = "skipped"`, so no setup wizard), or empty
//              when the entry says `fresh: true` (a first launch: the wizard shows)
//   home/      the app's HOME, so its log lands in the set

struct Fixture {
    /// A folder under tools/e2e-macos/fixtures/.
    let folder: String
    /// A file under <folder>/settings/, without `.toml`, pre-seeded as settings.toml.
    let settings: String?
    /// The state folder starts empty: no setup.toml.
    let fresh: Bool

    init(_ folder: String, settings: String? = nil, fresh: Bool = false) {
        self.folder = folder
        self.settings = settings
        self.fresh = fresh
    }
}

enum Fixtures {
    static let declared: [String: Fixture] = [
        "testSaveWithCmdSAndFileMenu": Fixture("plain"),
        "testSaveAllAndSaveAs": Fixture("two-files"),
        "testSaveEncodingsByteForByte": Fixture("encodings"),
        "testWindowTitleDirtyAndActive": Fixture("two-files"),
        "testQuitByMenuHotExitAndRelaunch": Fixture("plain"),
        "testFirstRunWizardOnce": Fixture("plain", fresh: true),
        "testTrustByMenu": Fixture("plain"),
        "testOpenRecentAndRecentToml": Fixture("recent"),
        "testVimClipboardOff": Fixture("vim", settings: "off"),
        "testVimClipboardUnnamedplus": Fixture("vim", settings: "unnamedplus"),
        "testMarkdownPreviewButtons": Fixture("markdown"),
        "testPaneToolbarEnabledState": Fixture("lines"),
        "testRunnerWritesOnlyItsScratch": Fixture("plain"),
    ]
}
