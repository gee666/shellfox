# PSReadLine reads no history-path variable by itself.
if ($env:PSREADLINE_HISTORYFILE) {
  if (-not (Get-Module PSReadLine)) { Import-Module PSReadLine -ErrorAction Ignore }
  if (Get-Module PSReadLine) { Set-PSReadLineOption -HistorySavePath $env:PSREADLINE_HISTORYFILE }
}

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
