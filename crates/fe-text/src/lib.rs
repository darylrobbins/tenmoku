//! Text storage for fastedit.
//!
//! This crate is the only place that names the underlying rope type. Everything
//! else talks to [`Text`] so the rope can be swapped (ropey 2 is still beta).
//! It must never depend on GPUI.

/// An immutable-by-convention text value. Cloning is O(1) and shares storage,
/// so a clone doubles as a snapshot for background work.
#[derive(Clone, Default)]
pub struct Text {
    rope: ropey::Rope,
}

impl Text {
    pub fn new(text: &str) -> Self {
        Self {
            rope: ropey::Rope::from_str(text),
        }
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Display for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for chunk in self.rope.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let text = Text::new("fn main() {}\n");
        assert_eq!(text.len(), 13);
        assert_eq!(text.to_string(), "fn main() {}\n");
    }
}
