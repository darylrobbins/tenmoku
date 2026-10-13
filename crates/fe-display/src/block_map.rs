//! Blocks: whole rows placed between display rows, such as a proposed
//! change's new text under the text it replaces.
//!
//! Block rows are *screen rows*, never display rows: the [`DisplayMap`] and
//! movement over it don't see them. Layout places display row `r` at screen
//! row [`BlockMap::screen_row`]`(r)`.
//!
//! [`DisplayMap`]: crate::DisplayMap

use std::ops::Range;

/// `rows` rows placed above display row `above`; `above` equal to the row
/// count places them after the last row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub above: u32,
    pub rows: u32,
    /// The caller's, echoed back.
    pub id: u64,
}

/// What a screen row shows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScreenRow {
    /// A display row.
    Text(u32),
    /// Row `line` of the block with `id`.
    Block { id: u64, line: u32 },
}

/// The blocks of an editor, sorted by the display row they're above.
#[derive(Clone, Debug)]
pub struct BlockMap {
    blocks: Vec<Block>,
    /// The block rows before each block, then all of them.
    rows_before: Vec<u32>,
    /// The screen row each block starts at.
    starts: Vec<u32>,
}

impl Default for BlockMap {
    /// No blocks.
    fn default() -> Self {
        Self::new([])
    }
}

impl BlockMap {
    /// Sorts `blocks` by the row they're above, keeping the order of blocks
    /// above the same row, and drops blocks of no rows.
    pub fn new(blocks: impl IntoIterator<Item = Block>) -> Self {
        let mut blocks: Vec<Block> = blocks.into_iter().filter(|block| block.rows > 0).collect();
        blocks.sort_by_key(|block| block.above);
        let mut rows_before = Vec::with_capacity(blocks.len() + 1);
        let mut starts = Vec::with_capacity(blocks.len());
        let mut total = 0u32;
        rows_before.push(0);
        for block in &blocks {
            starts.push(block.above + total);
            total += block.rows;
            rows_before.push(total);
        }
        Self {
            blocks,
            rows_before,
            starts,
        }
    }

    /// Block rows in all.
    pub fn block_rows(&self) -> u32 {
        self.rows_before[self.blocks.len()]
    }

    /// The screen row of `display_row`: it plus the rows of every block above
    /// it.
    pub fn screen_row(&self, display_row: u32) -> u32 {
        let before = self
            .blocks
            .partition_point(|block| block.above <= display_row);
        display_row + self.rows_before[before]
    }

    /// The screen rows of the block with `id`, if there's one.
    pub fn screen_rows(&self, id: u64) -> Option<Range<u32>> {
        let ix = self.blocks.iter().position(|block| block.id == id)?;
        let start = self.starts[ix];
        Some(start..start + self.blocks[ix].rows)
    }

    /// What `screen_row` shows. Past the last row, it's a display row past
    /// the last too.
    pub fn at_screen_row(&self, screen_row: u32) -> ScreenRow {
        match self.block_at(screen_row) {
            Ok(ix) => ScreenRow::Block {
                id: self.blocks[ix].id,
                line: screen_row - self.starts[ix],
            },
            Err(before) => ScreenRow::Text(screen_row - self.rows_before[before]),
        }
    }

    /// The display row at `screen_row` or, on a block row, the display row
    /// below the block.
    pub(crate) fn display_row_from(&self, screen_row: u32) -> u32 {
        match self.block_at(screen_row) {
            Ok(ix) => self.blocks[ix].above,
            Err(before) => screen_row - self.rows_before[before],
        }
    }

    /// Converts a top in display rows, which may be fractional, to screen
    /// rows.
    pub fn to_screen_top(&self, display_top: f32) -> f32 {
        let row = display_top.floor().max(0.);
        self.screen_row(row as u32) as f32 + (display_top - row)
    }

    /// Converts a top in screen rows, which may be fractional, to display
    /// rows. A top inside a block maps to the start of the display row below
    /// it.
    pub fn to_display_top(&self, screen_top: f32) -> f32 {
        let row = screen_top.floor().max(0.);
        match self.block_at(row as u32) {
            Ok(ix) => self.blocks[ix].above as f32,
            Err(before) => screen_top - self.rows_before[before] as f32,
        }
    }

    /// The blocks with a row in `screen_rows`, with the screen row each
    /// starts at.
    pub(crate) fn blocks_in(&self, screen_rows: Range<u32>) -> impl Iterator<Item = (u32, &Block)> {
        let first = self
            .block_at(screen_rows.start)
            .unwrap_or_else(|before| before);
        let end = self
            .starts
            .partition_point(|&start| start < screen_rows.end)
            .max(first);
        (first..end).map(|ix| (self.starts[ix], &self.blocks[ix]))
    }

    /// The block covering `screen_row`, or else how many blocks are before
    /// it.
    fn block_at(&self, screen_row: u32) -> Result<usize, usize> {
        let after = self.starts.partition_point(|&start| start <= screen_row);
        match after.checked_sub(1) {
            Some(ix) if screen_row < self.starts[ix] + self.blocks[ix].rows => Ok(ix),
            _ => Err(after),
        }
    }
}
