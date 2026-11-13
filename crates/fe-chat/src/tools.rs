//! The API chat's tool set (the agents record's § 6): five client tools,
//! their schemas and descriptions, the typed inputs a call is validated
//! into, and the text of the results Tenmoku answers for calls it doesn't
//! run.
//!
//! Everything here is constant: no per-workspace content, so the tool set
//! a chat freezes at its first send is the same bytes in every request.
//! All five are `strict` (every object has `additionalProperties: false`
//! and lists every property in `required`, `null` meaning "not given") and
//! stream their input eagerly, so an input is validated here, at the end
//! of its block, before anything runs.
//!
//! JSON built here is written with its keys in sorted order, so it
//! serialises the same whether or not another crate turns on serde_json's
//! `preserve_order`.

use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::provider::ToolSpec;

/// `list_files`: the project's files under a folder.
pub const LIST_FILES: &str = "list_files";
/// `propose_edit`: a proposed change to one file, for the user to review.
pub const PROPOSE_EDIT: &str = "propose_edit";
/// `read_file`: a file's text, or some of its lines.
pub const READ_FILE: &str = "read_file";
/// `run_command`: a program run in the project folder, without a shell.
pub const RUN_COMMAND: &str = "run_command";
/// `search`: lines of the project's files matching a pattern.
pub const SEARCH: &str = "search";

/// The smallest task budget the API takes, in tokens.
pub const MIN_TASK_BUDGET: u32 = 20_000;

const LIST_FILES_DESCRIPTION: &str = "\
Lists the files of the project under a folder, as paths relative to the \
project root, one per line. Ignored files and files the user keeps from \
the assistant are left out. At most 1000 paths are listed, then a line \
\"[truncated: N more]\". `dir` is a folder relative to the root, or null \
for the whole project; `glob` keeps only the paths matching a glob such as \
\"**/*.rs\", or is null for all.";

const PROPOSE_EDIT_DESCRIPTION: &str = "\
Proposes a change to one file of the project, which the user reviews and \
accepts or rejects. Nothing is written until they accept. `path` is the \
file relative to the project root. `edits` is a non-empty list of \
replacements: each `old_text` must match the file exactly once, whitespace \
included, as you last read it with your earlier proposals applied, and no \
two may overlap. A single edit whose `old_text` is \"\" replaces the whole \
file, or creates it when it doesn't exist. A later proposal to the same \
file replaces your earlier one's pending changes, so propose the file's \
whole intended change from what you read. The result says how many changes \
were proposed, or why none were.";

const READ_FILE_DESCRIPTION: &str = "\
Reads a text file of the project: the open editor's text when the file is \
open, unsaved edits included, else the file on disk, with your pending \
proposed changes applied. `path` is relative to the project root. \
`start_line` and `end_line` are 1-based and inclusive, or null for the \
file's start and end. At most 2000 lines or 64 KiB are returned, then a \
line \"[truncated: N more lines; read from line M]\". Binary files and \
files over 1 MiB are refused. A first line \"[includes N pending changes \
you proposed]\" or \"[M of your earlier changes no longer apply]\" says how \
your proposals stand.";

const RUN_COMMAND_DESCRIPTION: &str = "\
Runs a program in the project folder, or in `cwd`, a folder inside it \
relative to the root (null for the root), and returns its output. \
`program` is the program's name, such as \"cargo\", and `args` its \
arguments, each passed as is: there is no shell, so no pipes, redirection, \
globbing or variables. The command can change the user's files, and the \
user may be asked to allow it first. Output is stdout and stderr \
interleaved, at most 32 KiB: the first 8 KiB and the last 24 KiB, with \
\"[N bytes omitted]\" between, then \"exit N\", or \"killed after T s\" \
when it ran too long.";

