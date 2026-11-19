param(
    [ValidateSet('launch', 'capture', 'click', 'doubleclick', 'press', 'move', 'release', 'text', 'key', 'chord', 'repaint', 'close')]
    [string]$Action,
    [int]$TargetPid,
    [ValidateSet('Left','Right','Middle')][string]$Button = 'Left',
    [int]$X = 0,
    [int]$Y = 0,
    [string]$Text = '',
    [int]$Key = 0,
    [switch]$Control,
    [switch]$Shift,
    [ValidateRange(1,8192)][int]$Count = 1,
    [string]$Output = '',
    [string]$BuildPath = '',
    [string]$HelperPath = '',
    [string]$HelperSha256 = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$executable = if ($BuildPath) { [IO.Path]::GetFullPath($BuildPath) } else { Join-Path $root 'target/debug/shellfox.exe' }
$acceptance = Join-Path $root 'tmp/desktop-ui-acceptance'
New-Item -ItemType Directory -Path $acceptance -Force | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OwnedWindow {
    public delegate bool EnumProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, System.Text.StringBuilder text, int size);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, IntPtr wp, IntPtr lp, uint flags, uint timeout, out IntPtr result);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint source, uint target, bool attach);
    [DllImport("user32.dll")] public static extern bool GetKeyboardState(byte[] state);
    [DllImport("user32.dll")] public static extern bool SetKeyboardState(byte[] state);
    public static void Chord(IntPtr window, int key, bool control, bool shift) {
        uint process; uint target=GetWindowThreadProcessId(window,out process);
        uint source=GetCurrentThreadId();
        if (!AttachThreadInput(source,target,true)) throw new InvalidOperationException("Cannot attach owned window input queue");
        byte[] original=new byte[256]; bool captured=false;
        try {
            if (!GetKeyboardState(original)) throw new InvalidOperationException("Cannot read input queue state");
            captured=true; byte[] state=(byte[])original.Clone();
            state[0x11]=(byte)(control?0x80:0); state[0x10]=(byte)(shift?0x80:0);
            if (!SetKeyboardState(state)) throw new InvalidOperationException("Cannot set scoped input state");
            IntPtr result;
            if (SendMessageTimeout(window,0x100,(IntPtr)key,(IntPtr)1,2,2000,out result)==IntPtr.Zero ||
                SendMessageTimeout(window,0x101,(IntPtr)key,(IntPtr)1,2,2000,out result)==IntPtr.Zero)
                throw new InvalidOperationException("Owned window input timed out");
        } finally { if (captured) SetKeyboardState(original); AttachThreadInput(source,target,false); }
    }
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
    public static IntPtr Find(uint process, string expectedCaption) {
        IntPtr found=IntPtr.Zero;
        EnumWindows((window, _) => {
            uint actual; GetWindowThreadProcessId(window,out actual);
            if (actual==process && IsWindowVisible(window)) {
                var title=new System.Text.StringBuilder(256);GetWindowText(window,title,256);
                if (title.ToString()==expectedCaption) { found=window; return false; }
            }
            return true;
        },IntPtr.Zero);
        return found;
    }
}
'@
if ($Action -eq 'launch') {
    $privateRoot = [IO.Path]::GetFullPath((Join-Path $root 'tmp/gpui-builds'))+[IO.Path]::DirectorySeparatorChar
    if (![IO.Path]::GetFullPath($executable).StartsWith($privateRoot,[StringComparison]::OrdinalIgnoreCase)) { throw 'Native automation requires a separately built private executable, never target/debug or an installed app' }
    $caption = 'Shellfox v'+(Get-Content (Join-Path $root 'package.json') -Raw|ConvertFrom-Json).version
    if ($HelperPath -and !$HelperSha256) { throw 'Overridden helper requires its trusted build SHA-256' }
    $helper = Join-Path $root 'tmp/dev-helper/shellfox-tailscale.exe'
    if ($HelperPath -and [IO.Path]::GetFullPath($HelperPath) -ine [IO.Path]::GetFullPath($helper)) { throw "Windows dev launches must use $helper" }
    if (!(Test-Path -LiteralPath $helper -PathType Leaf)) { throw 'Build the dev helper with pnpm build:tailscale first' }
    $digest = if ($HelperSha256) { $HelperSha256 } else { (Get-FileHash -Algorithm SHA256 -LiteralPath $helper).Hash }
    if ($digest -notmatch '^[a-fA-F0-9]{64}$') { throw 'Invalid trusted helper digest' }
    $directory = Join-Path $acceptance ([DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-ffff'))
    New-Item -ItemType Directory -Path $directory | Out-Null
    $hostDirectory = Join-Path $directory 'host'
    $desktopDirectory = Join-Path $directory 'desktop'
    New-Item -ItemType Directory -Path $hostDirectory,$desktopDirectory | Out-Null
    $project = Join-Path $directory 'project'
    New-Item -ItemType Directory -Path $project | Out-Null
    Set-Content -Path (Join-Path $project 'proof.txt') -Value "Shellfox file editor fixture`nsecond line" -NoNewline
    $process = Start-Process -FilePath $executable -ArgumentList @('--shell','--ui-acceptance','--data-dir',('"'+$hostDirectory+'"'),'--desktop-dir',('"'+$desktopDirectory+'"'),'--helper',('"'+$helper+'"'),'--helper-sha256',$digest) -WorkingDirectory $root -PassThru -RedirectStandardOutput (Join-Path $directory 'stdout.log') -RedirectStandardError (Join-Path $directory 'stderr.log')
    $record = @{kind='gpui-ui-acceptance-v2';caption=$caption;binary_sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash;pid=$process.Id;started=$process.StartTime.ToUniversalTime().ToString('o');directory=$directory;host=$hostDirectory;desktop=$desktopDirectory;project=$project;executable=$executable}
    $record | ConvertTo-Json | Set-Content (Join-Path $acceptance ('owned-'+$process.Id+'.json'))
    $record | ConvertTo-Json
    exit
}
$record = Get-Content (Join-Path $acceptance ('owned-'+$TargetPid+'.json')) -Raw | ConvertFrom-Json
$privateRoot = [IO.Path]::GetFullPath((Join-Path $root 'tmp/gpui-builds'))+[IO.Path]::DirectorySeparatorChar
if ($record.kind -ne 'gpui-ui-acceptance-v2' -or ![IO.Path]::GetFullPath($record.executable).StartsWith($privateRoot,[StringComparison]::OrdinalIgnoreCase)) { throw 'Not a current, explicitly owned private acceptance process; preserve it' }
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $record.executable).Hash -ne $record.binary_sha256) { throw 'Private executable identity changed' }
$process = Get-Process -Id $TargetPid
if ($process.Path -ne $record.executable -or $process.StartTime.ToUniversalTime().Ticks -ne ([DateTime]$record.started).ToUniversalTime().Ticks) { throw 'Owned process identity changed' }
$command=(Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$TargetPid").CommandLine
if (!$command -or !$command.Contains('--ui-acceptance') -or $command.IndexOf($record.host,[StringComparison]::OrdinalIgnoreCase) -lt 0 -or $command.IndexOf($record.desktop,[StringComparison]::OrdinalIgnoreCase) -lt 0) { throw 'Private fixture flags/data directories changed' }
$window = [OwnedWindow]::Find($TargetPid,$record.caption)
if ($window -eq [IntPtr]::Zero) { throw 'Owned visible window not found' }
$title = New-Object Text.StringBuilder 256
[OwnedWindow]::GetWindowText($window,$title,256) | Out-Null
if ($title.ToString() -ne $record.caption) { throw 'Owned fixture does not have the actual source caption; leave it untouched' }
$position = [IntPtr](($Y -shl 16) -bor ($X -band 65535))
$down = switch ($Button) { 'Left' { 0x201 } 'Right' { 0x204 } 'Middle' { 0x207 } }
$up = $down + 1
$held = switch ($Button) { 'Left' { 1 } 'Right' { 2 } 'Middle' { 16 } }
switch ($Action) {
    'capture' {
        if (!$Output) { $Output = Join-Path $acceptance ('window-'+$TargetPid+'.png') }
        $fullOutput = [IO.Path]::GetFullPath($Output)
        if (!$fullOutput.StartsWith([IO.Path]::GetFullPath($acceptance)+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Capture must stay under tmp/desktop-ui-acceptance' }
        $rect = New-Object OwnedWindow+Rect
        if (![OwnedWindow]::GetWindowRect($window,[ref]$rect)) { throw 'Window bounds unavailable' }
        $bitmap = New-Object Drawing.Bitmap ($rect.Right-$rect.Left),($rect.Bottom-$rect.Top)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $dc = $graphics.GetHdc()
        try { if (![OwnedWindow]::PrintWindow($window,$dc,2)) { throw 'PrintWindow failed; do not substitute a desktop capture' } }
        finally { $graphics.ReleaseHdc($dc) }
        try { $bitmap.Save($fullOutput,[Drawing.Imaging.ImageFormat]::Png) }
        finally { $graphics.Dispose();$bitmap.Dispose() }
        Write-Output $fullOutput
    }
    'click' {
        [OwnedWindow]::PostMessage($window,0x200,[IntPtr]::Zero,$position) | Out-Null
        [OwnedWindow]::PostMessage($window,$down,[IntPtr]$held,$position) | Out-Null
        [OwnedWindow]::PostMessage($window,$up,[IntPtr]::Zero,$position) | Out-Null
    }
    'doubleclick' { for ($i=0;$i -lt 2;$i++) { [OwnedWindow]::PostMessage($window,$down,[IntPtr]$held,$position)|Out-Null;[OwnedWindow]::PostMessage($window,$up,[IntPtr]::Zero,$position)|Out-Null } }
    'press' { [OwnedWindow]::PostMessage($window,$down,[IntPtr]$held,$position) | Out-Null }
    'chord' { [OwnedWindow]::Chord($window,$Key,$Control.IsPresent,$Shift.IsPresent) }
    'move' { [OwnedWindow]::PostMessage($window,0x200,[IntPtr]$held,$position) | Out-Null }
    'release' { [OwnedWindow]::PostMessage($window,$up,[IntPtr]::Zero,$position) | Out-Null }
    'text' { foreach ($character in $Text.ToCharArray()) { [OwnedWindow]::PostMessage($window,0x102,[IntPtr][int]$character,[IntPtr]1) | Out-Null } }
    'key' {
        for ($i=0; $i -lt $Count; $i++) {
            [OwnedWindow]::PostMessage($window,0x100,[IntPtr]$Key,[IntPtr]1) | Out-Null
            [OwnedWindow]::PostMessage($window,0x101,[IntPtr]$Key,[IntPtr]1) | Out-Null
        }
    }
    'repaint' { [OwnedWindow]::PostMessage($window,0x0F,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null }
    'close' { [OwnedWindow]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null }
}
