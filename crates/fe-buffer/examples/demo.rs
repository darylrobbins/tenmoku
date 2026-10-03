//! Walks through the buffer's AI-editing primitives.
//! Run: `cargo run -p fe-buffer --example demo`

use fe_buffer::{Buffer, Origin};

fn show(step: &str, buffer: &Buffer) {
    println!("── {step}\n{}", buffer.text());
}

fn main() {
    let ai = || Origin::Ai {
        agent: "assistant".into(),
        session: "demo".into(),
    };
    let mut buffer =
        Buffer::new("fn total(items: &[u32]) -> u32 {\n    todo!()\n}\n\nfn main() {\n}\n");
    show("Original", &buffer);

    // The AI computes a change against a snapshot of the file...
    let base = buffer.snapshot();
    let body = base.text().to_string().find("todo!()").unwrap();
    // Right bias: if text is inserted exactly here, the anchor moves after it.
    let brace = buffer
        .current()
        .anchor_after(buffer.text().to_string().rfind('}').unwrap());

    // ...while the user keeps typing elsewhere.
    let main_body = buffer.text().to_string().rfind('}').unwrap();
    buffer.edit(
        [(
            main_body..main_body,
            "    println!(\"{}\", total(&[1, 2, 3]));\n",
        )],
        Origin::User,
    );
    show("User typed in main() while the AI was thinking", &buffer);

    // The AI's edit lands where it was meant to, despite the user's typing.
    buffer
        .edit_from(
            &base,
            [(body..body + "todo!()".len(), "items.iter().sum()")],
            ai(),
        )
        .expect("no conflict");
    show("AI edit applied against its older snapshot", &buffer);
    let offset = buffer.current().resolve(&brace);
    println!(
        "(an anchor placed before main's closing brace is now at offset {offset}, still before {:?})\n",
        buffer.text().char_at(offset).unwrap()
    );

    // A stale AI edit that targets text the user changed is refused.
    let stale = buffer.snapshot();
    let at = buffer.text().to_string().find("[1, 2, 3]").unwrap();
    buffer.edit([(at + 1..at + 2, "10")], Origin::User);
    let conflict = buffer.edit_from(&stale, [(at..at + "[1, 2, 3]".len(), "[4, 5]")], ai());
    println!("── Stale AI edit over text the user just changed: {conflict:?}\n");

    // Undo just the AI's work; the user's edits stay.
    buffer.undo(|origin| origin.is_ai()).unwrap();
    show("Undo AI only (user's edits kept)", &buffer);
    buffer.redo(|origin| origin.is_ai()).unwrap();
    show("Redo AI", &buffer);
}
