/** Generated hooks only query names. Registry output is data, never shell source. */
const sessionOperations = ['list', 'create', 'inspect', 'rename', 'pin', 'env', 'settle', 'unsettle', 'delete', 'stats', 'processes', 'focus', 'refresh', 'clear-error'];
const terminalOperations = ['list', 'create', 'rename', 'reorder', 'close', 'attach', 'write'];
const subcommands: Record<string, string[]> = {
  host: ['run', 'status', 'stop', 'service', 'keep-awake'], network: ['enable', 'disable', 'login', 'status'],
  machines: ['list', 'connect', 'disconnect', 'forget'], approvals: ['list', 'approve', 'revoke', 'reject'],
  session: sessionOperations, ss: sessionOperations, sessions: sessionOperations,
  terminals: terminalOperations, tm: terminalOperations,
  update: ['--check'], settings: ['read', 'update'], profiles: ['list'], path: ['home', 'resolve', 'complete', 'create'], remote: ['api'],
};
const commands = [
  ['host', 'Run or manage the host'], ['session', 'Manage sessions'],
  ['terminals', 'Manage and attach terminals'], ['network', 'Manage Tailscale networking'],
  ['machines', 'Manage remote machines'], ['approvals', 'Manage access approvals'],
  ['settings', 'Read or update settings'], ['profiles', 'List terminal profiles'],
  ['path', 'Use host path helpers'], ['commands', 'Run or manage script commands'],
  ['ssh', 'Connect to a saved SSH profile'], ['api', 'Call a validated manager method'],
  ['remote', 'Call remote management'], ['update', 'Update Shellfox'], ['version', 'Print the version'],
  ['help', 'Show help'], ['completion', 'Print shell completion'],
  ['--host', 'Select a local or remote host'], ['--json', 'Print JSON'], ['--yes', 'Confirm destructive operations'],
] as const;
const operations = [
  ['--run', 'Run a registered command'],
  ['--register', 'Register a name and script path'],
  ['--ls', 'List registered commands'], ['--list', 'List registered commands'],
  ['--remove', 'Remove a registered command'], ['--rm', 'Remove a registered command'],
  ['--unregister', 'Remove a registered command'],
  ['--rename', 'Rename a registered command'],
  ['--help', 'Show custom command help'], ['-h', 'Show custom command help'],
] as const;
const nameOperations = ['--run', '--remove', '--rm', '--unregister', '--rename'];
const psQuote = (value: string) => "'" + value.replaceAll("'", "''") + "'";

export function powershellCompletion(): string {
  const entries = (items: ReadonlyArray<readonly [string, string]>) => items.map(([name, hint]) => `@(${psQuote(name)},${psQuote(hint)})`).join(',');
  return `# Shellfox completion. Safe to load more than once; no profile edits.
Register-ArgumentCompleter -Native -CommandName shellfox,shellfox.ps1,shellfox.cmd -ScriptBlock {
  param($wordToComplete, $commandAst, $cursorPosition)
  # Use only completed arguments before the cursor, including mid-line completion.
  $before=@($commandAst.CommandElements | Select-Object -Skip 1 | Where-Object { $_.Extent.EndOffset -lt $cursorPosition })
  $position=$before.Count + 1
  $items=@()
  if ($position -eq 1) { $items=@(${entries(commands)}) }
  elseif ($before[0].Extent.Text -notmatch '^(?i:cmd|commands)$') {
    if ($position -eq 2) {
      $namespace=$before[0].Extent.Text
      switch ($namespace) {
${Object.entries(subcommands).map(([name, values]) => `        ${psQuote(name)} { $items=@(${entries(values.map(value => [value, value]))}) }`).join('\n')}
      }
    }
  }
  elseif ($position -eq 2) { $items=@(${entries(operations)}) }
  elseif ($position -ne 3 -or $before[1].Extent.Text -notin @(${nameOperations.map(psQuote).join(',')})) { return }
  foreach ($item in $items) {
    if ($item[0].StartsWith($wordToComplete,[StringComparison]::OrdinalIgnoreCase)) {
      [System.Management.Automation.CompletionResult]::new($item[0],$item[0],'ParameterValue',$item[1])
    }
  }
  if ($before.Count -gt 0 -and $before[0].Extent.Text -match '^(?i:cmd|commands)$' -and $position -ge 2 -and -not $wordToComplete.StartsWith('-')) {
    $savedExit=$global:LASTEXITCODE
    try {
      $commandName=$commandAst.CommandElements[0].Value
      $names=@(& $commandName cmd --complete ([string]$wordToComplete) 2>$null)
      if ($LASTEXITCODE -eq 0) {
        foreach ($name in $names) {
          if ($name -cmatch '^[A-Za-z][A-Za-z0-9_-]{0,63}$' -and $name.StartsWith($wordToComplete,[StringComparison]::OrdinalIgnoreCase)) {
            [System.Management.Automation.CompletionResult]::new($name,$name,'ParameterValue','Run or select registered command')
          }
        }
      }
    } catch { # Missing CLI or unreadable registry: no dynamic suggestions.
    } finally { $global:LASTEXITCODE=$savedExit }
  }
}
`;
}

