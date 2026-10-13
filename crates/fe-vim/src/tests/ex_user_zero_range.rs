use crate::{HeadlessVim, HostCommand as H};

fn host(keys: &str) -> Option<H> {
    let mut vim = HeadlessVim::new("a\nb\nc\nd", (1, 1));
    vim.feed(keys).unwrap();
    vim.state_mut().take_host_command()
}

fn user(name: &str, range: Option<std::ops::Range<u32>>, args: &str) -> Option<H> {
    Some(H::User {
        name: name.into(),
        bang: false,
        range,
        args: args.into(),
    })
}

#[test]
fn zero() {
    assert_eq!(host(":0Gclog<CR>"), user("Gclog", Some(0..0), ""));
    assert_eq!(host(":0Gitsigns x<CR>"), user("Gitsigns", Some(0..0), "x"));
    assert_eq!(host(":1Gclog<CR>"), user("Gclog", Some(0..1), ""));
    assert_eq!(host(":Gclog %<CR>"), user("Gclog", None, "%"));
}
