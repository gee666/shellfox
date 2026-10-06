using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using System.Text.RegularExpressions;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.Native;

public static class Windows {
 public delegate bool EnumProc(nint h, nint p);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f,nint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(nint h,StringBuilder text,int n);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(nint h,out uint pid);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(nint h);
 [DllImport("user32.dll")] static extern bool IsWindow(nint h);
 [DllImport("user32.dll")] static extern bool SetForegroundWindow(nint h);
 [DllImport("user32.dll")] static extern bool ShowWindow(nint h,int c);
 [DllImport("user32.dll")] static extern nint GetForegroundWindow();
 [DllImport("user32.dll",SetLastError=true)] static extern bool AllowSetForegroundWindow(uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern SafeProcessHandle OpenProcess(uint access,bool inherit,int pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(SafeProcessHandle p,out long created,out long exit,out long kernel,out long user);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool QueryFullProcessImageName(SafeProcessHandle p,int flags,StringBuilder text,ref int n);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(SafeProcessHandle p,uint access,out SafeAccessTokenHandle token);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool GetTokenInformation(SafeAccessTokenHandle token,int type,out int value,int length,out int returned);
 [DllImport("shell32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern nint CommandLineToArgvW(string commandLine,out int argc);
 [DllImport("kernel32.dll")] static extern nint LocalFree(nint p);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetNamedPipeClientProcessId(SafePipeHandle pipe,out uint pid);
 public static readonly string UserSid=WindowsIdentity.GetCurrent().User!.Value;
 public record ProcessInfo(Identity Identity,string Executable,bool SameUser,bool Elevated);
 public static ProcessInfo Process(int pid) {
  using var p=OpenProcess(0x1000,false,pid);
  if(p.IsInvalid){ var error=Marshal.GetLastWin32Error(); throw new NativeError(error==87?"NOT_FOUND":"MONITOR_UNAVAILABLE","Process is absent or inaccessible."); }
  if(!GetProcessTimes(p,out var created,out var exited,out _,out _)) throw new NativeError("MONITOR_UNAVAILABLE","Cannot read exact process creation time.");
  if(exited!=0)throw new NativeError("NOT_FOUND","Process has exited.");
  var text=new StringBuilder(32768); var n=text.Capacity;
  if(!QueryFullProcessImageName(p,0,text,ref n)) throw new NativeError("MONITOR_UNAVAILABLE","Cannot verify process executable.");
  if(!OpenProcessToken(p,8,out var token)) throw new NativeError("MONITOR_UNAVAILABLE","Cannot verify process owner.");
  using(token){
   using var identity=new WindowsIdentity(token.DangerousGetHandle());
   if(!GetTokenInformation(token,20,out var elevated,4,out _)) throw new NativeError("MONITOR_UNAVAILABLE","Cannot verify process elevation.");
   return new(new(pid,created.ToString(System.Globalization.CultureInfo.InvariantCulture)),text.ToString(),identity.User?.Value==UserSid,elevated!=0);
  }
 }
 public static bool Same(Identity a,Identity b) => a.Pid==b.Pid && a.StartTime==b.StartTime;
 public static string[] Args(string commandLine) {
  if(commandLine.Length==0) return [];
  var p=CommandLineToArgvW(commandLine,out var count);
  if(p==0) throw new NativeError("MONITOR_UNAVAILABLE","Cannot parse Windows command line.");
  try { return Enumerable.Range(0,count).Select(i=>Marshal.PtrToStringUni(Marshal.ReadIntPtr(p,i*IntPtr.Size))!).ToArray(); } finally { LocalFree(p); }
 }
 public record Window(string Hwnd,Identity Owner,string Title);
 public static List<Window> List() {
  var found=new List<Window>();
  EnumWindows((h,_)=>{
   if(!IsWindowVisible(h))return true;
   var text=new StringBuilder(2048); GetWindowText(h,text,text.Capacity);
   if(!text.ToString().StartsWith("SHELLFOX:",StringComparison.Ordinal))return true;
   GetWindowThreadProcessId(h,out var pid);
   try{
    var p=Process((int)pid);
    var packageRoot=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"WindowsApps")+"\\Microsoft.WindowsTerminal";
    if(!p.SameUser || p.Elevated || !p.Executable.StartsWith(packageRoot,StringComparison.OrdinalIgnoreCase) || !Path.GetFileName(p.Executable).Equals("WindowsTerminal.exe",StringComparison.OrdinalIgnoreCase))return true;
    found.Add(new(h.ToInt64().ToString(),p.Identity,text.ToString()));
   }catch(NativeError){ /* Unknown executable ownership must never become a usable target. */ }
   return true;
  },0);
  return found;
 }
 public static bool Marker(string title,string prefix) => Regex.IsMatch(title,"^"+Regex.Escape(prefix)+"[a-fA-F0-9]{8}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{12}( - Windows Terminal)?$",RegexOptions.CultureInvariant);
 public static void Validate(Target t){
  Wire.Id(t.SessionId); Wire.Identity(t.Owner);
  if(t.Kind!="windows-terminal" || t.Verification!="native-title" || t.WindowName!="shellfox-"+t.SessionId || t.MarkerPrefix!="SHELLFOX:"+t.SessionId+":" || !long.TryParse(t.Hwnd,out var h) || h<=0)throw new NativeError("VALIDATION","Invalid Windows Terminal target.");
 }
 public static Target Verify(Target t){
  Validate(t);
  var matches=List().Where(w=>Marker(w.Title,t.MarkerPrefix)).ToArray();
  if(matches.Length>1)throw new NativeError("TARGET_AMBIGUOUS","More than one window has this session's marker. Control is disabled.");
  if(matches.Length!=1 || matches[0].Hwnd!=t.Hwnd || !Same(matches[0].Owner,t.Owner) || !IsWindow((nint)long.Parse(t.Hwnd)))throw new NativeError("TARGET_LOST","Window ownership cannot be verified. Renaming, dragging tabs, hidden titles and unmanaged selected tabs are unsupported. Enable showTerminalTitleInTitlebar and switch manually.");
  return t;
 }
 public static Target? Bind(Launch request){
  var prefix="SHELLFOX:"+request.SessionId+":";
  var matches=List().Where(w=>Marker(w.Title,prefix)).ToArray();
  if(matches.Length!=1 || !matches[0].Title.StartsWith(request.TitleMarker,StringComparison.Ordinal))return null;
  return new("windows-terminal",request.WindowName,matches[0].Hwnd,matches[0].Owner,request.SessionId,prefix,"native-title");
 }
 // Physical liveness is used only for an already authenticated generation. A title change
 // is not proof that a window closed, and must never authorize a replacement.
 public static string Life(Target t){
  Validate(t);var h=(nint)long.Parse(t.Hwnd);
  if(!IsWindow(h))return "closed";
  GetWindowThreadProcessId(h,out var pid);
  try{
   var owner=Process((int)pid);
   if(!Same(owner.Identity,t.Owner))return "closed";
   if(!owner.SameUser || owner.Elevated)return "unavailable";
   return "alive";
  }catch(NativeError e){return e.Code=="NOT_FOUND"?"closed":"unavailable";}
 }
 // This helper mode is spawned directly by Electron for a focused-window IPC action.
 // A foreground process's newly started child may delegate its permission to the resident
 // broker. Never ASFW_ANY, arbitrary input, foreground-timeout changes or unverified targets.
 public static bool GrantForeground(int brokerPid){
  using var self=new System.Management.ManagementObject($"Win32_Process.Handle='{Environment.ProcessId}'");self.Get();
  var parentPid=Convert.ToInt32(self["ParentProcessId"]);
  var parent=Process(parentPid);var broker=Process(brokerPid);
  using var b=new System.Management.ManagementObject($"Win32_Process.Handle='{brokerPid}'");b.Get();
  if(!parent.SameUser || parent.Elevated || !broker.SameUser || broker.Elevated || !broker.Executable.Equals(Environment.ProcessPath,StringComparison.OrdinalIgnoreCase) || Convert.ToInt32(b["ParentProcessId"])!=parentPid || !Same(parent.Identity,Process(parentPid).Identity) || !Same(broker.Identity,Process(brokerPid).Identity))return false;
  return AllowSetForegroundWindow((uint)brokerPid);
 }
 public static async Task<object> FocusPhysical(Target t){
  if(Life(t)!="alive")throw new NativeError("TARGET_LOST","The authenticated window is closed or inaccessible.");
  var h=(nint)long.Parse(t.Hwnd);ShowWindow(h,9);SetForegroundWindow(h);await Task.Delay(100);
  if(Life(t)!="alive")throw new NativeError("TARGET_LOST","Window changed during focus.");
  if(GetForegroundWindow()!=h)throw new NativeError("FOCUS_DENIED","Windows denied foreground focus after target verification. Click again while Shellfox is foreground, or switch manually.");
  return new {focused=true};
 }
 public static async Task<object> Focus(Target t){
  Verify(t); var h=(nint)long.Parse(t.Hwnd);
  ShowWindow(h,9); SetForegroundWindow(h);
  await Task.Delay(100);
  Verify(t);
  if(GetForegroundWindow()!=h)throw new NativeError("FOCUS_DENIED","Windows denied foreground focus. Switch to Windows Terminal manually.");
  return new { focused=true };
 }
 public static string LocalDirectory(string path){
  if(string.IsNullOrWhiteSpace(path) || path.Length>32760 || path.Contains('\0') || !Regex.IsMatch(path,"^[a-zA-Z]:[\\\\/]") || path.StartsWith("\\\\") || !Directory.Exists(path))throw new NativeError("VALIDATION","Choose an accessible absolute local directory.");
  var full=Path.GetFullPath(path);
  var drive=new DriveInfo(Path.GetPathRoot(full)!);
  if(drive.DriveType!=DriveType.Fixed && drive.DriveType!=DriveType.Removable)throw new NativeError("VALIDATION","Remote and virtual drives are not supported.");
  // Reject reparse-point ancestors rather than accidentally accepting a link into UNC/device storage.
  for(var dir=new DirectoryInfo(full);dir!=null;dir=dir.Parent)
   if((dir.Attributes&FileAttributes.ReparsePoint)!=0)throw new NativeError("VALIDATION","Reparse-point directories are unsupported. Choose the local target folder.");
  Directory.EnumerateFileSystemEntries(full).Take(1).ToArray();
  return full;
 }
 public static Dictionary<string,string> Shells(){
  var candidates=new Dictionary<string,string>{
   ["pwsh"]=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"PowerShell","7","pwsh.exe"),
   ["windows-powershell"]=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows),"System32","WindowsPowerShell","v1.0","powershell.exe")
  };
  return candidates.Where(p=>File.Exists(p.Value)).ToDictionary(p=>p.Key,p=>p.Value);
 }
 public static void Shell(string id,string executable){
  if(!Shells().TryGetValue(id,out var expected) || !Path.GetFullPath(executable).Equals(expected,StringComparison.OrdinalIgnoreCase))throw new NativeError("DEPENDENCY_MISSING","Shell must be an installed PowerShell 7 or Windows PowerShell binary in its supported standard location.");
 }
}
