# Shellfox tab startup. The tab starts zsh with ZDOTDIR set to this file's
# temporary folder. ZDOTDIR is put back first, so the user's own files run as
# usual, and the folder is removed. `shellfox` completion then loads at the
# first prompt that has compinit's compdef, unless a completer already exists.
'builtin' 'typeset' __shellfox_hook_dir=$ZDOTDIR
if [[ -n ${SHELLFOX_ZDOTDIR+x} ]]; then
  'builtin' 'export' ZDOTDIR=$SHELLFOX_ZDOTDIR
  'builtin' 'unset' SHELLFOX_ZDOTDIR
else
  'builtin' 'unset' ZDOTDIR
fi
if [[ ${__shellfox_hook_dir:t} == shellfox-zsh-* && -f $__shellfox_hook_dir/.zshenv ]]; then
  'command' 'rm' -rf -- $__shellfox_hook_dir
fi
'builtin' 'unset' __shellfox_hook_dir
if [[ -o interactive ]]; then
  __shellfox_completion() {
    'builtin' 'emulate' -L zsh
    (( $+functions[compdef] )) || return 0
    precmd_functions=(${precmd_functions:#__shellfox_completion})
    'builtin' 'unfunction' __shellfox_completion
    (( $+_comps[shellfox] )) && return 0
@COMPLETION@
  }
  'builtin' 'typeset' -ga precmd_functions
  precmd_functions+=(__shellfox_completion)
fi
[[ ! -r ${ZDOTDIR:-$HOME}/.zshenv ]] || 'builtin' 'source' -- ${ZDOTDIR:-$HOME}/.zshenv
