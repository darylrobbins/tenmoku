//! Commit graph lanes laid out from ids and parents, no repository.

mod graph {
    use fe_git::graph::{GraphLayout, GraphLine, GraphRow};

    /// Lays out `log`, each commit as `"id parent..."`.
    fn layout(log: &[&str]) -> Vec<GraphRow> {
        let mut graph = GraphLayout::new();
        push_all(&mut graph, log)
    }

    fn push_all(graph: &mut GraphLayout, log: &[&str]) -> Vec<GraphRow> {
        log.iter()
            .map(|entry| {
                let mut ids = entry.split_whitespace();
                let id = ids.next().unwrap();
                let parents: Vec<String> = ids.map(str::to_owned).collect();
                graph.push(id, &parents)
            })
            .collect()
    }

    fn line(from: usize, to: usize, color: usize) -> GraphLine {
        GraphLine { from, to, color }
    }

    /// Every lane a row's segments touch.
    fn width(row: &GraphRow) -> usize {
        row.top
            .iter()
            .chain(&row.bottom)
            .map(|line| line.from.max(line.to) + 1)
            .max()
            .unwrap_or(0)
            .max(row.column + 1)
    }

    #[test]
    fn layouts() {
        // Linear: one lane, column 0, joined row to row.
        let rows = layout(&["c3 c2", "c2 c1", "c1"]);
        assert!(rows.iter().all(|row| row.column == 0 && row.color == 0));
        assert!(rows.iter().all(|row| width(row) == 1));
        assert_eq!(rows[0].top, []);
        assert_eq!(rows[0].bottom, [line(0, 0, 0)]);
        assert_eq!(rows[1].top, [line(0, 0, 0)]);
        assert_eq!(rows[2].bottom, []);

        // A merge: the side branch in column 1, its lane closing at the fork.
        let rows = layout(&["m c2 t2", "t2 t1", "t1 c1", "c2 c1", "c1"]);
        let columns: Vec<usize> = rows.iter().map(|row| row.column).collect();
        assert_eq!(columns, [0, 1, 1, 0, 0]);
        assert_eq!(rows[0].bottom, [line(0, 0, 0), line(0, 1, 1)]);
        assert_eq!(rows[1].top, [line(0, 0, 0), line(1, 1, 1)]);
        assert_eq!(rows[1].color, 1);
        assert_eq!(rows[4].top, [line(0, 0, 0), line(1, 0, 1)]);
        assert_eq!(rows[4].bottom, []);
        assert_eq!(width(&rows[4]), 2);

        // An octopus merge opens two lanes.
        let rows = layout(&["m a b c", "c r", "b r", "a r", "r"]);
        assert_eq!(
            rows[0].bottom,
            [line(0, 0, 0), line(0, 1, 1), line(0, 2, 2)]
        );
        let columns: Vec<usize> = rows.iter().map(|row| row.column).collect();
        assert_eq!(columns, [0, 2, 1, 0, 0]);
        assert_eq!(rows[4].top, [line(0, 0, 0), line(1, 0, 1), line(2, 0, 2)]);

        // Two unrelated roots, merged.
        let rows = layout(&["m a b", "b", "a"]);
        assert_eq!(
            (rows[1].column, rows[1].bottom.as_slice()),
            (1, &[line(0, 0, 0)][..])
        );
        assert_eq!((rows[2].column, rows[2].bottom.as_slice()), (0, &[][..]));

        // A parent not listed yet keeps its lane open; a tip no lane expects
        // opens one at the right with the next colour.
        let rows = layout(&["x p", "y q"]);
        assert_eq!(rows[1].column, 1);
        assert_eq!(rows[1].color, 1);
        assert_eq!(rows[1].top, [line(0, 0, 0)]);
        assert_eq!(rows[1].bottom, [line(0, 0, 0), line(1, 1, 1)]);

        // When a lane closes, the lanes right of it shift left.
        let rows = layout(&["x a", "y b", "z c", "b", "a", "c"]);
        assert_eq!(rows[3].column, 1);
        assert_eq!(rows[3].bottom, [line(0, 0, 0), line(2, 1, 2)]);
        assert_eq!((rows[5].column, rows[5].color), (0, 2));

        // Two pages through one layout give the same rows as one.
        let log = [
            "m c3 t2", "t2 t1", "c3 c2", "t1 c1", "c2 c1", "s1 c1", "c1 r", "r",
        ];
        let whole = layout(&log);
        let mut graph = GraphLayout::new();
        let mut paged = push_all(&mut graph, &log[..3]);
        paged.extend(push_all(&mut graph, &log[3..]));
        assert_eq!(paged, whole);

        // A lane keeps its colour along its first-parent line.
        let main = ["m", "c3", "c2", "c1", "r"];
        let topic = ["t2", "t1"];
        for (entry, row) in log.iter().zip(&whole) {
            let id = entry.split_whitespace().next().unwrap();
            if main.contains(&id) {
                assert_eq!(row.color, 0, "{id}");
            } else if topic.contains(&id) {
                assert_eq!(row.color, 1, "{id}");
            } else {
                assert_eq!(row.color, 2, "{id}");
            }
        }
    }
}
