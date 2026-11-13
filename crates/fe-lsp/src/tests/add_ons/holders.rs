//! Holders (§4): one per (URI, lead), chosen apart from placement and
//! placed on its own route; groups merging on a reload; two leads' holders
//! meeting on one add-on.

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;

use super::{CSS, Fixture, SCSS, defined, settle, spec};

#[test]
fn same_lead_different_add_on_sets_have_one_holder() {
    // `css` is the primary of both languages; `vars` serves SCSS alone.
    for css_first in [true, false] {
        let mut f = Fixture::new(&["css", "vars"]);
        f.manager.register_spec(spec("css", &[CSS, SCSS], false));
        f.manager.register_spec(spec("vars", &[SCSS], true));
        let mut css = Buffer::new("css {}\n");
        let mut scss = Buffer::new("scss {}\n");
        let (first, second, first_language, second_language) = if css_first {
            (&mut css, &mut scss, CSS, SCSS)
        } else {
            (&mut scss, &mut css, SCSS, CSS)
        };
        let uri = f.open(first, "a.css", first_language);
        f.open(second, "a.css", second_language);
        let what = if css_first { "CSS first" } else { "SCSS first" };

        // One group, whatever the add-ons: the first opened holds it, on
        // its own route, and the other waits on every server.
        assert_eq!(f.manager.route_of(css.id()).as_deref(), Some("css"));
        assert_eq!(f.manager.route_of(scss.id()).as_deref(), Some("css"));
        let (holder, alias) = if css_first {
            (&css, &scss)
        } else {
            (&scss, &css)
        };
        f.wait_synced("css", &uri, holder);
        assert!(f.manager.is_served(holder.id()), "{what}");
        assert!(f.manager.servers_of(alias.id()).is_empty(), "{what}");
        if css_first {
            // The CSS buffer's route has no add-on: `vars` never starts.
            settle();
            assert_eq!(f.fake("vars").launches(), 0, "{what}");
        } else {
            f.wait_synced("vars", &uri, &scss);
            assert_eq!(f.manager.servers_of(scss.id()).len(), 2, "{what}");
        }

        // Closing the holder hands the path to the other, on its route.
        let alias_id = alias.id();
        f.manager.close(holder.id());
        let alias = if css_first { &scss } else { &css };
        f.wait_synced("css", &uri, alias);
        if css_first {
            f.wait_synced("vars", &uri, &scss);
            assert_eq!(f.manager.servers_of(alias_id).len(), 2, "{what}");
        } else {
            f.wait_closed("vars", &uri);
            assert_eq!(f.manager.servers_of(alias_id).len(), 1, "{what}");
        }
    }
}

/// Settings defining `css` for `css_languages`, `scss` for SCSS (on if
/// `scss`), and `vars`, an add-on for SCSS.
fn settings(css_languages: &[&str], scss: bool) -> LspSettings {
    defined(&[
        ("css", css_languages, false, true),
        ("scss", &[SCSS], false, scss),
        ("vars", &[SCSS], true, true),
    ])
}

#[test]
fn reload_merging_two_groups_keeps_earlier_holder() {
    let mut f = Fixture::new(&["css", "scss", "vars"]);
    f.manager.update_settings(&settings(&[CSS], true));
    let mut css = Buffer::new("css {}\n");
    let mut scss = Buffer::new("scss {}\n");
    let uri = f.open(&mut css, "a.css", CSS);
    f.open(&mut scss, "a.css", SCSS);

    // Two leads, two groups: each holds the path on its own servers.
    f.wait_synced("css", &uri, &css);
    f.wait_synced("scss", &uri, &scss);
    f.wait_synced("vars", &uri, &scss);
    assert_eq!(f.manager.route_of(scss.id()).as_deref(), Some("scss"));
    assert_eq!(f.manager.servers_of(scss.id()).len(), 2);

    // `css` now leads SCSS too, and `scss` is off: the groups merge, and
    // the earlier holder keeps the path. The other is closed on each
    // server it was on: `scss` shuts down, and `vars` stays up.
    f.manager.update_settings(&settings(&[CSS, SCSS], false));
    f.fake("scss").wait_for("scss to shut down", |records| {
        records.iter().any(|r| r.method() == "shutdown")
    });
    f.wait_closed("vars", &uri);
    assert_eq!(f.fake("vars").launches(), 1);
    assert_eq!(f.manager.route_of(scss.id()).as_deref(), Some("css"));
    assert!(f.manager.servers_of(scss.id()).is_empty());
    assert!(f.manager.served_identities(scss.id()).is_empty());
    // `css` restarted for its new languages, with the holder.
    let text = css.text().to_string();
    f.fake("css")
        .wait_for_document(&uri, "the holder on the new instance", |r| {
            r.is_some_and(|r| r.open && r.launch == 2 && r.text == text)
        });
    let lead = f.manager.lead_of(css.id()).unwrap();
    assert_eq!(f.manager.servers_of(css.id()), [lead]);
    f.barrier(lead);
    let replica = f.fake("css").document(&uri).unwrap();
    assert_eq!(
        (replica.launch, replica.text),
        (2, text),
        "the alias never got there"
    );

    // Closing the holder hands the path to the alias, on its route.
    f.manager.close(css.id());
    f.wait_synced("css", &uri, &scss);
    f.wait_synced("vars", &uri, &scss);
    assert_eq!(f.manager.servers_of(scss.id()).len(), 2);
}

