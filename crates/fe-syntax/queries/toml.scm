; Replaces tree-sitter-toml-ng's highlights.scm for TOML.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit e289100ff98969e118c702199d88b764ce9e7fdf (file last changed in
; 692b051b09935653befdb8f7ba8afdb640adf17b),
; path runtime/queries/toml/highlights.scm, license Apache-2.0.
; Changes: the nvim-only `@spell` is dropped, `@number.float` becomes
; `@number`, and table-header keys are captured as `type`, which the source
; doesn't do.
;
; The crate's own query can't be extended instead: its
; `(pair (bare_key)) @property` covers the whole pair, so the whitespace in
; each pair gets a stray span, and appended patterns can't cancel it.
; Later patterns win over earlier ones on the same node.

(bare_key) @string.special.key

[
  (string)
  (quoted_key)
] @string

(boolean) @boolean

(comment) @comment

(escape_sequence) @string.escape

[
  (integer)
  (float)
] @number

[
  (local_date)
  (local_date_time)
  (local_time)
  (offset_date_time)
] @string.special

"=" @operator

[
  "."
  ","
] @punctuation.delimiter

[
  "["
  "]"
  "[["
  "]]"
  "{"
  "}"
] @punctuation.bracket

; Every component of a key, quoted or not, is a data-file key (`string.special.key`) in a pair and a type
; in a table header. Dotted keys nest to the left (`a.b.c` is
; `(dotted_key (dotted_key a b) c)`), so each pattern names a component's
; depth under the pair or header, up to 10 nested dotted_keys, so a key of up
; to 11 components is covered. This is a limit of the query language, which
; can't recurse. In a longer key the leftmost components, which sit deeper,
; keep the plain `string.special.key` or `string` above, even in a header. Headers come
; last so they win.

(pair
  (bare_key) @string.special.key)

(pair
  (dotted_key
    (bare_key) @string.special.key))

(pair
  (dotted_key
    (dotted_key
      (bare_key) @string.special.key)))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (bare_key) @string.special.key))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (bare_key) @string.special.key)))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (bare_key) @string.special.key))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (bare_key) @string.special.key)))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (bare_key) @string.special.key))))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (bare_key) @string.special.key)))))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (bare_key) @string.special.key))))))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (dotted_key
                      (bare_key) @string.special.key)))))))))))

(pair
  (quoted_key) @string.special.key)

(pair
  (dotted_key
    (quoted_key) @string.special.key))

(pair
  (dotted_key
    (dotted_key
      (quoted_key) @string.special.key)))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (quoted_key) @string.special.key))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (quoted_key) @string.special.key)))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (quoted_key) @string.special.key))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (quoted_key) @string.special.key)))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (quoted_key) @string.special.key))))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (quoted_key) @string.special.key)))))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (quoted_key) @string.special.key))))))))))

(pair
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (dotted_key
                      (quoted_key) @string.special.key)))))))))))

(table
  (bare_key) @type)

(table
  (dotted_key
    (bare_key) @type))

(table
  (dotted_key
    (dotted_key
      (bare_key) @type)))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (bare_key) @type))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (bare_key) @type)))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (bare_key) @type))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (bare_key) @type)))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (bare_key) @type))))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (bare_key) @type)))))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (bare_key) @type))))))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (dotted_key
                      (bare_key) @type)))))))))))

(table
  (quoted_key) @type)

(table
  (dotted_key
    (quoted_key) @type))

(table
  (dotted_key
    (dotted_key
      (quoted_key) @type)))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (quoted_key) @type))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (quoted_key) @type)))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (quoted_key) @type))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (quoted_key) @type)))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (quoted_key) @type))))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (quoted_key) @type)))))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (quoted_key) @type))))))))))

(table
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (dotted_key
                      (quoted_key) @type)))))))))))

(table_array_element
  (bare_key) @type)

(table_array_element
  (dotted_key
    (bare_key) @type))

(table_array_element
  (dotted_key
    (dotted_key
      (bare_key) @type)))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (bare_key) @type))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (bare_key) @type)))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (bare_key) @type))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (bare_key) @type)))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (bare_key) @type))))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (bare_key) @type)))))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (bare_key) @type))))))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (dotted_key
                      (bare_key) @type)))))))))))

(table_array_element
  (quoted_key) @type)

(table_array_element
  (dotted_key
    (quoted_key) @type))

(table_array_element
  (dotted_key
    (dotted_key
      (quoted_key) @type)))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (quoted_key) @type))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (quoted_key) @type)))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (quoted_key) @type))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (quoted_key) @type)))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (quoted_key) @type))))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (quoted_key) @type)))))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (quoted_key) @type))))))))))

(table_array_element
  (dotted_key
    (dotted_key
      (dotted_key
        (dotted_key
          (dotted_key
            (dotted_key
              (dotted_key
                (dotted_key
                  (dotted_key
                    (dotted_key
                      (quoted_key) @type)))))))))))
