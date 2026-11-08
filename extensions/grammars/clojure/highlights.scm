;; Based on tree-sitter-clojure's queries/highlights.scm (CC0-1.0, same licence
;; as the grammar) at commit e43eff80d17cf34852dcd92ca5e6986d23a7040f:
;; https://github.com/sogaiu/tree-sitter-clojure/blob/e43eff80d17cf34852dcd92ca5e6986d23a7040f/queries/highlights.scm
;; Changes: this header; added brackets, definitions, calls, types, special
;; forms and metadata, written for this grammar's node names. Last pattern wins.

;; Literals

(num_lit) @number

[
  (char_lit)
  (str_lit)
] @string

(regex_lit) @string.special

[
 (bool_lit)
 (nil_lit)
] @constant.builtin

(kwd_lit) @constant

;; Comments

(comment) @comment
(dis_expr) @comment

;; Treat quasiquotation as operators for the purpose of highlighting.

[
 "'"
 "`"
 "~"
 "@"
 "~@"
 "#'"
 "#"
 "^"
] @operator

[ "(" ")" "[" "]" "{" "}" ] @punctuation.bracket

;; Symbols

(sym_lit (sym_name) @variable)
(sym_ns) @module

;; Metadata: type hints and tags
(meta_lit value: (sym_lit (sym_name) @type))
(tagged_or_ctor_lit tag: (sym_lit (sym_name) @type))

;; Capitalised or dotted symbols name classes
((sym_lit (sym_name) @type)
 (#match? @type "^[A-Z][A-Za-z0-9_]*$"))

;; Calls: the head of a list
(list_lit
  .
  (sym_lit (sym_name) @function))
(anon_fn_lit
  .
  (sym_lit (sym_name) @function))
(list_lit
  .
  (sym_lit (sym_name) @function.macro)
  (#any-of? @function.macro
    "defn" "defn-" "defmacro" "defmulti" "defmethod" "defprotocol" "defrecord"
    "deftype" "definterface" "defonce" "def" "ns" "fn" "fn*" "let" "letfn" "loop"
    "binding" "when" "when-not" "when-let" "when-some" "when-first" "if-let"
    "if-some" "if-not" "cond" "condp" "case" "doseq" "dotimes" "for" "doto"
    "->" "->>" "as->" "some->" "some->>" "cond->" "cond->>" "and" "or" "comment"
    "try" "catch" "finally" "throw" "defstruct" "extend-type" "extend-protocol"
    "reify" "proxy" "with-open" "with-redefs" "with-local-vars" "delay" "future"
    "lazy-seq" "import" "require" "use" "refer" "declare" "assert" "time"))
(anon_fn_lit
  .
  (sym_lit (sym_name) @function.macro)
  (#any-of? @function.macro
    "defn" "defn-" "defmacro" "defmulti" "defmethod" "defprotocol" "defrecord"
    "deftype" "definterface" "defonce" "def" "ns" "fn" "fn*" "let" "letfn" "loop"
    "binding" "when" "when-not" "when-let" "when-some" "when-first" "if-let"
    "if-some" "if-not" "cond" "condp" "case" "doseq" "dotimes" "for" "doto"
    "->" "->>" "as->" "some->" "some->>" "cond->" "cond->>" "and" "or" "comment"
    "try" "catch" "finally" "throw" "defstruct" "extend-type" "extend-protocol"
    "reify" "proxy" "with-open" "with-redefs" "with-local-vars" "delay" "future"
    "lazy-seq" "import" "require" "use" "refer" "declare" "assert" "time"))
(list_lit
  .
  (sym_lit (sym_name) @keyword)
  (#any-of? @keyword
    "if" "do" "quote" "var" "recur" "set!" "monitor-enter" "monitor-exit" "new"
    "." "&"))

;; Definitions name the second element
(anon_fn_lit
  .
  (sym_lit (sym_name) @keyword)
  (#any-of? @keyword
    "if" "do" "quote" "var" "recur" "set!" "monitor-enter" "monitor-exit" "new"
    "." "&"))

;; Definitions name the second element
(list_lit
  .
  (sym_lit (sym_name) @_def)
  .
  (sym_lit (sym_name) @function)
  (#any-of? @_def "defn" "defn-" "defmacro" "defmulti" "defmethod"))
(anon_fn_lit
  .
  (sym_lit (sym_name) @_def)
  .
  (sym_lit (sym_name) @function)
  (#any-of? @_def "defn" "defn-" "defmacro" "defmulti" "defmethod"))
(list_lit
  .
  (sym_lit (sym_name) @_def)
  .
  (sym_lit (sym_name) @type)
  (#any-of? @_def "defprotocol" "defrecord" "deftype" "definterface"))
(anon_fn_lit
  .
  (sym_lit (sym_name) @_def)
  .
  (sym_lit (sym_name) @type)
  (#any-of? @_def "defprotocol" "defrecord" "deftype" "definterface"))
(list_lit
  .
  (sym_lit (sym_name) @_def)
  .
  (sym_lit (sym_name) @variable)
  (#any-of? @_def "def" "defonce"))
(anon_fn_lit
  .
  (sym_lit (sym_name) @_def)
  .
  (sym_lit (sym_name) @variable)
  (#any-of? @_def "def" "defonce"))
(list_lit
  .
  (sym_lit (sym_name) @_ns)
  .
  (sym_lit (sym_name) @module)
  (#eq? @_ns "ns"))
(anon_fn_lit
  .
  (sym_lit (sym_name) @_ns)
  .
  (sym_lit (sym_name) @module)
  (#eq? @_ns "ns"))

;; Interop and rest args
((sym_lit (sym_name) @variable.builtin)
 (#any-of? @variable.builtin "&" "*ns*" "*1" "*2" "*3" "*e"))

((sym_lit (sym_name) @variable.parameter)
 (#match? @variable.parameter "^%[0-9&]*$"))

;; Discarded forms: every node inside, to a fixed depth, is a comment, since a
;; nested capture beats the enclosing one. Must stay last.
(dis_expr _ @comment)
(dis_expr (_ _ @comment))
(dis_expr (_ (_ _ @comment)))
(dis_expr (_ (_ (_ _ @comment))))
(dis_expr (_ (_ (_ (_ _ @comment)))))
(dis_expr (_ (_ (_ (_ (_ _ @comment))))))
(dis_expr (_ (_ (_ (_ (_ (_ _ @comment)))))))
(dis_expr (_ (_ (_ (_ (_ (_ (_ _ @comment))))))))
(dis_expr (_ (_ (_ (_ (_ (_ (_ (_ _ @comment)))))))))
(dis_expr (_ (_ (_ (_ (_ (_ (_ (_ (_ _ @comment))))))))))
(dis_expr (_ (_ (_ (_ (_ (_ (_ (_ (_ (_ _ @comment)))))))))))
(dis_expr (_ (_ (_ (_ (_ (_ (_ (_ (_ (_ (_ _ @comment))))))))))))