const SEARCH_DESCRIPTION: &str = "\
Searches the project's files for lines matching `pattern`, a regular \
expression when `regex` is true, else literal text, matched with case when \
`case_sensitive` is true. `glob` keeps only the files matching a glob such \
as \"src/**/*.rs\", or is null for all. Ignored files and files the user \
keeps from the assistant are skipped. Each match is a line \"path:line: \
text\", the text cut to 512 bytes. At most 200 lines or 64 KiB are \
returned, then a line \"[truncated]\".";

/// The chat's five tools, sorted by name, each `strict` and streaming its
/// input eagerly.
pub fn tool_set() -> Vec<ToolSpec> {
    let tool = |name: &str, description: &str, input_schema: Value| ToolSpec {
        name: name.to_owned(),
        description: description.to_owned(),
        input_schema,
        strict: true,
        eager_input_streaming: true,
    };
    let string = || json!({"type": "string"});
    let string_or_null = || json!({"type": ["string", "null"]});
    let integer_or_null = || json!({"type": ["integer", "null"]});
    let boolean = || json!({"type": "boolean"});
    vec![
        tool(
            LIST_FILES,
            LIST_FILES_DESCRIPTION,
            object([("dir", string_or_null()), ("glob", string_or_null())]),
        ),
        tool(
            PROPOSE_EDIT,
            PROPOSE_EDIT_DESCRIPTION,
            object([
                (
                    "edits",
                    json!({
                        "items": object([("new_text", string()), ("old_text", string())]),
                        "type": "array",
                    }),
                ),
                ("path", string()),
            ]),
        ),
        tool(
            READ_FILE,
            READ_FILE_DESCRIPTION,
            object([
                ("end_line", integer_or_null()),
                ("path", string()),
                ("start_line", integer_or_null()),
            ]),
        ),
        tool(
            RUN_COMMAND,
            RUN_COMMAND_DESCRIPTION,
            object([
                ("args", json!({"items": string(), "type": "array"})),
                ("cwd", string_or_null()),
                ("program", string()),
            ]),
        ),
        tool(
            SEARCH,
            SEARCH_DESCRIPTION,
            object([
                ("case_sensitive", boolean()),
                ("glob", string_or_null()),
                ("pattern", string()),
                ("regex", boolean()),
            ]),
        ),
    ]
}

/// A strict object schema of `properties`, given in sorted order: no other
/// properties, and every one required.
fn object<const N: usize>(properties: [(&str, Value); N]) -> Value {
    let required: Vec<&str> = properties.iter().map(|(name, _)| *name).collect();
    let properties: serde_json::Map<String, Value> = properties
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    json!({
        "additionalProperties": false,
        "properties": properties,
        "required": required,
        "type": "object",
    })
}

/// A call's input, validated: what the executors take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolInput {
    ListFiles(ListFilesInput),
    ProposeEdit(ProposeEditInput),
    ReadFile(ReadFileInput),
    RunCommand(RunCommandInput),
    Search(SearchInput),
}

/// `list_files`'s input.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListFilesInput {
    #[serde(deserialize_with = "present")]
    pub dir: Option<String>,
    #[serde(deserialize_with = "present")]
    pub glob: Option<String>,
}

/// `propose_edit`'s input. `edits` is never empty.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeEditInput {
    pub path: String,
    pub edits: Vec<ProposedEdit>,
}

/// One replacement of a [`ProposeEditInput`]; `old_text` empty means the
/// whole file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedEdit {
    pub old_text: String,
    pub new_text: String,
}

/// `read_file`'s input; lines are 1-based and inclusive.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadFileInput {
    pub path: String,
    #[serde(deserialize_with = "present")]
    pub start_line: Option<u64>,
    #[serde(deserialize_with = "present")]
    pub end_line: Option<u64>,
}

/// `run_command`'s input: an argv, never a shell string.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommandInput {
    pub program: String,
    pub args: Vec<String>,
    #[serde(deserialize_with = "present")]
    pub cwd: Option<String>,
}

