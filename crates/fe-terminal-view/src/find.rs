//! Find in a [`TerminalView`]: holds the query whose matches the element
//! highlights, and steps the session's selection through them.

use fe_terminal::{QueryError, TerminalMatch, TerminalQuery};
use gpui::Context;

use crate::TerminalView;

impl TerminalView {
    /// Highlights `query`'s matches on screen; `None` or an empty `text`
    /// clears it. With a session the query is checked first, and a query that
    /// doesn't compile isn't stored. Before the session starts it's stored
    /// unchecked.
    pub fn set_find(
        &mut self,
        query: Option<TerminalQuery>,
        cx: &mut Context<Self>,
    ) -> Result<(), QueryError> {
        let query = query.filter(|query| !query.text.is_empty());
        if let (Some(query), Some(terminal)) = (&query, &self.terminal) {
            terminal.find_all_visible(query)?;
        }
        if query.is_none() {
            self.find_matches.clear();
        }
        self.find = query;
        cx.notify();
        Ok(())
    }

    /// Selects the next match of the query from the selection, else the
    /// cursor, and scrolls it into view. `Ok(None)` with no query, no
    /// session or no match.
    pub fn find_next(
        &mut self,
        forward: bool,
        cx: &mut Context<Self>,
    ) -> Result<Option<TerminalMatch>, QueryError> {
        let (Some(query), Some(terminal)) = (&self.find, &self.terminal) else {
            return Ok(None);
        };
        let found = terminal.find(query, forward)?;
        cx.notify();
        Ok(found)
    }

    /// The query being highlighted.
    pub fn find_query(&self) -> Option<&TerminalQuery> {
        self.find.as_ref()
    }

    /// The query's matches the last prepaint found on screen.
    pub fn find_matches(&self) -> &[TerminalMatch] {
        &self.find_matches
    }
}
