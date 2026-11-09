; From tree-sitter-blade at b5291d1ba207a8ebb8383b2ecb8a8a6535210a50,
; https://github.com/EmranMR/tree-sitter-blade, path queries/injections.scm,
; license MIT. Upstream's query starts `; inherits: html`: in its place come
; tree-sitter-html 0.23.2's queries/injections.scm (MIT,
; https://github.com/tree-sitter/tree-sitter-html), unchanged, injecting
; `<script>` as javascript and `<style>` as css. Changes: drops upstream's
; Envoy pattern, which injects an `@task`'s text as bash, as its
; `#has-ancestor?` predicate isn't one fe-syntax has, and keeps the Alpine
; `:attr` pattern off `x-*` components and matches conditional attribute
; groups in the tag-scoped patterns (see there).

; tree-sitter-html 0.23.2, queries/injections.scm.

((script_element
  (raw_text) @injection.content)
 (#set! injection.language "javascript"))

((style_element
  (raw_text) @injection.content)
 (#set! injection.language "css"))

; tree-sitter-blade, queries/injections.scm.

; The code of {{ }}, {!! !!} and @php ... @endphp.
((php_only) @injection.content
  (#set! injection.language "php_only"))

; A directive's parameter: @if ($x), @foreach ($xs as $x).
((parameter) @injection.content
    (#set! injection.include-children)
    (#set! injection.language "php_only"))

; Livewire attributes
; <div wire:click="baz++">
(attribute
  (attribute_name) @_attr
    (#any-of? @_attr "wire:model"
      "wire:click"
      "wire:stream"
      "wire:text"
      "wire:show")
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#set! injection.language "javascript"))


; AlpineJS attributes
; <div x-data="{ foo: 'bar' }" x-init="baz()">
(attribute
  (attribute_name) @_attr
    (#match? @_attr "^x-[a-z]+")
    (#not-any-of? @_attr "x-teleport" "x-ref" "x-transition")
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#set! injection.language "javascript"))
; <div :foo="bar" @click="baz()">
; Changed: upstream's one pattern for both, "^[:@][a-z]+", also matched a
; component's `:attr`, which the Blade PHP attribute pattern below injects as
; php_only, so the same value was injected twice. Here a `:attr` is
; javascript only on a tag that isn't `x-*`.
;
; Changed, in the `:attr` pattern after the `@attr` one and in the two
; tag-scoped patterns below: an attribute may
; also sit in a conditional group in the tag, `@if($x) :foo="bar" @endif`,
; which the grammar makes an `attribute` holding the attributes, or in a group
; in a group. Upstream matched only a tag's direct attributes. Deeper nesting
; isn't matched (a query can't match a descendant at any depth).
(attribute
  (attribute_name) @_attr
    (#match? @_attr "^@[a-z]+")
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#set! injection.language "javascript"))
(element
  (_
    (tag_name) @_tag
    [
      (attribute
        (attribute_name) @_attr
        (quoted_attribute_value
          (attribute_value) @injection.content))
      (attribute
        (attribute
          (attribute_name) @_attr
          (quoted_attribute_value
            (attribute_value) @injection.content)))
      (attribute
        (attribute
          (attribute
            (attribute_name) @_attr
            (quoted_attribute_value
              (attribute_value) @injection.content))))
    ])
  (#not-match? @_tag "^x-[a-z]+")
  (#match? @_attr "^:[a-z]+")
  (#set! injection.language "javascript"))

; Blade escaped JS attributes
; <x-foo ::bar="baz" />
(element
  (_
    (tag_name) @_tag
    [
      (attribute
        (attribute_name) @_attr
        (quoted_attribute_value
          (attribute_value) @injection.content))
      (attribute
        (attribute
          (attribute_name) @_attr
          (quoted_attribute_value
            (attribute_value) @injection.content)))
      (attribute
        (attribute
          (attribute
            (attribute_name) @_attr
            (quoted_attribute_value
              (attribute_value) @injection.content))))
    ])
  (#match? @_tag "^x-[a-z]+")
  (#match? @_attr "^::[a-z]+")
  (#set! injection.language "javascript"))

; Blade PHP attributes
; <x-foo :bar="$baz" />
(element
  (_
    (tag_name) @_tag
    [
      (attribute
        (attribute_name) @_attr
        (quoted_attribute_value
          (attribute_value) @injection.content))
      (attribute
        (attribute
          (attribute_name) @_attr
          (quoted_attribute_value
            (attribute_value) @injection.content)))
      (attribute
        (attribute
          (attribute
            (attribute_name) @_attr
            (quoted_attribute_value
              (attribute_value) @injection.content))))
    ])
  (#match? @_tag "^x-[a-z]+")
  (#match? @_attr "^:[a-z]+")
  (#set! injection.language "php_only"))