/// `search`'s input.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    #[serde(deserialize_with = "present")]
    pub glob: Option<String>,
}

/// An optional field that must still be given, as `null` when it has no
/// value, as a strict schema requires: with `deserialize_with`, serde
/// treats a missing field as an error rather than `None`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

/// Why a call's input didn't validate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputError {
    /// The call names a tool that isn't one of the five.
    UnknownTool(String),
    /// The input isn't the tool's: not JSON, not an object, a field
    /// missing, unknown or of the wrong type, or no edits.
    Invalid(String),
}

impl ToolInput {
    /// Validates `input`, a `tool_use` block's input, as `name`'s: strictly,
    /// as objects (never arrays of the fields), with no unknown or missing
    /// fields. Input that wasn't JSON reaches here as the raw string, which
    /// is never valid.
    pub fn parse(name: &str, input: &Value) -> Result<ToolInput, InputError> {
        // A derived struct also takes a sequence of its fields in order;
        // the strict schemas take objects only.
        fn typed<T: for<'de> Deserialize<'de>>(input: &Value) -> Result<T, InputError> {
            if !input.is_object() {
                return Err(InputError::Invalid("the input isn't an object".to_owned()));
            }
            T::deserialize(input).map_err(|error| InputError::Invalid(error.to_string()))
        }
        match name {
            LIST_FILES => typed(input).map(ToolInput::ListFiles),
            PROPOSE_EDIT => {
                let edits = input.get("edits").and_then(Value::as_array);
                if edits.is_some_and(|edits| !edits.iter().all(Value::is_object)) {
                    return Err(InputError::Invalid("an edit isn't an object".to_owned()));
                }
                let input: ProposeEditInput = typed(input)?;
                if input.edits.is_empty() {
                    return Err(InputError::Invalid("`edits` is empty".to_owned()));
                }
                Ok(ToolInput::ProposeEdit(input))
            }
            READ_FILE => typed(input).map(ToolInput::ReadFile),
            RUN_COMMAND => typed(input).map(ToolInput::RunCommand),
            SEARCH => typed(input).map(ToolInput::Search),
            other => Err(InputError::UnknownTool(other.to_owned())),
        }
    }
}

/// Whether a `tool_use` block's input can be committed to the history: an
/// object, and for one of the five tools, valid as its input. A call to a
/// tool that isn't one of the five is committed (and answered as an
/// error by the loop) when its input is an object, since it can be echoed.
pub fn input_is_valid(name: &str, input: &Value) -> bool {
    match ToolInput::parse(name, input) {
        Ok(_) => true,
        Err(InputError::UnknownTool(_)) => input.is_object(),
        Err(InputError::Invalid(_)) => false,
    }
}

/// Why a call of a reply was answered without running: each is an
/// `is_error` result with its [`text`](NotRun::text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotRun {
    /// The user stopped the turn before the call finished.
    Stopped,
    /// The folder's trust was withdrawn, or its root changed, mid-turn.
    TrustWithdrawn,
    /// The reply would have started a round past the turn's limit.
    RoundLimit,
    /// The turn's wall time ran out before the call ran.
    TimeLimit,
    /// The reply was cut off at the token limit (`max_tokens`).
    CutOff,
    /// The reply ended without waiting for results (a stop reason other
    /// than `tool_use`), or the chat has no tools.
    Ended,
}

impl NotRun {
    /// The result's text, as the model reads it.
    pub fn text(self) -> &'static str {
        match self {
            NotRun::Stopped => "Stopped by the user",
            NotRun::TrustWithdrawn => "Folder trust was withdrawn",
            NotRun::RoundLimit => "The round limit was reached; the user can continue",
            NotRun::TimeLimit => "The time limit was reached; the user can continue",
            NotRun::CutOff => "Cut off at the token limit",
            NotRun::Ended => "Not run: the reply ended without waiting for results",
        }
    }
}
