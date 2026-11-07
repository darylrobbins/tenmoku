" LazyVim's maps, loaded beneath your vimrc and [[vim.remap]] when
" vim.preset = "lazyvim", with <Space> as <leader> unless vim.leader is set.
" The vimrc subset fastedit reads; each <Cmd>fe:…<CR> names a fastedit
" command.

" find
nnoremap <leader>ff <Cmd>fe:quick_open.toggle<CR>
nnoremap <leader><space> <Cmd>fe:quick_open.toggle<CR>
nnoremap <leader>fr <Cmd>fe:workbench.open_recent<CR>
nnoremap <leader>fb <Cmd>fe:buffers.show<CR>
nnoremap <leader>fg <Cmd>fe:git.show_files<CR>
nnoremap <leader>fc <Cmd>fe:workbench.show_config_files<CR>
nnoremap <leader>fn <Cmd>fe:workbench.new_file<CR>
nnoremap <leader>fp <Cmd>fe:workbench.open_folder<CR>
nnoremap <leader>ft <Cmd>fe:terminal.toggle<CR>
nnoremap <leader>fT <Cmd>fe:terminal.toggle<CR>

" search
nnoremap <leader>/ <Cmd>fe:search.show<CR>
nnoremap <leader>sg <Cmd>fe:search.show<CR>
nnoremap <leader>sr <Cmd>fe:search.show_replace<CR>
nnoremap <leader>ss <Cmd>fe:outline.show<CR>
nnoremap <leader>sS <Cmd>fe:nav.workspace_symbols<CR>
nnoremap <leader>: <Cmd>fe:command_palette.toggle<CR>
nnoremap <leader>sc <Cmd>fe:command_palette.toggle<CR>
nnoremap <leader>sk <Cmd>fe:keymaps.search<CR>
" No :help to search: the Keyboard Shortcuts editor is the nearest reference.
nnoremap <leader>sh <Cmd>fe:workbench.open_keyboard_shortcuts<CR>
nnoremap <leader>sd <Cmd>fe:diagnostics.search_document<CR>
nnoremap <leader>sD <Cmd>fe:diagnostics.search_workspace<CR>
nnoremap <leader>sb <Cmd>fe:search.buffer_lines<CR>
nnoremap <leader>st <Cmd>fe:todo.list<CR>

" explorer, buffers
nnoremap <leader>e <Cmd>fe:dock.show_project<CR>
nnoremap <leader>E <Cmd>fe:dock.show_project<CR>
nnoremap <leader>bd <Cmd>fe:workbench.close_tab<CR>
nnoremap <leader>bb <Cmd>fe:buffers.show<CR>
nnoremap <leader>, <Cmd>fe:buffers.show<CR>

" code
nnoremap <leader>cr <Cmd>fe:lsp.rename<CR>
nnoremap <leader>ca <Cmd>fe:lsp.code_actions<CR>
nnoremap <leader>cf <Cmd>fe:lsp.format_document<CR>
xnoremap <leader>cf <Cmd>fe:lsp.format_selection<CR>
nnoremap <leader>cs <Cmd>fe:outline.show<CR>
nnoremap <leader>co <Cmd>fe:lsp.organize_imports<CR>
nnoremap <leader>cd <Cmd>fe:diagnostics.next<CR>

" git
nnoremap <leader>gg <Cmd>fe:git.show_changes<CR>
nnoremap <leader>gs <Cmd>fe:git.show_changes<CR>
nnoremap <leader>gb <Cmd>fe:git.toggle_blame_annotations<CR>
nnoremap <leader>ghs <Cmd>fe:git.stage_hunk<CR>
nnoremap <leader>ghr <Cmd>fe:git.revert_hunk<CR>

" test
nnoremap <leader>tr <Cmd>fe:tests.run_at_cursor<CR>
nnoremap <leader>tt <Cmd>fe:tests.run_file<CR>
nnoremap <leader>tl <Cmd>fe:tests.rerun_last<CR>

" diagnostics/quickfix
nnoremap <leader>xx <Cmd>fe:problems.show<CR>
nnoremap <leader>xq <Cmd>fe:quickfix.toggle<CR>
nnoremap <leader>xl <Cmd>fe:location.toggle<CR>
nnoremap ]q <Cmd>fe:quickfix.next<CR>
nnoremap [q <Cmd>fe:quickfix.previous<CR>
nnoremap <leader>xt <Cmd>fe:todo.list<CR>

" todo comments, as todo-comments.nvim jumps between them
nnoremap ]t <Cmd>fe:todo.next<CR>
nnoremap [t <Cmd>fe:todo.previous<CR>

" windows
nnoremap <leader>- <Cmd>fe:pane.split_down<CR>
nnoremap <leader><Bar> <Cmd>fe:pane.split_right<CR>
nnoremap <leader>wd <Cmd>fe:pane.close<CR>
nnoremap <leader>ww <Cmd>fe:pane.focus_next<CR>
nnoremap <leader>w= <Cmd>fe:pane.equalize<CR>
nnoremap <C-h> <Cmd>fe:pane.focus_left<CR>
nnoremap <C-j> <Cmd>fe:pane.focus_down<CR>
nnoremap <C-k> <Cmd>fe:pane.focus_up<CR>
nnoremap <C-l> <Cmd>fe:pane.focus_right<CR>

" harpoon
nnoremap <leader>H <Cmd>fe:workbench.harpoon_add<CR>
nnoremap <leader>h <Cmd>fe:workbench.harpoon_toggle_list<CR>
nnoremap <leader>1 <Cmd>fe:workbench.harpoon_jump1<CR>
nnoremap <leader>2 <Cmd>fe:workbench.harpoon_jump2<CR>
nnoremap <leader>3 <Cmd>fe:workbench.harpoon_jump3<CR>
nnoremap <leader>4 <Cmd>fe:workbench.harpoon_jump4<CR>
nnoremap <leader>5 <Cmd>fe:workbench.harpoon_jump5<CR>

" quit, ui
nnoremap <leader>qq <Cmd>fe:app.quit<CR>
nnoremap <leader>uw <Cmd>fe:editor.toggle_soft_wrap<CR>

" which-key's names for the groups above, as LazyVim's which-key spec names
" them: only prefixes the preset has maps beneath. Each icon= names a Lucide
" icon drawn beside the group, after LazyVim's.
fegroup <leader>b buffer icon=files
fegroup <leader>c code icon=code
fegroup <leader>f file/find icon=search
fegroup <leader>g git icon=git-branch
fegroup <leader>gh hunks icon=file-diff
fegroup <leader>q quit/session icon=log-out
fegroup <leader>s search icon=search
fegroup <leader>t test icon=flask-conical
fegroup <leader>u ui icon=toggle-right
fegroup <leader>w windows icon=app-window
fegroup <leader>x diagnostics/quickfix icon=triangle-alert

" Unmapped LazyVim keys: fastedit has no command for them.
"   <leader>l   lazy
"   <leader>L   changelog
"   <leader>K   keywordprg
"   <leader>cm  mason
"   <leader>ur  redraw
"   <leader>ud  toggle diagnostics
"   <leader>un  dismiss notifications
"   <leader>wm  zoom
"   <leader>bo  delete other buffers
"   <leader><tab>  tab pages
"   <leader>qs  sessions
