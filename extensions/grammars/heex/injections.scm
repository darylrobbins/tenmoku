; From tree-sitter-heex at 5842537f734d7c12685bf27d6005313e3e5a47a0,
; https://github.com/phoenixframework/tree-sitter-heex, path
; queries/injections.scm, license MIT. Changes: upstream injects a whole
; `<style>`/`<script>` tag as css/javascript and trims the tags off with
; `#offset!`, which fe-syntax lacks, so these inject the element's `(text)`
; children instead, combined. The grammar parses HTML itself (no html
; injection), and knows no raw text: a `{`, `<`, `>` or `&` inside a
; `<style>` or `<script>` starts an expression, tag or entity of the host, so
; only the text between them is css/javascript. Each directive's piece of a
; `<%= if x do %>` ... `<% end %>` is its own elixir layer, as fe-syntax
; combines siblings only.

; Directives are standalone tags like '<%= @x %>'
;
; partial_expression_values are elixir code that is part of an expression that
; spans multiple directive nodes, so they must be combined. For example:
;     <%= if true do %>
;       <p>hello, tree-sitter!</p>
;     <% end %>
((directive
  [
    (partial_expression_value)
    (ending_expression_value)
  ] @injection.content)
  (#set! injection.language "elixir")
  (#set! injection.include-children)
  (#set! injection.combined))

; Regular expression_values do not need to be combined
((directive
  (expression_value) @injection.content)
  (#set! injection.language "elixir"))

; Expressions live within HTML tags, and do not need to be combined
;     <link href={ Routes.static_path(..) } />
; Note that we include children, because expression_value may consist
; of multiple nodes, when the value contains { and }
((expression
  (expression_value) @injection.content)
  (#set! injection.language "elixir")
  (#set! injection.include-children))

; Syntax highlight for style tags
(tag
  (start_tag
    (tag_name) @_tag_name
    (#eq? @_tag_name "style"))
  (text) @injection.content
  (#set! injection.language "css")
  (#set! injection.combined))

; Sytanx highlight for style attributes
(attribute
  (attribute_name) @_attribute_name
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#eq? @_attribute_name "style")
  (#set! injection.language "css"))

; Syntax highlight for script tags
(tag
  (start_tag
    (tag_name) @_tag_name
    (#eq? @_tag_name "script"))
  (text) @injection.content
  (#set! injection.language "javascript")
  (#set! injection.combined))

; Syntax highlight for onEVENT attributes
(attribute
  (attribute_name) @_attribute_name
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#match? @_attribute_name "^on[a-z]+$")
  (#set! injection.language "javascript"))
