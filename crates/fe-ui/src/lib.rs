//! fastedit's UI primitives: stateless GPUI elements styled by a
//! [`UiTheme`]. Views that own state (the project panel, the tab pane) live
//! in `fe-workbench` and build their rows and tabs from these.
//!
//! Depends on GPUI alone, never on a workspace crate. Written for fastedit:
//! nothing here is copied from gpui-kit or elsewhere.

mod tab_bar;
mod theme;
mod tree_row;

pub use tab_bar::{TAB_HEIGHT, Tab, TabBar};
pub use theme::UiTheme;
pub use tree_row::{ROW_HEIGHT, TreeRow};

type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>;
