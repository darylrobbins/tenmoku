git-switch-would-overwrite =
    { $count ->
        [one] RU-ONE { $count }
        [few] RU-FEW { $count }
        [many] RU-MANY { $count }
       *[other] RU-OTHER { $count }
    }
