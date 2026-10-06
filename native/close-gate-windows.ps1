param([Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$project=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$reportFile=[IO.Path]::GetFullPath($ReportPath)
if(!$reportFile.StartsWith((Join-Path $project 'tmp')+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Gate report must be in project tmp/.'}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class GateClose {
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint message,IntPtr w,IntPtr l);
}
'@
$r=[IO.File]::ReadAllText($reportFile)|ConvertFrom-Json
$output=@()
$groups=@($r.shells | Group-Object {$_.target.hwnd})
foreach($group in $groups){
 $t=$group.Group[0].target;$h=[IntPtr][long]$t.hwnd
 if(![GateClose]::IsWindow($h)){$output+=@{hwnd=$t.hwnd;result='already absent'};continue}
 [uint32]$ownerPid=0;[GateClose]::GetWindowThreadProcessId($h,[ref]$ownerPid)|Out-Null
 $owner=Get-Process -Id $ownerPid -ErrorAction SilentlyContinue
 if(!$owner -or $ownerPid -ne $t.owner.pid -or $owner.StartTime.ToFileTimeUtc().ToString() -ne $t.owner.startTime){$output+=@{hwnd=$t.hwnd;result='changed owner, untouched'};continue}
 $live=@($group.Group | ForEach-Object { $p=Get-Process -Id $_.registration.shell.pid -ErrorAction SilentlyContinue;if($p -and $p.StartTime.ToFileTimeUtc().ToString() -eq $_.registration.shell.startTime){$p.Id} })
 if($live.Count -gt 0){$output+=@{hwnd=$t.hwnd;result='live registered test shells, untouched'};continue}
 $root=[Windows.Automation.AutomationElement]::FromHandle($h)
 $condition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::TabItem)
 $tabs=$root.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)
 $names=@();foreach($tab in $tabs){$names+=$tab.Current.Name}
 $expected=@($group.Group | ForEach-Object {'SHELLFOX:'+$_.registration.sessionId+':'+$_.registration.tabId})
 $title=New-Object Text.StringBuilder 2048;[GateClose]::GetWindowText($h,$title,2048)|Out-Null
 if($names.Count -ne 3 -or $expected.Count -ne 3 -or (Compare-Object ($expected | Sort-Object) ($names | Sort-Object)) -or (!$title.ToString().StartsWith($t.markerPrefix) -and $title.ToString() -ne 'Shellfox TEST renamed title')){
  $output+=@{hwnd=$t.hwnd;result='unexpected tabs/title, untouched; manual permission needed';tabs=$names};continue
 }
 # Test cleanup only: invoke each recorded dead fixture tab's own close button.
 # UIA is NOT used for product targeting/focus/close. No keystrokes or global process termination.
 $remaining=@($expected)
 while($remaining.Count -gt 0){
  $current=$root.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)
  $currentNames=@();foreach($tab in $current){$currentNames+=$tab.Current.Name}
  if($currentNames.Count -ne $remaining.Count -or (Compare-Object ($remaining | Sort-Object) ($currentNames | Sort-Object))){throw 'Unexpected tab appeared during cleanup; left untouched'}
  $item=$current[$current.Count-1]
  $closeCondition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton')
  $button=$item.FindFirst([Windows.Automation.TreeScope]::Descendants,$closeCondition)
  if(!$button){throw 'Recorded dead fixture tab has no accessible close button'}
  $name=$item.Current.Name
  $invoke=[Windows.Automation.InvokePattern]$button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)
  $invoke.Invoke()
  $remaining=@($remaining|Where-Object {$_ -ne $name})
  Start-Sleep -Milliseconds 300
 }
 $deadline=[DateTime]::UtcNow.AddSeconds(5)
 while([GateClose]::IsWindow($h) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100}
 if([GateClose]::IsWindow($h)){throw 'Recorded fixture HWND did not close'}
 $output+=@{hwnd=$t.hwnd;result='closed dead-shell fixture window';tabs=$names}
}
$output | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 ($reportFile+'.windows-cleanup.json')
if(@($output | Where-Object {$_.result -ne 'already absent' -and $_.result -ne 'closed dead-shell fixture window'}).Count -gt 0){throw 'Some test windows were left untouched. Inspect cleanup report and ask permission.'}
exit 0
