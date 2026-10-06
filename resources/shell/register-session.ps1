param([Parameter(Mandatory = $true)][string]$TicketPath)
# Explicit one-shot integration in the actual selected interactive PowerShell.
# This script does not change CWD, profiles, prompt, WT settings or process lifetime.
try {
    $ticket = [IO.File]::ReadAllText($TicketPath) | ConvertFrom-Json
    if ($ticket.kind -ne 'adopt' -or !$env:WT_SESSION) { throw 'Unsupported terminal' }
    $Host.UI.RawUI.WindowTitle = $ticket.titleMarker
    $result = & $ticket.helperPath register --ticket $TicketPath --shell-pid $PID
    if ($LASTEXITCODE -ne 0) { throw 'Registration rejected' }
    $registration = $result | ConvertFrom-Json
    $Host.UI.RawUI.WindowTitle = 'SHELLFOX:' + $registration.sessionId + ':' + $registration.tabId
    Write-Host 'This PowerShell is registered to the selected Shellfox session. Re-run registration in the destination after moving this tab.'
} catch {
    Write-Warning 'Registration was not confirmed. Select the intended tab/window and prepare another ticket. If WT suppresses application titles, manually rename this tab to the supplied marker before running the command. No membership was guessed.'
}
Remove-Variable ticket,result,registration -ErrorAction SilentlyContinue
