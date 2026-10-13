//! The lanes of a commit graph, laid out row by row from ids and parents, as
//! VS Code's Source Control Graph does: each row's input lanes are the
//! previous row's output lanes, and a merge's further parents get lanes
//! appended at the right.
//!
//! A pure function of the log: no repository. Colours are indexes from a
//! rising counter; the consumer takes them modulo its palette.

/// The open lanes between two rows: the commit each expects next, and its
/// colour. Laying out a further page with the same `GraphLayout` continues the
/// one before.
#[derive(Debug, Clone, Default)]
pub struct GraphLayout {
    lanes: Vec<Lane>,
    next_color: usize,
}

#[derive(Debug, Clone)]
struct Lane {
    /// The id of the commit this lane leads to.
    id: String,
    color: usize,
}

/// One commit's row of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// The commit's column.
    pub column: usize,
    /// Its colour index: a lane keeps its colour along its first-parent line.
    pub color: usize,
    /// Segments in the row's top half (row top edge at `from` to the middle
    /// at `to`) and bottom half (middle at `from` to the bottom edge at `to`).
    pub top: Vec<GraphLine>,
    pub bottom: Vec<GraphLine>,
}

/// A segment between two columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphLine {
    pub from: usize,
    pub to: usize,
    pub color: usize,
}

impl GraphLayout {
    pub fn new() -> Self {
        Self::default()
    }

    /// The row for the next commit, in log order: `id` and its parents'
    /// ids, first parent first.
    pub fn push(&mut self, id: &str, parents: &[String]) -> GraphRow {
        let mut top = Vec::new();
        let mut column = None;
        for (index, lane) in self.lanes.iter().enumerate() {
            let expects = lane.id == id;
            if expects && column.is_none() {
                column = Some(index);
            }
            top.push(GraphLine {
                from: index,
                to: if expects {
                    column.unwrap_or(index)
                } else {
                    index
                },
                color: lane.color,
            });
        }
        // A commit no lane expects opens one at the right.
        let column = column.unwrap_or_else(|| {
            let color = self.take_color();
            self.lanes.push(Lane {
                id: id.to_owned(),
                color,
            });
            self.lanes.len() - 1
        });
        let color = self.lanes[column].color;

        // The middle of the row: the commit's lane continues to its first
        // parent, the other lanes that expected it end at its dot, and the
        // rest pass through, shifting left past the ones that closed.
        let mut bottom = Vec::new();
        let mut output: Vec<Lane> = Vec::with_capacity(self.lanes.len() + parents.len());
        for (index, lane) in std::mem::take(&mut self.lanes).into_iter().enumerate() {
            if index == column {
                let Some(first) = parents.first() else {
                    continue;
                };
                bottom.push(GraphLine {
                    from: index,
                    to: output.len(),
                    color,
                });
                output.push(Lane {
                    id: first.clone(),
                    color,
                });
            } else if lane.id != id {
                bottom.push(GraphLine {
                    from: index,
                    to: output.len(),
                    color: lane.color,
                });
                output.push(lane);
            }
        }
        // Each further parent joins the lane already expecting it, else opens
        // one at the right.
        for parent in parents.iter().skip(1) {
            if parent == &parents[0] {
                continue;
            }
            let to = match output.iter().position(|lane| &lane.id == parent) {
                Some(to) => to,
                None => {
                    let color = self.take_color();
                    output.push(Lane {
                        id: parent.clone(),
                        color,
                    });
                    output.len() - 1
                }
            };
            let line = GraphLine {
                from: column,
                to,
                color: output[to].color,
            };
            if !bottom.contains(&line) {
                bottom.push(line);
            }
        }
        self.lanes = output;
        GraphRow {
            column,
            color,
            top,
            bottom,
        }
    }

    fn take_color(&mut self) -> usize {
        let color = self.next_color;
        self.next_color += 1;
        color
    }
}
