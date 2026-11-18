//! T1853 F2: an evaluation in the Debug Console can change the program's
//! data in the stop, so once it's answered, the children shown and the
//! watches are asked for again: a node whose reference stays the same in
//! the stop shows its new children, in the console, Variables and Watch.

use fe_dap::protocol::{SetVariableArguments, request};
use gpui::TestAppContext;

use super::{
    debug_commands::{Debug, main_path, requests, script, trusted},
    debug_panel::{keys, panel},
};
use crate::debug_panel::{ConsoleRow, Part, ValueRow};

/// The children shown named `x`, with their values: in the console, in
/// Variables (point's) and in Watch.
fn xs(debug: &mut Debug) -> [Vec<String>; 3] {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| {
        let x = |row: &ValueRow| match row {
            ValueRow::Variable { name, value, .. } if name == "x" => Some(value.text.clone()),
            _ => None,
        };
        let console = (panel.console_rows().0.iter())
            .filter_map(|row| match row {
                ConsoleRow::Child(row) => x(row),
                _ => None,
            })
            .collect();
        // point's own, not another local's.
        let rows = panel.variable_rows().0;
        let point = (rows.iter())
            .position(|row| matches!(row, ValueRow::Variable { name, .. } if name == "point"));
        let variables = (point.into_iter())
            .flat_map(|ix| rows[ix + 1..].iter().find_map(x))
            .collect();
        let watch = panel.watch_rows().0.iter().filter_map(x).collect();
        [console, variables, watch]
    })
}

/// Whether the console, Variables and Watch each show an `x`, every one
/// `value`.
fn every_x_is(debug: &mut Debug, value: &str) -> bool {
    (xs(debug).iter()).all(|shown| !shown.is_empty() && shown.iter().all(|x| x == value))
}

fn value_rows(debug: &mut Debug, part: Part) -> Vec<ValueRow> {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| match part {
        Part::Watch => panel.watch_rows().0,
        _ => panel.variable_rows().0,
    })
}

/// Expands the Variables (or Watch) row `name`.
fn expand(debug: &mut Debug, part: Part, name: &str) {
    let rows = value_rows(debug, part);
    let ix = (rows.iter()).position(|row| match row {
        ValueRow::Variable { name: have, .. } => have == name,
        ValueRow::Watch { expression, .. } => expression == name,
        _ => false,
    });
    let ix = ix.unwrap_or_else(|| panic!("no row {name}"));
    let panel = panel(debug);
    panel.update(debug.cx, |panel, cx| panel.click_value_row(part, ix, cx));
    debug.draw();
}

fn evaluate(debug: &mut Debug, expression: &str) {
    debug.cx.simulate_input(expression);
    keys(debug, "enter");
}

#[gpui::test]
fn a_console_evaluation_refreshes_the_stops_children_and_watches(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(1);
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.add_watch("point".into(), cx));
    debug.until("the locals and the watch", |debug| {
        let rows = value_rows(debug, Part::Variables);
        let watched = value_rows(debug, Part::Watch);
        rows.iter()
            .any(|row| matches!(row, ValueRow::Variable { name, .. } if name == "point"))
            && watched.iter().any(|row| row.reference().is_some())
    });
    expand(&mut debug, Part::Variables, "point");
    expand(&mut debug, Part::Watch, "point");

    // point evaluated in the console, its result expanded.
    debug.run("debug.show_console");
    evaluate(&mut debug, "point");
    debug.until("the result", |debug| {
        let rows = panel.read_with(debug.cx, |panel, _| panel.console_rows().0);
        rows.iter()
            .any(|row| matches!(row, ConsoleRow::Result { .. }))
    });
    let (ix, reference) = panel.read_with(debug.cx, |panel, _| {
        let rows = panel.console_rows().0;
        (rows.iter().enumerate())
            .find_map(|(ix, row)| match row {
                ConsoleRow::Result { value, .. } => Some((ix, value.reference)),
                _ => None,
            })
            .expect("a result")
    });
    panel.update(debug.cx, |panel, cx| panel.click_console_row(ix, cx));
    debug.until("every x", |debug| every_x_is(debug, "1"));

    // The program's point changes in the stop, as an evaluation with a
    // side effect would change it; nothing shown knows yet.
    let debugger = debug.debugger();
    debug.cx.update(|_, cx| {
        let arguments = SetVariableArguments {
            variables_reference: reference,
            name: "x".into(),
            value: "7".into(),
            ..Default::default()
        };
        let session = debugger.read(cx).session().expect("a session");
        let set = session.request::<request::SetVariable>(&arguments);
        cx.spawn(async move |_| _ = set.await).detach();
    });
    debug.until("the change", |_| !requests(&fake, "setVariable").is_empty());
    debug.draw();
    assert!(every_x_is(&mut debug, "1"), "{:?}", xs(&mut debug));

    // Another evaluation in the stop: what shows is asked for again, the
    // result's children under the same reference too.
    let watched = requests(&fake, "evaluate").len();
    evaluate(&mut debug, "point");
    debug.until("every x anew", |debug| every_x_is(debug, "7"));
    let evaluated = requests(&fake, "evaluate");
    let watch_again = (evaluated[watched..].iter())
        .filter(|arguments| arguments["context"] == "watch")
        .count();
    assert_eq!(watch_again, 1, "the watch evaluated again");
}
