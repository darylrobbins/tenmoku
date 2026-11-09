lsp-server-initialize-timeout =
    { $seconds ->
        [one] RU-ONE { $name } { $seconds }
        [few] RU-FEW { $name } { $seconds }
        [many] RU-MANY { $name } { $seconds }
       *[other] RU-OTHER { $name } { $seconds }
    }
lsp-backlog-over-bytes =
    { $count ->
        [one] RU-ONE { $count }
        [few] RU-FEW { $count }
        [many] RU-MANY { $count }
       *[other] RU-OTHER { $count }
    }
