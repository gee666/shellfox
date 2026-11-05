/** Loaded after the user's PowerShell profile. Delegates its prompt without editing profile files. */
export function powershellDirectoryHook(): string {
  return `
if (-not (Get-Variable -Name ShellfoxOriginalPrompt -Scope Global -ErrorAction Ignore)) {
  $global:ShellfoxOriginalPrompt = (Get-Command prompt -CommandType Function -ErrorAction Stop).ScriptBlock
  function global:prompt {
    $shellfoxPrompt = & $global:ShellfoxOriginalPrompt
    $shellfoxExitVariable = Get-Variable -Name LASTEXITCODE -Scope Global -ErrorAction Ignore
    $shellfoxExit = if ($null -ne $shellfoxExitVariable) { $shellfoxExitVariable.Value } else { $null }
    try {
      if ($PWD.Provider.Name -eq 'FileSystem') {
        $shellfoxUri = [Uri]::new($PWD.ProviderPath).AbsoluteUri
        [Console]::Write(([char]27).ToString() + ']7;' + $shellfoxUri + ([char]7).ToString())
      } else {
        [Console]::Write(([char]27).ToString() + ']7;' + ([char]7).ToString())
      }
    } catch {
      # An unavailable console or non-filesystem provider must not break the user's prompt.
    } finally { if ($null -ne $shellfoxExitVariable) { $global:LASTEXITCODE = $shellfoxExit } }
    $shellfoxPrompt
  }
}
`;
}
