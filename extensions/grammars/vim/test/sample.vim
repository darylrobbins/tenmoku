" Sample vimrc, in the style of Tenmoku's own config.
scriptencoding utf-8
set nocompatible
set number relativenumber
set tabstop=4 shiftwidth=4 expandtab
set listchars=tab:>-,trail:.
let mapleader = " "
let g:sample_enabled = v:true
let g:sample_count = 42
let g:sample_ratio = 0.75
let s:names = ['alpha', "beta\tgamma", 'it''s']
let s:table = {'one': 1, 'two': 2}
const s:LIMIT = 100

filetype plugin indent on
syntax enable
colorscheme default
highlight Comment ctermfg=gray guifg=#808080
highlight link SampleGroup Special

nnoremap <silent> <leader>w :write<CR>
inoremap <expr> <Tab> pumvisible() ? "\<C-n>" : "\<Tab>"
map <buffer> <nowait> gs :echo 'hello'<CR>

" Join a list into a string.
function! s:Join(items, sep) abort
  let l:result = ''
  for l:item in a:items
    if l:result ==# ''
      let l:result = l:item
    elseif len(l:item) > 0
      let l:result .= a:sep . l:item
    endif
  endfor
  return l:result
endfunction

function! SampleCount(...) abort
  let l:total = 0
  let l:i = 0
  while l:i < a:0
    let l:total += a:000[l:i]
    let l:i += 1
  endwhile
  try
    echo 10 / l:total
  catch /E\d\+/
    echohl ErrorMsg
    echomsg "division failed"
    echohl None
  finally
    unlet! l:i
  endtry
  return l:total
endfunction

command! -nargs=? SampleRun call s:Join(split(<q-args>), ',')

augroup SampleGroup
  autocmd!
  autocmd BufWritePre *.vim call SampleCount(1, 2, 3)
  autocmd FileType markdown setlocal spell
augroup END

lua << EOF
local name = "tenmoku"
print(name)
EOF

if has('gui_running') && !empty($DISPLAY)
  set guifont=Monospace\ 12
endif
