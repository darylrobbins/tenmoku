;; outline query, ours; capture convention of tree-sitter tags

; A rule set by its selectors. One in an at-rule's block or in another rule
; set nests under it.
(rule_set (selectors) @name) @definition.class

; @media and @supports by their first query, after any comments, and
; @keyframes by its name.
(media_statement
  .
  [(comment) (js_comment)]*
  .
  [
    (binary_query)
    (feature_query)
    (keyword_query)
    (parenthesized_query)
    (selector_query)
    (unary_query)
  ] @name) @definition.module

(supports_statement
  .
  [(comment) (js_comment)]*
  .
  [
    (binary_query)
    (feature_query)
    (keyword_query)
    (parenthesized_query)
    (selector_query)
    (unary_query)
  ] @name) @definition.module

(keyframes_statement (keyframes_name) @name) @definition.module
