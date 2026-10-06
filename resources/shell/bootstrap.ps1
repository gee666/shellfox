param([Parameter(Mandatory = $true)][string]$TicketPath)
# Runs once in the real interactive shell after its normal profiles. No prompt/profile changes.
try {
    $ticket = [IO.File]::ReadAllText($TicketPath) | ConvertFrom-Json
    Set-Location -LiteralPath $ticket.cwd -ErrorAction Stop
    if ($ticket.titleMarker) { $Host.UI.RawUI.WindowTitle = $ticket.titleMarker }
    $ack = & $ticket.helperPath register --ticket $TicketPath --shell-pid $PID
    # One-shot metadata is captured, not printed to the terminal.
    if ($LASTEXITCODE -ne 0) {
        Write-Warning 'Shellfox registration failed. This shell remains usable; monitoring is unknown.'
    }
} catch {
    # Do not print the ticket or exception details, which might contain launch secrets.
    Write-Warning 'Shellfox bootstrap failed. This shell remains usable; monitoring is unknown.'
}
Remove-Variable ticket,ack -ErrorAction SilentlyContinue
