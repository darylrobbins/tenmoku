//! Standalone fastedit host. For now (M0) it only proves the GPUI toolchain:
//! a window with the workbench's rough shape, editor left, project tree right.

use gpui::{
    App, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, rgb, size,
};

struct Workbench;

impl Render for Workbench {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .bg(rgb(0x1a1b1e))
            .text_color(rgb(0xc9ccd3))
            .text_size(px(13.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("fastedit"),
            )
            .child(
                div()
                    .w(px(260.))
                    .h_full()
                    .border_l_1()
                    .border_color(rgb(0x2a2c31))
                    .bg(rgb(0x16171a))
                    .p_3()
                    .text_color(rgb(0x8b8f98))
                    .child("Project"),
            )
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1100.), px(720.)), cx)),
                titlebar: Some(TitlebarOptions {
                    title: Some("fastedit".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Workbench),
        )
        .expect("failed to open the main window");
        cx.activate(true);
    });
}
