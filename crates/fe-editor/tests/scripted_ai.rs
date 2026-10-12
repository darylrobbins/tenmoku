//! The scripted fake AI: its script grammar, and a whole session playing
//! every AI seam together (the M8 exit criterion).

use std::{collections::HashSet, mem::discriminant, time::Duration};

use fe_buffer::{Buffer, HunkStatus, ProposalError, StreamError};
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, Tint},
    scripted_ai::{self, DEMO_SCRIPT, DEMO_TEXT, ScriptedAi, Step, StepError},
};

fn model(text: &str) -> (EditorModel, ScriptedAi) {
    let mut model = EditorModel::new(Buffer::new(text), None);
    let ai = ScriptedAi::new(&mut model);
    (model, ai)
}

/// Runs `script`, every step of which must succeed.
fn play(model: &mut EditorModel, ai: &mut ScriptedAi, script: &str) {
    for step in scripted_ai::parse(script).unwrap() {
        if let Err(error) = ai.step(model, &step) {
            panic!("{step:?}: {error}");
        }
    }
}

fn text(model: &EditorModel) -> String {
    model.text().to_string()
}

fn touched(model: &EditorModel) -> Vec<Decoration> {
    model.decorations(0..model.text().len())
}

/// The first and last row each covers.
fn rows(model: &EditorModel, decorations: &[Decoration]) -> Vec<(u32, u32)> {
    (decorations.iter())
        .map(|decoration| decoration.covered_rows(model.text()).into_inner())
        .collect()
}

#[test]
fn parses_every_verb() {
    let script = "\
# a comment

wait 250
caret 3:7
type a\\nb\\tc\\\\d
suggest  two words
accept
accept-word
accept-line
dismiss
   # an indented comment
stream
push x\\n
finish
propose 2-4 new\\n
accept-hunk 1
reject-hunk 2
undo
redo
undo-ai
redo-ai
";
    let steps = scripted_ai::parse(script).unwrap();
    assert_eq!(
        steps,
        [
            Step::Wait(Duration::from_millis(250)),
            Step::Caret { row: 3, col: 7 },
            Step::Type("a\nb\tc\\d".into()),
            Step::Suggest(" two words".into()),
            Step::Accept,
            Step::AcceptWord,
            Step::AcceptLine,
            Step::Dismiss,
            Step::Stream,
            Step::Push("x\n".into()),
            Step::Finish,
            Step::Propose {
                first: 2,
                last: 4,
                text: "new\n".into(),
            },
            Step::AcceptHunk(1),
            Step::RejectHunk(2),
            Step::Undo,
            Step::Redo,
            Step::UndoAi,
            Step::RedoAi,
        ]
    );

    for (script, line, message) in [
        ("undo\n\nfly 3", 3, "unknown verb `fly`"),
        ("wait", 1, "`wait` needs <ms>"),
        ("# c\nwait soon", 2, "<ms> must be a number, got `soon`"),
        ("type", 1, "`type` needs <text>"),
        ("accept-hunk", 1, "`accept-hunk` needs <n>"),
        ("accept-hunk x", 1, "<n> must be a number, got `x`"),
        ("undo now", 1, "`undo` takes no argument"),
        ("caret 3", 1, "expected <row>:<col>, got `3`"),
        ("caret a:1", 1, "<row> must be a number, got `a`"),
        ("caret 1:", 1, "<col> must be a number, got ``"),
        ("caret 0:1", 1, "<row> counts from 1, got 0"),
        ("propose 4-2 x", 1, "row range 4-2 ends before it starts"),
        ("propose 2 x", 1, "expected <first>-<last>, got `2`"),
        ("type a\\qb", 1, "unknown escape `\\q`"),
    ] {
        let error = scripted_ai::parse(script).unwrap_err();
        assert_eq!(
            (error.line, error.message.as_str()),
            (line, message),
            "{script}"
        );
        assert_eq!(error.to_string(), format!("line {line}: {message}"));
    }

    let demo = scripted_ai::parse(DEMO_SCRIPT).unwrap();
    let verbs: HashSet<_> = demo.iter().map(discriminant).collect();
    let all: HashSet<_> = steps.iter().map(discriminant).collect();
    assert_eq!(verbs, all, "the demo uses every verb");
}

