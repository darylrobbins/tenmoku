---
task_uid: "01m7c2mjmzk1nt5zmghvp7r8wv"
task: "T379"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "task_defs_dotnet_tests: root .sln gives build/test/clean/restore with the solution path; .sln in src/ is found; .slnx detected; root .csproj without a solution adds dotnet run --project; no .NET files gives none; tasks are argv lists; listed alongside package.json tasks"
    required: true
    manual: false
  - id: "C2"
    text: "task_defs's existing tests pass and the module doc names .NET and the one-level solution lookup"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench task_defs passes"
    required: true
    manual: false
---

# Detect .NET tasks in the task list: dotnet build, test, clean and restore per .sln or .slnx (root or one folder down) or per root .csproj, and dotnet run per project (Mac T1830)

User report: in a repo with a .NET solution and a package.json, the Tasks menu shows only the yarn scripts, no dotnet build options.

crates/fe-workbench/src/task_defs.rs `detected_tasks` (~480) detects Cargo.toml, package.json, Gradle, Maven, Makefile and justfile, root only. There is no .NET source. T1660 (Gradle and Maven) is the template: copy its `Source`, ordering and default-group rules.

Add a Dotnet source after Maven:
- A .sln or .slnx at the root, or else in one immediate subfolder (src/ is common): `dotnet build <sln>`, `dotnet test <sln>`, `dotnet clean <sln>`, `dotnet restore <sln>`, run from the root with the solution path relative to it. Several solutions: one set per solution, labelled by file name.
- No solution but .csproj (or .fsproj, .vbproj) files at the root: the same four per project, plus `dotnet run --project <proj>`. With a solution, add `dotnet run --project <proj>` only for projects whose file says `<OutputType>Exe</OutputType>` or `WinExe` if that is cheap to read; otherwise leave run out and say so.
- Build tasks go in the Build group, test in the Test group, as T1660 does.
- Tasks are argv lists, never a shell string.
- Change the module doc's "no recursion" rule to name the one-level .sln lookup, and add .NET to its Detection list.
- Listed alongside package.json tasks, never instead of them.

Tests in a new crates/fe-workbench/src/task_defs_dotnet_tests.rs declared next to task_defs_java_tests.