export function bashCompletion(): string {
  return `# Shellfox completion for Bash 3.2+. No bash-completion dependency.
_shellfox_complete() {
  local current="\${COMP_WORDS[COMP_CWORD]}" name output candidate
  COMPREPLY=()
  if [[ $COMP_CWORD -eq 1 ]]; then
    for candidate in ${commands.map(([name]) => name).join(' ')}; do
      [[ $candidate == "$current"* ]] && COMPREPLY+=("$candidate")
    done
    return 0
  fi
  if [[ \${COMP_WORDS[1]} != cmd && \${COMP_WORDS[1]} != commands ]]; then
    [[ $COMP_CWORD -eq 2 ]] || return 0
    case "\${COMP_WORDS[1]}" in
${Object.entries(subcommands).map(([name, values]) => `      ${name}) output='${values.join(' ')}' ;;`).join('\n')}
      *) return 0 ;;
    esac
    for candidate in $output; do [[ $candidate == "$current"* ]] && COMPREPLY+=("$candidate"); done
    return 0
  fi
  if [[ $COMP_CWORD -eq 2 ]]; then
    for candidate in ${operations.map(([name]) => name).join(' ')}; do
      [[ $candidate == "$current"* ]] && COMPREPLY+=("$candidate")
    done
  elif [[ $COMP_CWORD -eq 3 ]]; then
    case "\${COMP_WORDS[2]}" in ${nameOperations.join('|')}) ;; *) return 0 ;; esac
  else
    return 0
  fi
  [[ $current != -* ]] || return 0
  output=$("\${COMP_WORDS[0]}" cmd --complete "$current" 2>/dev/null) || return 0
  while IFS= read -r name; do
    name=\${name%$'\\r'}
    [[ $name =~ ^[A-Za-z][A-Za-z0-9_-]{0,63}$ ]] && COMPREPLY+=("$name")
  done <<< "$output"
  return 0
}
complete -F _shellfox_complete shellfox
`;
}

export function zshCompletion(): string {
  return `# Shellfox completion. Load after compinit.
_shellfox_complete() {
  local output name pattern='^[A-Za-z][A-Za-z0-9_-]{0,63}$'
  local -a names subcommands
  if (( CURRENT == 2 )); then
    subcommands=(${commands.map(([name, hint]) => `'${name}:${hint}'`).join(' ')})
    _describe 'Shellfox command' subcommands
    return 0
  fi
  if [[ $words[2] != cmd && $words[2] != commands ]]; then
    (( CURRENT == 3 )) || return 0
    case "$words[2]" in
${Object.entries(subcommands).map(([name, values]) => `      ${name}) subcommands=(${values.join(' ')}) ;;`).join('\n')}
      *) return 0 ;;
    esac
    compadd -- "\${subcommands[@]}"
    return 0
  fi
  if (( CURRENT == 3 )); then
    local -a operations
    operations=(${operations.map(([name, hint]) => `'${name}:${hint}'`).join(' ')})
    _describe 'Custom command operation' operations
  elif (( CURRENT == 4 )); then
    case "$words[3]" in ${nameOperations.join('|')}) ;; *) return 0 ;; esac
  else
    return 0
  fi
  [[ $PREFIX != -* ]] || return 0
  output=$("$words[1]" cmd --complete "$PREFIX" 2>/dev/null) || return 0
  for name in "\${(@f)output}"; do
    name=\${name%$'\\r'}
    # Names are ASCII identifiers. Do not pass arbitrary endpoint text to compadd.
    if [[ $name =~ $pattern ]]; then names+=("$name"); fi
  done
  (( \${#names} )) && compadd -Q -M 'm:{a-zA-Z}={A-Za-z}' -X 'Registered command' -- "\${names[@]}"
  return 0
}
(( $+functions[compdef] )) && compdef _shellfox_complete shellfox
`;
}

export const COMPLETION_HELP = `Usage: shellfox completion <powershell|bash|zsh>
Print a shell completion hook. Does not edit shell profiles.
See docs/cli.md for loading instructions.
`;
export function completionScript(shell: string): string | null {
  switch (shell) {
    case 'powershell': return powershellCompletion();
    case 'bash': return bashCompletion();
    case 'zsh': return zshCompletion();
    default: return null;
  }
}
/** Kept separate from custom-command dispatch: printing hooks never reads the registry. */
export function dispatchCompletion(argv: string[], output = process.stdout, error = process.stderr): number | null {
  if (argv[0] !== 'completion') return null;
  if (argv.length === 1 || argv.length === 2 && ['--help', '-h', 'help'].includes(argv[1])) {
    output.write(COMPLETION_HELP); return 0;
  }
  const script = argv.length === 2 ? completionScript(argv[1]) : null;
  if (script === null) { error.write(COMPLETION_HELP); return 2; }
  output.write(script); return 0;
}