#[test]
fn two_leads_meeting_on_one_add_on_keep_the_first_placed() {
    let mut f = Fixture::new(&["css", "scss", "vars"]);
    f.manager.register_spec(spec("css", &[CSS], false));
    f.manager.register_spec(spec("scss", &[SCSS], false));
    f.manager.register_spec(spec("vars", &[CSS, SCSS], true));
    let mut css = Buffer::new("css {}\n");
    let mut scss = Buffer::new("scss {}\n");
    let uri = f.open(&mut css, "a.css", CSS);
    f.wait_synced("css", &uri, &css);
    f.wait_synced("vars", &uri, &css);
    let vars = f.placed(&css, "vars").unwrap();

    // Separate owners: the second holds the path on its own lead, and
    // waits on the add-on the first holds it on.
    f.open(&mut scss, "a.css", SCSS);
    f.wait_synced("scss", &uri, &scss);
    let lead = f.manager.lead_of(scss.id()).unwrap();
    assert_eq!(f.manager.servers_of(scss.id()), [lead]);
    f.barrier(vars);
    assert_eq!(f.fake("vars").document(&uri).unwrap().text, "css {}\n");

    // Once the first closes, the add-on takes the second.
    f.manager.close(css.id());
    f.wait_synced("vars", &uri, &scss);
    assert_eq!(f.manager.servers_of(scss.id()), [lead, vars]);
}

#[test]
fn reload_keeps_the_earlier_holder_when_it_loses_its_only_server() {
    // The CSS buffer holds the path on `css`, its only server; the later
    // SCSS buffer holds it on `scss`.
    let mut f = Fixture::new(&["css", "scss"]);
    let reload = |css: bool, scss_languages: &[&str]| {
        defined(&[
            ("css", &[CSS], false, css),
            ("scss", scss_languages, false, true),
        ])
    };
    f.manager.update_settings(&reload(true, &[SCSS]));
    let mut css = Buffer::new("css {}\n");
    let mut scss = Buffer::new("scss {}\n");
    let uri = f.open(&mut css, "a.css", CSS);
    f.open(&mut scss, "a.css", SCSS);
    f.wait_synced("css", &uri, &css);
    f.wait_synced("scss", &uri, &scss);

    // `css` goes and `scss` leads CSS too: the groups merge, and the
    // earlier holder keeps the path though it's placed nowhere as they do.
    f.manager.update_settings(&reload(false, &[CSS, SCSS]));
    let text = css.text().to_string();
    f.fake("scss")
        .wait_for_document(&uri, "the earlier holder on scss", |r| {
            r.is_some_and(|r| r.open && r.launch == 2 && r.text == text)
        });
    let lead = f.manager.lead_of(css.id()).unwrap();
    assert_eq!(f.manager.servers_of(css.id()), [lead]);
    assert!(f.manager.servers_of(scss.id()).is_empty());
    assert_eq!(f.manager.route_of(scss.id()).as_deref(), Some("scss"));
    f.barrier(lead);
    assert_eq!(f.fake("scss").document(&uri).unwrap().text, text);

    // Closing it hands the path on.
    f.manager.close(css.id());
    f.wait_synced("scss", &uri, &scss);
    assert_eq!(f.manager.servers_of(scss.id()), [lead]);
}
