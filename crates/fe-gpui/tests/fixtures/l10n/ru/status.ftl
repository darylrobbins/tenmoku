# A fake Russian, for tests/l10n_status.rs: Russian's plural categories
# (one for 1 and 21, few for 2-4, many for 5-20), with marked text that puts
# the count first, as no appended fragment could.

gpui-status-cursor = ⟦{ $line }:{ $column }⟧
gpui-status-cursor-selections =
    { $count ->
        [one] ⟦one: { $count } carets, { $line }:{ $column }⟧
        [few] ⟦few: { $count } carets, { $line }:{ $column }⟧
        [many] ⟦many: { $count } carets, { $line }:{ $column }⟧
       *[other] ⟦other: { $count } carets, { $line }:{ $column }⟧
    }
gpui-status-cursor-selected =
    { $count ->
        [one] ⟦one: { $count } chars, { $line }:{ $column }⟧
        [few] ⟦few: { $count } chars, { $line }:{ $column }⟧
        [many] ⟦many: { $count } chars, { $line }:{ $column }⟧
       *[other] ⟦other: { $count } chars, { $line }:{ $column }⟧
    }
