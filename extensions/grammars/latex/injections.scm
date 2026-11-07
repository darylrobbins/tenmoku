;; From nvim-treesitter's runtime/queries/latex/injections.scm at
;; 2c30e515ebe79037ab8d15f7e59c0e2690f50626 (https://github.com/nvim-treesitter/nvim-treesitter),
;; Apache-2.0 licence (see LICENSE-APACHE).
;;
;; Changes: the injections into `comment` (not a language here) and the
;; `luacode` and `asy` environments (nodes this grammar doesn't have) are dropped.

(pycode_environment
  code: (source_code) @injection.content
  (#set! injection.language "python"))

(sagesilent_environment
  code: (source_code) @injection.content
  (#set! injection.language "python"))

(sageblock_environment
  code: (source_code) @injection.content
  (#set! injection.language "python"))

(minted_environment
  (begin
    language: (curly_group_text
      (text) @injection.language))
  (source_code) @injection.content)
