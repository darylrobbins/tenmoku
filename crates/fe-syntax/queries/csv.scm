; Ours, for CSV and TSV. A column's index is its position among the row's
; children, so each pattern counts fields from the start of the row. The first
; 16 columns cycle through 8 names of our vocabulary, so the colours come from
; the theme. Later columns and the separators stay plain.

(row . (field (content) @keyword))
(row . (field) . (field (content) @string))
(row . (field) . (field) . (field (content) @number))
(row . (field) . (field) . (field) . (field (content) @function))
(row . (field) . (field) . (field) . (field) . (field (content) @type))
(row . (field) . (field) . (field) . (field) . (field) . (field (content) @constant))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @property))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @variable.parameter))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @keyword))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @string))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @number))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @function))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @type))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @constant))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @property))
(row . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field) . (field (content) @variable.parameter))