/// The M8 exit test: an AI transaction applied during simulated typing
/// rebases its anchors, through the whole demo session.
#[test]
fn demo_session_rebases_anchors() {
    let (mut model, mut ai) = model(DEMO_TEXT);
    // What the open stream has pushed, and what the suggestion has left
    // to show.
    let mut pushed = String::new();
    let mut rest: Option<String> = None;
    let mut typed_before_anchor = false;
    let mut shown_again = false;
    for step in scripted_ai::parse(DEMO_SCRIPT).unwrap() {
        let live_before = ai
            .ghost_text()
            .and_then(|ghost| ghost.remaining(&model))
            .is_some();
        let report = ai.step(&mut model, &step);
        assert!(report.is_ok(), "{step:?}: {report:?}");
        let live = (ai.ghost_text())
            .and_then(|ghost| ghost.remaining(&model))
            .map(str::to_owned);
        match &step {
            Step::Stream => pushed.clear(),
            Step::Push(chunk) => pushed.push_str(chunk),
            Step::Suggest(text) => rest = Some(text.clone()),
            Step::Type(typed) if live_before => {
                let left = rest.as_deref().unwrap().strip_prefix(typed.as_str());
                rest = Some(left.expect("typed the suggestion's next chars").into());
            }
            Step::Type(_) if ai.ghost_text().is_some() => typed_before_anchor = true,
            Step::Accept | Step::AcceptWord | Step::AcceptLine | Step::Dismiss => {
                rest = live.clone();
            }
            Step::Caret { .. } if live.is_some() && typed_before_anchor => shown_again = true,
            _ => {}
        }
        if let Some(live) = &live {
            assert_eq!(Some(live), rest.as_ref(), "after {step:?}");
        }
        if !matches!(step, Step::Type(_)) {
            continue;
        }
        let snapshot = model.snapshot();
        if let Some(stream) = ai.stream() {
            let region = stream.range(snapshot);
            assert_eq!(
                snapshot.text().slice_to_string(region),
                pushed,
                "after {step:?}"
            );
        }
        for proposal in ai.proposals() {
            for hunk in proposal.borrow().hunks(snapshot) {
                if hunk.status == HunkStatus::Pending {
                    assert_eq!(
                        snapshot.text().slice_to_string(hunk.range),
                        *hunk.old_text,
                        "after {step:?}"
                    );
                }
            }
        }
    }
    assert!(typed_before_anchor && shown_again);

    let expected = DEMO_TEXT
        .replacen("use std", "// Word statistics.\nuse std", 1)
        .replacen("in `text`.", "in `text`, ignoring case.", 1)
        .replacen("or_insert(0)", "or_default()", 1)
        .replacen(
            "/// The `n` most",
            "/// How many different words `counts` holds.\n\
             pub fn distinct_words(counts: &HashMap<String, usize>) -> usize {\n\
            \x20   counts.len()\n\
             }\n\
             \n\
             // Ranking.\n\
             /// The `n` most",
            1,
        )
        .replacen("fn main() {", "fn main() { // A demo.", 1)
        .replacen(
            "        println!(\"{word}: {count}\");\n    }\n",
            "        println!(\"{word}: {count}\");\n    }\n\
            \x20   let total: usize = counts.values().sum();\n\
            \x20   println!(\"{total} words\");\n",
            1,
        );
    assert_eq!(text(&model), expected);

    // The accepted hunk's row, then the streamed region's.
    let decorations = touched(&model);
    assert_eq!(rows(&model, &decorations), [(8, 8), (13, 17)]);
    let covered: Vec<String> = (decorations.iter())
        .map(|decoration| model.text().slice_to_string(decoration.range.clone()))
        .collect();
    assert_eq!(
        covered,
        [
            "        *counts.entry(word).or_default() += 1;\n",
            pushed.as_str()
        ]
    );
    for decoration in &decorations {
        assert_eq!(
            (&decoration.kind, decoration.tint),
            (&DecorationKind::RowBackground, Tint::Accent)
        );
    }
    let proposal = ai.proposals()[0].borrow();
    let statuses: Vec<_> = (proposal.hunks(model.snapshot()).into_iter())
        .map(|hunk| hunk.status)
        .collect();
    assert_eq!(statuses, [HunkStatus::Rejected, HunkStatus::Accepted]);
}

#[test]
fn undo_grouped_by_origin() {
    let (mut model, mut ai) = model("alpha\nomega\n");
    play(
        &mut model,
        &mut ai,
        "\
caret 1:6
type  beta
suggest  gamma
accept
caret 2:1
stream
push one\\n
caret 1:1
type >
push two\\n
caret 4:1
type -
push three\\n
finish
propose 1-1 >alpha beta gamma delta\\n
accept-hunk 1
",
    );
    let all = ">alpha beta gamma delta\none\ntwo\nthree\n-omega\n";
    assert_eq!(text(&model), all);
    for expected in [
        // The accepted hunk.
        ">alpha beta gamma\none\ntwo\nthree\n-omega\n",
        // Typing after the stream, then before it.
        ">alpha beta gamma\none\ntwo\nthree\nomega\n",
        "alpha beta gamma\none\ntwo\nthree\nomega\n",
        // The accepted suggestion, as one step.
        "alpha beta\none\ntwo\nthree\nomega\n",
        // The typing, a word at a time.
        "alpha \none\ntwo\nthree\nomega\n",
        "alpha\none\ntwo\nthree\nomega\n",
    ] {
        assert!(model.undo().unwrap().is_some());
        assert_eq!(text(&model), expected);
    }
    // Nothing of the user's is left to undo; every streamed byte stays.
    assert_eq!(model.undo(), Ok(None));
    assert_eq!(text(&model), "alpha\none\ntwo\nthree\nomega\n");

    play(&mut model, &mut ai, "undo-ai");
    assert_eq!(text(&model), "alpha\nomega\n");
    play(&mut model, &mut ai, "redo-ai");
    assert_eq!(text(&model), "alpha\none\ntwo\nthree\nomega\n");
}

