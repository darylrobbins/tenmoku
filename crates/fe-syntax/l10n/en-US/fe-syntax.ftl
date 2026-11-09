# fe-syntax's messages: why a grammar extension failed to load.

syntax-grammar-file-error = { $file }: { $error }
syntax-grammar-too-large =
    { $bytes ->
        [one] { $file }: { $bytes } bytes; a grammar's files may have at most { $max }
       *[other] { $file }: { $bytes } bytes; a grammar's files may have at most { $max }
    }
syntax-grammar-deadline = { $file }: still running at its { $deadline } deadline, so stopped
syntax-grammar-clash =
    { $kind ->
        [extension] { $language }: the extension { $value } is the built-in { $built_in }'s
        [file-name] { $language }: the file name { $value } is the built-in { $built_in }'s
        [interpreter] { $language }: the interpreter { $value } is the built-in { $built_in }'s
       *[name] { $language }: its name is the built-in { $built_in }'s
    }
syntax-grammar-abi = ABI version { $abi }; this build loads { $min } to { $max }
syntax-grammar-no-ticker = no thread to enforce grammars' deadlines: { $error }
