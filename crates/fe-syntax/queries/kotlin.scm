; Goes after tree-sitter-kotlin-sg's highlights.scm for Kotlin.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 3e1f54c1ce0a86959313f1cb7f60262ea57aaa23,
; path runtime/queries/kotlin/highlights.scm, license Apache-2.0.
; Changes: only the patterns below are taken, and `#lua-match?` becomes
; `#match?` with the same pattern.
;
; The crate's query is an older copy of the same file. It anchors a function's
; name to the first child, so annotated or modified functions (`private fun f`)
; lost it, it colours the whole `package` line as a module, and it has no
; `companion` keyword or SCREAMING_CASE constants. Later patterns win over
; earlier ones on the same node.
;
; The constant patterns are broad, so they would also take over calls such as
; `TODO()` and `Foo.BAR()`. The crate's call patterns are therefore restated
; after them, unchanged, and a call keeps its function highlight.

((simple_identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]*$"))

(_
  (navigation_suffix
    (simple_identifier) @constant
    (#match? @constant "^[A-Z][A-Z0-9_]*$")))

(package_header
  "package" @keyword
  .
  (identifier
    (simple_identifier) @module))

(function_declaration
  (simple_identifier) @function)

(companion_object
  "companion" @keyword)

; function()
(call_expression
	. (simple_identifier) @function)

; object.function() or object.property.function()
(call_expression
	(navigation_expression
		(navigation_suffix
			(simple_identifier) @function) . ))

(call_expression
	. (simple_identifier) @function.builtin
    (#any-of? @function.builtin
		"arrayOf"
		"arrayOfNulls"
		"byteArrayOf"
		"shortArrayOf"
		"intArrayOf"
		"longArrayOf"
		"ubyteArrayOf"
		"ushortArrayOf"
		"uintArrayOf"
		"ulongArrayOf"
		"floatArrayOf"
		"doubleArrayOf"
		"booleanArrayOf"
		"charArrayOf"
		"emptyArray"
		"mapOf"
		"setOf"
		"listOf"
		"emptyMap"
		"emptySet"
		"emptyList"
		"mutableMapOf"
		"mutableSetOf"
		"mutableListOf"
		"print"
		"println"
		"error"
		"TODO"
		"run"
		"runCatching"
		"repeat"
		"lazy"
		"lazyOf"
		"enumValues"
		"enumValueOf"
		"assert"
		"check"
		"checkNotNull"
		"require"
		"requireNotNull"
		"with"
		"synchronized"
))