/// Runs `step`, which must fail with `error` and change neither the text
/// nor the touched rows.
#[track_caller]
fn refuse(model: &mut EditorModel, ai: &mut ScriptedAi, step: Step, error: StepError) {
    let (before, marks) = (text(model), touched(model));
    assert_eq!(ai.step(model, &step), Err(error), "{step:?}");
    assert_eq!(text(model), before, "{step:?}");
    assert_eq!(touched(model), marks, "{step:?}");
}

#[test]
fn bad_steps_change_nothing() {
    use StepError as E;

    let (mut model, mut ai) = model("alpha beta\ngamma\n");
    refuse(&mut model, &mut ai, Step::Push("x".into()), E::NoStream);
    refuse(&mut model, &mut ai, Step::Finish, E::NoStream);
    for step in [
        Step::Accept,
        Step::AcceptWord,
        Step::AcceptLine,
        Step::Dismiss,
    ] {
        refuse(&mut model, &mut ai, step, E::NoSuggestion);
    }
    // A suggestion the caret has left isn't live.
    play(&mut model, &mut ai, "caret 1:6\nsuggest  delta\ncaret 2:1");
    refuse(&mut model, &mut ai, Step::Accept, E::NoSuggestion);
    refuse(&mut model, &mut ai, Step::AcceptHunk(1), E::NoProposal);
    refuse(
        &mut model,
        &mut ai,
        Step::Caret { row: 4, col: 1 },
        E::RowPastEnd(4),
    );
    refuse(
        &mut model,
        &mut ai,
        Step::Propose {
            first: 1,
            last: 4,
            text: "x\n".into(),
        },
        E::RowPastEnd(4),
    );

    // A stream marks rows; the failures after it leave them be.
    play(&mut model, &mut ai, "caret 3:1\nstream\npush zeta\\n");
    refuse(&mut model, &mut ai, Step::Stream, E::StreamOpen);
    play(&mut model, &mut ai, "undo-ai");
    refuse(
        &mut model,
        &mut ai,
        Step::Push("eta\n".into()),
        E::Stream(StreamError::Undone),
    );
    // The dead stream was dropped.
    assert!(ai.stream().is_none());
    refuse(&mut model, &mut ai, Step::Finish, E::NoStream);
    play(&mut model, &mut ai, "redo-ai");
    assert_eq!(text(&model), "alpha beta\ngamma\nzeta\n");
    assert_eq!(rows(&model, &touched(&model)), [(2, 2)]);

    // The user edits inside the target before the first chunk lands.
    model.select_range(0..5, false);
    play(&mut model, &mut ai, "stream\ncaret 1:3\ntype x");
    refuse(
        &mut model,
        &mut ai,
        Step::Push("ALPHA".into()),
        E::Stream(StreamError::Conflict),
    );
    assert!(ai.stream().is_none());

    // Two hunks: rows 1 and 3.
    play(&mut model, &mut ai, "propose 1-3 one\\ngamma\\nthree\\n");
    refuse(&mut model, &mut ai, Step::AcceptHunk(3), E::NoSuchHunk(3));
    refuse(&mut model, &mut ai, Step::AcceptHunk(0), E::NoSuchHunk(0));
    refuse(&mut model, &mut ai, Step::RejectHunk(3), E::NoSuchHunk(3));
    play(&mut model, &mut ai, "accept-hunk 2");
    refuse(
        &mut model,
        &mut ai,
        Step::AcceptHunk(2),
        E::Proposal(ProposalError::NotPending),
    );
    refuse(
        &mut model,
        &mut ai,
        Step::RejectHunk(2),
        E::Proposal(ProposalError::NotPending),
    );
    // Typing inside hunk 1's old text makes it stale.
    play(&mut model, &mut ai, "caret 1:2\ntype y");
    refuse(
        &mut model,
        &mut ai,
        Step::AcceptHunk(1),
        E::Proposal(ProposalError::Stale),
    );
    assert_eq!(text(&model), "aylxpha beta\ngamma\nthree\n");
}
