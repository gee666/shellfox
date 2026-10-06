# Loaded once by bash --rcfile ... -i. Keep the real shell, history and user rc behavior.
# No profile installation, prompt wrapper, executable command template or CWD interpolation.
if [[ -r "$HOME/.bashrc" ]]; then
  source "$HOME/.bashrc"
fi
if [[ -n ${SHELLFOX_HELPER:-} && -n ${SHELLFOX_TICKET:-} ]]; then
  "$SHELLFOX_HELPER" register --ticket "$SHELLFOX_TICKET" --shell-pid "$BASHPID" ||
    printf '%s\n' 'Shellfox registration failed; this Bash remains usable and monitoring is unknown.' >&2
fi
unset SHELLFOX_HELPER SHELLFOX_TICKET
