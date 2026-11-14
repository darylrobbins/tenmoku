//! .NET detection over a fake [`ProjectFiles`].

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use crate::task_defs::{Group, ProjectFiles, Source, Task, TaskCommand, load};

const ROOT: &str = "/project";

struct Files(BTreeSet<PathBuf>, String);

impl Files {
    fn with(names: &[&str]) -> Self {
        Self(
            names
                .iter()
                .map(|name| Path::new(ROOT).join(name))
                .collect(),
            String::new(),
        )
    }

    fn tasks(&self) -> Vec<Task> {
        load(self, Path::new(ROOT)).0
    }
}

impl ProjectFiles for Files {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if self.0.contains(path) {
            if path.ends_with("package.json") {
                return Ok(self.1.clone());
            }
            Ok(String::new())
        } else {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.0.contains(path)
    }

    fn list_dir(&self, dir: &Path) -> Vec<PathBuf> {
        let mut out = BTreeSet::new();
        for file in &self.0 {
            let Ok(rest) = file.strip_prefix(dir) else {
                continue;
            };
            let mut parts = rest.components();
            if let Some(first) = parts.next() {
                out.insert(dir.join(first));
            }
        }
        out.into_iter().collect()
    }
}

fn dotnet(tasks: &[Task]) -> Vec<(Vec<&str>, Option<Group>)> {
    tasks
        .iter()
        .filter(|task| task.source == Source::Dotnet)
        .map(|task| match &task.command {
            TaskCommand::Argv(words) => (words.iter().map(String::as_str).collect(), task.group),
            other => panic!("not an argument list: {other:?}"),
        })
        .collect()
}

fn four(path: &str) -> Vec<(Vec<&str>, Option<Group>)> {
    vec![
        (vec!["dotnet", "build", path], Some(Group::Build)),
        (vec!["dotnet", "test", path], Some(Group::Test)),
        (vec!["dotnet", "clean", path], None),
        (vec!["dotnet", "restore", path], None),
    ]
}

#[test]
fn root_sln_gives_four_tasks() {
    let tasks = Files::with(&["App.sln", "App/App.csproj"]).tasks();
    assert_eq!(dotnet(&tasks), four("App.sln"));
}

#[test]
fn sln_in_src_is_found() {
    let tasks = Files::with(&["src/App.sln", "src/App/App.csproj"]).tasks();
    assert_eq!(dotnet(&tasks), four("src/App.sln"));
}

#[test]
fn slnx_is_detected() {
    let tasks = Files::with(&["App.slnx"]).tasks();
    assert_eq!(dotnet(&tasks), four("App.slnx"));
}

#[test]
fn root_csproj_without_solution_adds_run() {
    let tasks = Files::with(&["App.csproj"]).tasks();
    let mut want = four("App.csproj");
    want.push((vec!["dotnet", "run", "--project", "App.csproj"], None));
    assert_eq!(dotnet(&tasks), want);
}

#[test]
fn no_dotnet_files_gives_none() {
    let tasks = Files::with(&["README.md", "src/main.rs", "src/notes.txt"]).tasks();
    assert!(dotnet(&tasks).is_empty());
}

#[test]
fn solutions_in_sibling_folders_each_get_a_set() {
    let tasks = Files::with(&["client/Client.sln", "server/Server.slnx"]).tasks();
    let mut want = four("client/Client.sln");
    want.extend(four("server/Server.slnx"));
    assert_eq!(dotnet(&tasks), want);
}

#[test]
fn listed_alongside_package_json_tasks() {
    let mut files = Files::with(&["App.sln", "package.json"]);
    files.1 = r#"{"scripts":{"dev":"vite"}}"#.to_owned();
    let tasks = files.tasks();
    assert_eq!(dotnet(&tasks), four("App.sln"));
    let npm: Vec<_> = tasks
        .iter()
        .filter(|task| task.source == Source::Npm)
        .collect();
    assert_eq!(npm.len(), 1);
}
