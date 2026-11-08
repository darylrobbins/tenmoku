; Empty on purpose. nvim-treesitter's HLSL injections (inherited from C++ and
; C) target languages fastedit doesn't have (comment, doxygen, re2c), inject
; macro bodies into the host language itself (`injection.self`, which
; fe-syntax doesn't support), or format strings of the C library's printf
; family, which HLSL lacks but for a debugging-only `printf`; that one's
; strings are left plain.
