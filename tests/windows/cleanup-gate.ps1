param([Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
$project=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$ReportPath=[IO.Path]::GetFullPath($ReportPath)
if(!$ReportPath.StartsWith((Join-Path $project 'tmp')+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Report must be in project tmp'}
$r=Get-Content -Raw $ReportPath|ConvertFrom-Json
$expected=@($r.shells | ForEach-Object {'SHELLFOX:'+$_.registration.sessionId+':'+$_.registration.tabId})
$targets=@($r.shells.target | Where-Object {$_})
$agentScripts=@((Join-Path $project 'tmp/native-gate/agent-alpha.cjs'),(Join-Path $project 'tmp/native-gate/agent-beta.cjs'))
$actions=@()
foreach($record in $r.cleanup){
 $id=$record.shell
 $p=Get-Process -Id $id.pid -ErrorAction SilentlyContinue
 if(!$p -or $p.StartTime.ToFileTimeUtc().ToString() -ne $id.startTime){continue}
 $children=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($id.pid)")
 foreach($c in $children){
  if($c.Name -ne 'node.exe' -or !$c.CommandLine -or !($agentScripts|Where-Object {$c.CommandLine.Contains($_)})){throw 'Unexpected child; left untouched'}
  $agent=Get-Process -Id $c.ProcessId -ErrorAction Stop
  if($agent.StartTime.ToUniversalTime() -lt $p.StartTime.ToUniversalTime()){throw 'Recycled child; untouched'}
  $time=$agent.StartTime.ToFileTimeUtc().ToString()
  if((Get-Process -Id $agent.Id).StartTime.ToFileTimeUtc().ToString() -ne $time){throw 'Identity changed'}
  Stop-Process -Id $agent.Id
  $actions+=@{agentPid=$agent.Id;startTime=$time;result='stopped recorded fixture script child'}
 }
 Start-Sleep -Milliseconds 300
 if(@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($id.pid)").Count -ne 0){throw 'Unexpected children remain'}
 if((Get-Process -Id $id.pid).StartTime.ToFileTimeUtc().ToString() -ne $id.startTime){throw 'Root identity changed'}
 Stop-Process -Id $id.pid
 $actions+=@{shell=$id;result='stopped exact recorded test root'}
}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class DRecovery {
 public delegate bool EnumProc(IntPtr h,IntPtr l);
 [DllImport("user32.dll")]public static extern bool EnumWindows(EnumProc p,IntPtr l);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")]public static extern bool IsWindow(IntPtr h);
}
'@
$windows=New-Object 'System.Collections.Generic.List[long]'
[DRecovery]::EnumWindows({param($h,$l) $b=New-Object Text.StringBuilder 2048;[DRecovery]::GetWindowText($h,$b,2048)|Out-Null;foreach($t in $targets) {if($b.ToString().StartsWith($t.markerPrefix)){$windows.Add($h.ToInt64());break}};return $true},[IntPtr]::Zero)|Out-Null
foreach($hwnd in $windows){
 $h=[IntPtr]$hwnd
 [uint32]$pidOwner=0;[DRecovery]::GetWindowThreadProcessId($h,[ref]$pidOwner)|Out-Null
 $owner=Get-Process -Id $pidOwner
 $binding=@($targets|Where-Object {$_.owner.pid -eq $pidOwner -and $_.owner.startTime -eq $owner.StartTime.ToFileTimeUtc().ToString()})
 if(!$binding.Count){throw 'Owner changed; untouched'}
 $element=[Windows.Automation.AutomationElement]::FromHandle($h)
 $cond=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::TabItem)
 $tabs=$element.FindAll([Windows.Automation.TreeScope]::Descendants,$cond)
 $names=@();foreach($tab in $tabs){$names+=$tab.Current.Name}
 if(!$names.Count -or $names.Count -gt 3){throw 'Unexpected tab count; untouched'}
 foreach($name in $names){
  if($expected -notcontains $name){throw 'Unexpected tab title; untouched'}
 }
 foreach($record in $r.shells){
  $name='SHELLFOX:'+$record.registration.sessionId+':'+$record.registration.tabId
  if($names -contains $name){
   $identity=$record.registration.shell;$live=Get-Process -Id $identity.pid -ErrorAction SilentlyContinue
   if($live -and $live.StartTime.ToFileTimeUtc().ToString() -eq $identity.startTime){throw 'Live recorded shell; tab left untouched'}
  }
 }
 # Roots and only their two known fixture-script forms have exited. UIA close only these UUID-marked tabs.
 while([DRecovery]::IsWindow($h)){
  $tabs=$element.FindAll([Windows.Automation.TreeScope]::Descendants,$cond)
  if(!$tabs.Count){break}
  $item=$tabs[$tabs.Count-1]
  if($names -notcontains $item.Current.Name){throw 'Unexpected tab appeared; untouched'}
  $button=$item.FindFirst([Windows.Automation.TreeScope]::Descendants,(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton')))
  if(!$button){throw 'No test tab close button'}
  ([Windows.Automation.InvokePattern]$button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)).Invoke()
  Start-Sleep -Milliseconds 400
 }
 if([DRecovery]::IsWindow($h)){throw 'Test HWND remains'}
 $actions+=@{hwnd=$hwnd;tabs=$names;result='closed only dead test UUID tabs after exact WT owner check'}
}
foreach($t in $targets){
 $h=[IntPtr][long]$t.hwnd
 if([DRecovery]::IsWindow($h)){
  [uint32]$pidOwner=0;[DRecovery]::GetWindowThreadProcessId($h,[ref]$pidOwner)|Out-Null
  $owner=Get-Process -Id $pidOwner -ErrorAction SilentlyContinue
  if($owner -and $pidOwner -eq $t.owner.pid -and $owner.StartTime.ToFileTimeUtc().ToString() -eq $t.owner.startTime){throw 'Recorded test HWND remains with an unexpected title; left untouched'}
 }
}
$actions|ConvertTo-Json -Depth 8|Set-Content ($ReportPath+'.recovery.json')
