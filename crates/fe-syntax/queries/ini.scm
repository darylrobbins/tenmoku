; Ours. After the crate's query, which leaves values plain because the grammar
; has no value subtypes.
(setting_value) @string

; Setting names are data-file keys, like JSON's member names. Section names
; stay as the crate has them.
(setting
  (setting_name) @string.special.key)
