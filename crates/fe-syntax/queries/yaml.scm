; Ours. After the crate's query, which paints keys `property`; these win on the
; same node. Keys of block and flow mappings, plain and quoted, are data-file
; keys, matching what JSON's query gives member names. A plain key can be any
; plain scalar subtype (`123:`, `true:`, `null:`), whose node the crate's query
; paints as a number or constant.

(block_mapping_pair
  key: (flow_node
    [
      (plain_scalar
        (_) @string.special.key)
      (double_quote_scalar) @string.special.key
      (single_quote_scalar) @string.special.key
    ]))

(flow_pair
  key: (flow_node
    [
      (plain_scalar
        (_) @string.special.key)
      (double_quote_scalar) @string.special.key
      (single_quote_scalar) @string.special.key
    ]))
