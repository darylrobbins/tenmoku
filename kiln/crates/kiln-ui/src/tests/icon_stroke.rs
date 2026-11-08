// SPDX-License-Identifier: Apache-2.0

use crate::Icon;

/// The design's line icons: a 24 grid stroked at 1.75 (Lucide ships 2).
#[test]
fn every_bundled_icon_strokes_at_1_75() {
    for icon in Icon::ALL {
        let svg = std::str::from_utf8(icon.svg()).expect("UTF-8");
        assert!(
            svg.contains(r#"viewBox="0 0 24 24""#),
            "{icon:?} isn't on a 24 grid"
        );
        let widths: Vec<&str> = svg
            .split("stroke-width=\"")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap_or(""))
            .collect();
        assert_eq!(widths, ["1.75"], "{icon:?} stroke widths");
    }
}
