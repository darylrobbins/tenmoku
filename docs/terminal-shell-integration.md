# Shell integration in the terminal

Shells can tell the terminal two things with standard escape sequences:
where they are, and where each prompt starts. Tenmoku's terminal uses both
once your shell sends them. Tenmoku doesn't inject anything into your shell:
add the snippet for your shell below to its startup file.

## What Tenmoku does with them

- **OSC 7, the working directory** (`ESC ] 7 ; file://<host>/<path> BEL`,
  the path percent-encoded). `terminal.new` starts the new terminal in the
  directory the active terminal's shell last reported. If it reported none,
  or that path isn't a directory on this machine, the new terminal starts in
  the project root as before. The first terminal (`terminal.toggle`) always
  starts in the project root.
- **OSC 133, prompt marks** (`ESC ] 133 ; A BEL` where a prompt starts).
  **Terminal: Previous Prompt** (`terminal.previous_prompt`) scrolls the view
  so the previous prompt is its top row, and **Terminal: Next Prompt**
  (`terminal.next_prompt`) scrolls forward. Past the newest prompt it goes
  back to the bottom. The terminal remembers the prompt it last scrolled to
  until you type or the view returns to the bottom, so pressing the command
  again carries on from there. The commands are in the palette while a
  terminal has focus, and have no default keys yet: bind them in your keymap.
  Marks are kept for as long as their rows are in the scrollback and follow
  their lines when a resize rewraps them. Clearing the history (`clear`,
  `CSI 3 J`) drops the marks that were in it. Programs on the alternate screen (vim, less)
  can't set marks.

`133;B` (command start), `133;C` (output start) and `133;D;<status>`
(command end) are read but not used yet. The snippets send them anyway so
later features work without changing your setup.

Each snippet checks `TERM_PROGRAM`, which Tenmoku sets to `tenmoku` in its
terminals, so other terminals don't get these sequences.

## zsh

Add at the end of `~/.zshrc`, after anything that sets your prompt or adds
prompt hooks:

```zsh
if [[ $TERM_PROGRAM == tenmoku ]]; then
  # The working directory, percent-encoded, as OSC 7.
  __tenmoku_osc7() {
    local LC_ALL=C url= ch i
    for (( i = 1; i <= ${#PWD}; i++ )); do
      ch=${PWD[i]}
      if [[ $ch == [A-Za-z0-9/._~-] ]]; then
        url+=$ch
      else
        url+=$(printf '%%%02X' "'$ch")
      fi
    done
    printf '\e]7;file://%s%s\a' "$HOST" "$url"
  }
  # The end of the last command. It goes first among the precmd hooks, to
  # see the command's exit status, and passes the status on.
  __tenmoku_command_end() {
    local code=$?
    if [[ -n $__tenmoku_ran ]]; then
      printf '\e]133;D;%s\a' "$code"
      __tenmoku_ran=
    fi
    return $code
  }
  # The start of the prompt. It goes last, after any other hook's output.
  __tenmoku_prompt_start() {
    local code=$?
    printf '\e]133;A\a'
    return $code
  }
  # The start of the command's output.
  __tenmoku_preexec() {
    __tenmoku_ran=1
    printf '\e]133;C\a'
  }
  autoload -Uz add-zsh-hook
  precmd_functions=(__tenmoku_command_end $precmd_functions __tenmoku_prompt_start)
  add-zsh-hook preexec __tenmoku_preexec
  add-zsh-hook chpwd __tenmoku_osc7
  __tenmoku_osc7
  # The end of the prompt, where the command line starts.
  PS1+='%{'$'\e]133;B\a''%}'
fi
```

A prompt theme that rebuilds `PS1` before every prompt (powerlevel10k,
starship) drops the `133;B` at its end. Only `133;A` matters for now.

## bash

Add at the end of `~/.bashrc`, after anything that sets `PS1` or `PROMPT_COMMAND`
(`PS0` needs bash 4.4 or later):

```bash
if [ "$TERM_PROGRAM" = tenmoku ]; then
  # The working directory, percent-encoded, as OSC 7.
  __tenmoku_osc7() {
    local LC_ALL=C url= ch i
    for (( i = 0; i < ${#PWD}; i++ )); do
      ch=${PWD:i:1}
      case $ch in
        [A-Za-z0-9/._~-]) url+=$ch ;;
        *) printf -v ch '%%%02X' "'$ch"; url+=$ch ;;
      esac
    done
    printf '\e]7;file://%s%s\a' "$HOSTNAME" "$url"
  }
  # The end of the last command, and the directory. It runs first in
  # PROMPT_COMMAND, to see the command's exit status, and passes the
  # status on to the rest.
  __tenmoku_command_end() {
    local code=$?
    if [ -n "$__tenmoku_started" ]; then
      printf '\e]133;D;%s\a' "$code"
    fi
    __tenmoku_started=1
    __tenmoku_osc7
    return "$code"
  }
  # The start of the prompt. It runs last, after any other output.
  __tenmoku_prompt_start() {
    local code=$?
    printf '\e]133;A\a'
    return "$code"
  }
  # Around whatever runs there already: an array (bash 5.1 and later) or a
  # string.
  if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a"* ]]; then
    PROMPT_COMMAND=(__tenmoku_command_end "${PROMPT_COMMAND[@]}" __tenmoku_prompt_start)
  else
    PROMPT_COMMAND="__tenmoku_command_end"$'\n'"$PROMPT_COMMAND"$'\n'"__tenmoku_prompt_start"
  fi
  # The end of the prompt, where the command line starts.
  PS1+='\[\e]133;B\a\]'
  # The start of the command's output.
  PS0+='\e]133;C\a'
fi
```

## fish

fish 4.0 and later send OSC 133 themselves. Add OSC 7 in
`~/.config/fish/config.fish`:

```fish
if test "$TERM_PROGRAM" = tenmoku
    function __tenmoku_osc7 --on-variable PWD
        printf '\e]7;file://%s%s\a' $hostname (string escape --style=url -- $PWD)
    end
    __tenmoku_osc7
end
```

## Remote shells

A shell on another machine over ssh reports its own path, with that machine's
name as the host. Tenmoku doesn't check the host. It uses the path only when
it is a directory on this machine, so `terminal.new` from an ssh session
usually starts in the project root. If the remote shell has the snippet,
prompt marks work there as they do locally. The remote side needs
`TERM_PROGRAM` passed through (`SendEnv` and `AcceptEnv`) for the check to
pass.
