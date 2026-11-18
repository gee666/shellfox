param(
  [Parameter(Mandatory=$true)][ValidateSet('identity','discover','configure','capture','close')][string]$Action,
  [Parameter(Mandatory=$true)][int]$ProcessId,
  [string]$WindowHandle,
  [string]$Birth,
  [string]$Executable,
  [string]$Output
)
$ErrorActionPreference = 'Stop'
if ($ProcessId -in @(9484,25904)) { throw 'Protected user process. Refusing access.' }
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ParityWindow {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 public delegate bool EnumCallback(IntPtr hwnd,IntPtr data);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumCallback callback,IntPtr data);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int hgt,uint flags);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] public static extern bool QueryFullProcessImageName(IntPtr h,int flags,System.Text.StringBuilder name,ref int length);
 public static IntPtr FindOwned(int pid) {
  IntPtr found=IntPtr.Zero; int count=0;
  EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid&&IsWindowVisible(h)){found=h;count++;}return true;},IntPtr.Zero);
  return count==1?found:IntPtr.Zero;
 }
 [DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(uint access,bool inherit,int pid);
 [DllImport("kernel32.dll")] public static extern bool GetProcessTimes(IntPtr h,out long born,out long exit,out long kernel,out long user);
 [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
 [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h,int attr,out RECT rect,int size);
}
'@
$hwnd = if ($Action -eq 'discover') { [ParityWindow]::FindOwned($ProcessId) } else { [IntPtr]::new([long]::Parse($WindowHandle)) }
if ($hwnd -eq [IntPtr]::Zero) { throw 'No unique visible owned window yet.' }
$WindowHandle=$hwnd.ToInt64().ToString()
$processHandle = [ParityWindow]::OpenProcess(0x1000,$false,$ProcessId)
if ($processHandle -eq [IntPtr]::Zero) { throw 'Cannot open exact owned process.' }
function Assert-Identity {
  if (![ParityWindow]::IsWindow($hwnd)) { throw 'Owned window no longer exists.' }
  [uint32]$owner=0
  [void][ParityWindow]::GetWindowThreadProcessId($hwnd,[ref]$owner)
  if ($owner -ne $ProcessId) { throw 'HWND owner changed.' }
  [long]$born=0; [long]$ended=0; [long]$kernel=0; [long]$user=0
  if (![ParityWindow]::GetProcessTimes($processHandle,[ref]$born,[ref]$ended,[ref]$kernel,[ref]$user) -or $ended -ne 0) { throw 'Owned process has exited.' }
  if ($Birth -and $Birth -ne $born.ToString()) { throw 'PID birth identity mismatch.' }
  if ($Executable) {
    $name=New-Object Text.StringBuilder 32768; [int]$length=$name.Capacity
    if (![ParityWindow]::QueryFullProcessImageName($processHandle,0,$name,[ref]$length) -or ![string]::Equals([IO.Path]::GetFullPath($Executable),$name.ToString(),[StringComparison]::OrdinalIgnoreCase)) { throw 'Owned executable identity mismatch.' }
  }
  return $born.ToString()
}
try {
  $actualBirth=Assert-Identity
  $windowRect=New-Object ParityWindow+RECT
  $clientRect=New-Object ParityWindow+RECT
  $dwmRect=New-Object ParityWindow+RECT
  $origin=New-Object ParityWindow+POINT
  if (![ParityWindow]::GetWindowRect($hwnd,[ref]$windowRect) -or ![ParityWindow]::GetClientRect($hwnd,[ref]$clientRect) -or ![ParityWindow]::ClientToScreen($hwnd,[ref]$origin)) { throw 'Window geometry unavailable.' }
  if ($Action -in @('configure','close')) {
    if (!$Birth -or !$Executable) { throw 'Native mutation requires owned birth and executable identity.' }
    if ($Action -eq 'close') {
      if (![ParityWindow]::PostMessage($hwnd,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)) { throw 'Owned close message failed.' }
    } else {
      $outerWidth=1200+($windowRect.Right-$windowRect.Left)-($clientRect.Right-$clientRect.Left)
      $outerHeight=760+($windowRect.Bottom-$windowRect.Top)-($clientRect.Bottom-$clientRect.Top)
      if (![ParityWindow]::SetWindowPos($hwnd,[IntPtr]::Zero,64,64,$outerWidth,$outerHeight,0x0014)) { throw 'Owned resize failed.' }
      $origin=New-Object ParityWindow+POINT
      [void][ParityWindow]::GetWindowRect($hwnd,[ref]$windowRect); [void][ParityWindow]::GetClientRect($hwnd,[ref]$clientRect); [void][ParityWindow]::ClientToScreen($hwnd,[ref]$origin)
    }
  }
  [void][ParityWindow]::DwmGetWindowAttribute($hwnd,9,[ref]$dwmRect,16)
  $width=$windowRect.Right-$windowRect.Left; $height=$windowRect.Bottom-$windowRect.Top
  $result=@{pid=$ProcessId;hwnd=$WindowHandle;birth=$actualBirth;dpi=[ParityWindow]::GetDpiForWindow($hwnd);focused=([ParityWindow]::GetForegroundWindow() -eq $hwnd);window=$windowRect;client=$clientRect;clientOrigin=$origin;dwm=$dwmRect}
  if ($Action -eq 'capture') {
    if (!$Birth -or !$Output -or !$Executable) { throw 'Capture requires birth, executable identity and output base.' }
    $root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $full=[IO.Path]::GetFullPath($Output)
    $allowed=@((Join-Path $root 'tmp'),(Join-Path $root 'tests\visual-parity\baselines'))
    if (!($allowed | Where-Object { $full.StartsWith($_+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) })) { throw 'Output must be under owned parity fixtures or project tmp.' }
    if ($width -lt 100 -or $height -lt 100 -or $width -gt 8000 -or $height -gt 8000) { throw 'Unexpected owned-window dimensions.' }
    $bitmap=New-Object Drawing.Bitmap($width,$height,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
    try {
      $graphics=[Drawing.Graphics]::FromImage($bitmap)
      try {
        $dc=$graphics.GetHdc()
        try { if (![ParityWindow]::PrintWindow($hwnd,$dc,2)) { throw 'PrintWindow failed. No screen/foreign-window fallback.' } }
        finally { $graphics.ReleaseHdc($dc) }
      } finally { $graphics.Dispose() }
      [void](Assert-Identity)
      $bitmap.Save($full+'.window.png',[Drawing.Imaging.ImageFormat]::Png)
      $crop=New-Object Drawing.Rectangle(($origin.X-$windowRect.Left),($origin.Y-$windowRect.Top),($clientRect.Right-$clientRect.Left),($clientRect.Bottom-$clientRect.Top))
      $client=$bitmap.Clone($crop,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
      try { $client.Save($full+'.client.png',[Drawing.Imaging.ImageFormat]::Png) } finally { $client.Dispose() }
    } finally { $bitmap.Dispose() }
  }
  $result | ConvertTo-Json -Depth 5 -Compress
} finally { [void][ParityWindow]::CloseHandle($processHandle) }
