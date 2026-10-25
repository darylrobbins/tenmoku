//! The buffer registry notes each tab a pane gains, shows or loses as it
//! happens, however many come in one update.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::PaneView as _;

use super::super::super::vim_host_tests::{draw, window, write};
use crate::Workbench;

/// Every buffer's number, in tab order, of the active pane's tabs now.
fn numbers(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<u32> {
    workbench.update(cx, |workbench, cx| {
        let mut tabs = workbench.buffer_tabs(cx);
        workbench.vim_buffers(&mut tabs, cx, |_| ());
        tabs.iter().map(|tab| tab.number).collect()
    })
}

#[gpui::test]
fn several_tabs_in_one_update_keep_distinct_numbers(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt"]);
    let cx = &mut vcx;
    assert_eq!(numbers(&workbench, cx), [1]);

    // Two tabs added and the first removed again, all in one update.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new("one"), None), cx);
            pane.add(EditorModel::new(Buffer::new("two"), None), cx);
            let first = pane.tabs().len() - 2;
            pane.take_item(first, window, cx);
        });
    });
    draw(cx);
    // The removed tab was numbered, so the one after it has the next.
    assert_eq!(numbers(&workbench, cx), [1, 3]);
    let last = workbench.update(cx, |workbench, cx| {
        let mut tabs = workbench.buffer_tabs(cx);
        workbench.vim_buffers(&mut tabs, cx, |buffers| buffers.numbered.len())
    });
    assert_eq!(last, 3);
}

#[gpui::test]
fn activations_in_one_update_are_recorded_in_order(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;
    assert_eq!(numbers(&workbench, cx), [1, 2, 3]);

    // Tab 2, then 1, then 3 shown in one update: the pane showed 3 last,
    // 1 before it, so its alternate from 3 is 1, not 2.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            for ix in [1, 0, 2] {
                pane.activate(ix, window, cx);
            }
        });
    });
    draw(cx);
    numbers(&workbench, cx);
    let alternate = workbench.update(cx, |workbench, cx| {
        workbench.vim_buffers(&mut [], cx, |buffers| {
            buffers.alternate(pane.entity_id(), Some(3))
        })
    });
    assert_eq!(alternate, Some(1));
}

#[gpui::test]
fn tabs_added_in_one_update_are_each_shown_in_order(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt"]);
    let cx = &mut vcx;

    // No read of the registry between the mutations and the first read.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    pane.update(cx, |pane, cx| {
        pane.add(EditorModel::new(Buffer::new("one"), None), cx);
        pane.add(EditorModel::new(Buffer::new("two"), None), cx);
    });
    assert_eq!(numbers(&workbench, cx), [1, 2, 3]);
    let alternate = workbench.update(cx, |workbench, cx| {
        workbench.vim_buffers(&mut [], cx, |buffers| {
            buffers.alternate(pane.entity_id(), Some(3))
        })
    });
    assert_eq!(alternate, Some(2));
}

#[gpui::test]
fn a_restored_active_tab_is_shown_before_the_next(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt"]);
    let cx = &mut vcx;

    // Tabs restored with one active, then another shown, in one update.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let items: Vec<Box<dyn kiln_shell::ItemHandle>> = ["one", "two"]
                .into_iter()
                .map(|text| {
                    Box::new(pane.new_editor(EditorModel::new(Buffer::new(text), None), cx))
                        as Box<dyn kiln_shell::ItemHandle>
                })
                .collect();
            pane.add_items(items, 0, cx);
            pane.activate(pane.tabs().len() - 1, window, cx);
        });
    });
    assert_eq!(numbers(&workbench, cx), [1, 2, 3]);
    let alternate = workbench.update(cx, |workbench, cx| {
        workbench.vim_buffers(&mut [], cx, |buffers| {
            buffers.alternate(pane.entity_id(), Some(3))
        })
    });
    assert_eq!(alternate, Some(2));
}
