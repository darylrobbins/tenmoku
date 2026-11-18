# T682: Language Server Log for a server that never started

The log branch of `run_server_op` now opens a read-only tab titled
"<name> Log" (like `debug_show_log`), reused per (name, root) with fresh text.
With no server output it shows the server, its root and its status text.

C4, real Linux app (python project, no pyright-langserver on PATH, clicking
the red status item): see `9h44rnbgtz-lsp-log-stopped.png`. The tab is
"pyright Log" and says "Status: pyright: stopped: pyright not found".

Known: the breadcrumb row above the text still reads "untitled" (the same for
the debug and audit log tabs); it comes from the pane, outside this card's scope.
