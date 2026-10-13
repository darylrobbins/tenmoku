;; outline query, ours; capture convention of tree-sitter tags

; Every pair with a key, block or flow, named by the key as written, quotes
; and all: a module when its value is a mapping, else a field. The module
; patterns come first, so they win. Mappings in sequence items, - name: x,
; nest under the pair whose value holds the sequence.
(block_mapping_pair
  key: (_) @name
  value: [(block_node (block_mapping)) (flow_node (flow_mapping))]) @definition.module

(flow_pair
  key: (_) @name
  value: (flow_node (flow_mapping))) @definition.module

(block_mapping_pair key: (_) @name) @definition.field

(flow_pair key: (_) @name) @definition.field
