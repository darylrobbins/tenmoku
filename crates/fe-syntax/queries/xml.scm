; Ours. After the crate's query, so this wins on the same node.
; tree-sitter-xml 0.7.0 captures attribute names and `<?xml ...?>` pseudo
; attributes as @property, which most themes leave as plain text.
(Attribute (Name) @attribute)
(PseudoAtt (Name) @attribute)
; The declaration's own `version`, `encoding` and `standalone` are keyword
; tokens, not Names.
[ "version" "encoding" "standalone" ] @attribute
