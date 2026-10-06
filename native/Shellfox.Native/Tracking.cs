using System.Management;

namespace Shellfox.Native;

public record ProcessRow(Identity? Identity,int Pid,int ParentPid,string? Executable,string? CommandLine,bool Accessible);
public sealed class Tracker {
 // Only discovered live identities are retained. A reused numeric PID cannot inherit this map.
 readonly Dictionary<string,string> owners=new();
 Dictionary<int,ProcessRow> previous=new();
 public static string Key(Identity p) => p.Pid+":"+p.StartTime;
 public static bool Executable(Rule r,string path) => r.ExecutablePaths.Length>0
  ? r.ExecutablePaths.Any(p=>Path.GetFullPath(p).Equals(path,StringComparison.OrdinalIgnoreCase))
  : r.ExecutableBasenames.Any(p=>Path.GetFileName(path).Equals(p,StringComparison.OrdinalIgnoreCase));
 public static string Normalize(string path) => path.Replace('/','\\').TrimStart('\\');
 public static bool Suffix(string path,string suffix){
  var p=Normalize(path); var s=Normalize(suffix);
  return p.Equals(s,StringComparison.OrdinalIgnoreCase) || p.EndsWith("\\"+s,StringComparison.OrdinalIgnoreCase);
 }
 public record ParsedScript(string? Path,bool Unsupported);
 static readonly HashSet<string> NodeValues=["-r","--require","--import","--loader","--experimental-loader","--title","--conditions","-C","--input-type","--unhandled-rejections","--redirect-warnings","--trace-event-categories","--trace-event-file-pattern","--icu-data-dir","--openssl-config","--heap-prof-dir","--heap-prof-name","--cpu-prof-dir","--cpu-prof-name","--test-name-pattern","--test-reporter","--test-reporter-destination","--test-concurrency","--test-shard","--disable-warning","--inspect-port","--heapsnapshot-signal","--heapsnapshot-near-heap-limit","--snapshot-blob"];
 static readonly HashSet<string> NodeFlags=["--no-warnings","--enable-source-maps","--trace-warnings","--trace-uncaught","--trace-deprecation","--no-deprecation","--throw-deprecation","--use-strict","--experimental-modules","--inspect","--inspect-brk","--inspect-wait"];
 public static ParsedScript ParseScript(string commandLine){
  var args=Windows.Args(commandLine);
  if(args.Length<2)return new(null,false);
  var exe=Path.GetFileName(args[0]).ToLowerInvariant();
  // cmd.exe and pnpm shims may preserve argv[0] as `node`, without the .exe suffix.
  // The rule still verifies the real executable from QueryFullProcessImageName first.
  if(exe is "node" or "nodejs" or "bun" or "pwsh" or "powershell")exe += ".exe";
  if(exe is "pwsh.exe" or "powershell.exe"){
   for(var i=1;i<args.Length;i++){
    var flag=args[i].ToLowerInvariant();
    if(flag is "-command" or "-c" or "-encodedcommand" or "-enc" or "-ec" or "-e")return new(null,false);
    if(flag is "-file" or "-f")return i+1<args.Length?new(args[i+1],false):new(null,true);
    if(flag is "-executionpolicy" or "-ep" or "-workingdirectory" or "-wd" or "-inputformat" or "-outputformat" or "-windowstyle" or "-configurationname"){if(++i>=args.Length)return new(null,true);continue;}
    if(flag is "-nologo" or "-noprofile" or "-nop" or "-noexit" or "-noninteractive")continue;
    return new(null,true);
   }
   return new(null,false);
  }
  if(exe is not "node.exe" and not "bun.exe")return new(null,true);
  // Only the interpreter's script slot is inspected, never arbitrary prompts or option values.
  for(var i=1;i<args.Length;i++){
   var a=args[i];
   if(a=="--")return new(i+1<args.Length?args[i+1]:null,false);
   if(a is "-e" or "--eval" or "-p" or "--print" || a.StartsWith("--eval=",StringComparison.Ordinal) || a.StartsWith("--print=",StringComparison.Ordinal) || !a.StartsWith("--",StringComparison.Ordinal) && (a.StartsWith("-e",StringComparison.Ordinal) || a.StartsWith("-p",StringComparison.Ordinal)))return new(null,false);
   if(NodeValues.Contains(a)){if(++i>=args.Length)return new(null,true);continue;}
   if(NodeFlags.Contains(a) || a.StartsWith("--",StringComparison.Ordinal) && a.Contains('='))continue;
   if(a.StartsWith('-') || exe=="bun.exe" && a=="run")return new(null,true);
   return new(a,false);
  }
  return new(null,false);
 }
 public static string? Script(string commandLine) => ParseScript(commandLine).Path;
 public static bool Matches(Rule rule,ProcessRow p) => p.Executable!=null && Executable(rule,p.Executable) &&
  (rule.ScriptPathSuffixes.Length==0 || p.CommandLine!=null && Script(p.CommandLine) is string script && rule.ScriptPathSuffixes.Any(s=>Suffix(script,s)));
 public static bool WmiCreationMatches(Identity identity,string dmtf){
  try{return ulong.Parse(identity.StartTime)/10==(ulong)ManagementDateTimeConverter.ToDateTime(dmtf).ToFileTimeUtc()/10;}catch(Exception){return false;}
 }
 public static List<ProcessRow> Snapshot(){
  using var search=new ManagementObjectSearcher("SELECT ProcessId,ParentProcessId,CommandLine,CreationDate FROM Win32_Process");
  search.Options.Timeout=TimeSpan.FromSeconds(5);
  using var collection=search.Get(); var rows=new List<ProcessRow>();
  foreach(ManagementObject p in collection){
   using(p){
    var pid=Convert.ToInt32(p["ProcessId"]); var parent=Convert.ToInt32(p["ParentProcessId"]);
    try {
     var native=Windows.Process(pid);
     // WMI fields may describe a process that exited while the native PID query ran.
     // DMTF is microsecond precision; identities used for ownership remain exact native FILETIME.
     if(p["CreationDate"] is not string created || !WmiCreationMatches(native.Identity,created))rows.Add(new(null,pid,parent,null,null,false));
     else rows.Add(new(native.Identity,pid,parent,native.Executable,p["CommandLine"] as string,native.SameUser && !native.Elevated));
    }
    catch(NativeError){ rows.Add(new(null,pid,parent,null,null,false)); }
    if(rows.Count>20000)throw new NativeError("MONITOR_UNAVAILABLE","Process snapshot exceeds supported limit.");
   }
  }
  return rows;
 }
 public IReadOnlyDictionary<string,string> ResolveOwners(Watch watch,List<ProcessRow> snapshot){
  var rows=snapshot.ToDictionary(p=>p.Pid); var roots=watch.Tabs.ToDictionary(t=>Key(t.Registration.Shell),t=>t.TabId);
  var aliveKeys=snapshot.Where(p=>p.Identity!=null).Select(p=>Key(p.Identity!)).ToHashSet();
  foreach(var key in owners.Keys.ToArray()) if(!aliveKeys.Contains(key) || !roots.ContainsKey(owners[key])) owners.Remove(key);
  foreach(var root in roots)if(aliveKeys.Contains(root.Key))owners[root.Key]=root.Key;
  string? Find(ProcessRow p,HashSet<int> visiting){
   if(p.Identity==null || !p.Accessible || !visiting.Add(p.Pid))return null;
   var key=Key(p.Identity);
   if(roots.ContainsKey(key))return key;
   if(rows.TryGetValue(p.ParentPid,out var parent) && parent.Identity!=null && parent.Accessible && ulong.Parse(parent.Identity.StartTime)<=ulong.Parse(p.Identity.StartTime)){
    // A child observed before a different generation of this parent PID cannot be reparented.
    // This also protects ownership if the wall clock moves backwards between process births.
    if(previous.TryGetValue(p.Pid,out var oldChild) && oldChild.Identity!=null && Windows.Same(oldChild.Identity,p.Identity) && previous.TryGetValue(parent.Pid,out var oldParent) && oldParent.Identity!=null && !Windows.Same(oldParent.Identity,parent.Identity))return owners.GetValueOrDefault(key);
    var owner=Find(parent,visiting);
    if(owner!=null){ owners[key]=owner; return owner; }
   }
   return owners.GetValueOrDefault(key); // Intermediate exited, keep previously established ownership.
  }
  foreach(var p in snapshot)Find(p,new());
  previous=rows;
  return owners.ToDictionary(p=>p.Key,p=>roots[p.Value]);
 }
 public Observation[] Observe(Watch watch,List<ProcessRow> snapshot){
  ResolveOwners(watch,snapshot);
  var rows=snapshot.ToDictionary(p=>p.Pid);
  var output=new List<Observation>();
  foreach(var tab in watch.Tabs){
   var root=tab.Registration.Shell; string state="alive"; string? reason=null;
   try{ var process=Windows.Process(root.Pid); if(!Windows.Same(root,process.Identity))state="exited"; else if(!process.SameUser || process.Elevated || !process.Executable.Equals(tab.Registration.ShellExecutable,StringComparison.OrdinalIgnoreCase)){ state="unavailable"; reason="Root executable, owner or elevation could not be verified against registration."; } }
   catch(NativeError e){ state=e.Code=="NOT_FOUND"?"exited":"unavailable"; reason=state=="unavailable"?e.Message:null; }
   if(state=="alive" && (!rows.TryGetValue(root.Pid,out var rootRow) || rootRow.Identity==null)){ reason="Shell absent from available process snapshot."; }
   var agents=0;
   foreach(var p in snapshot){
    var owned=p.Identity!=null && owners.GetValueOrDefault(Key(p.Identity))==Key(root);
    if(!owned){
     if(!p.Accessible && rows.TryGetValue(p.ParentPid,out var parent) && parent.Identity!=null && owners.GetValueOrDefault(Key(parent.Identity))==Key(root))reason="A descendant is inaccessible or elevated.";
     continue;
    }
    if(Windows.Same(p.Identity!,root))continue; // The interactive shell is the root, not a descendant agent.
    if(!p.Accessible || p.Executable==null){ reason="A descendant is inaccessible."; continue; }
    if(watch.Rules.Any(r=>r.Enabled && Executable(r,p.Executable) && r.ScriptPathSuffixes.Length>0)){
     if(p.CommandLine==null)reason="A relevant descendant command line is inaccessible.";
     else if(ParseScript(p.CommandLine).Unsupported)reason="A configured script rule uses an unsupported interpreter or option form. Tracking cannot infer healthy waiting.";
    }
    if(watch.Rules.Any(r=>r.Enabled && Matches(r,p)))agents++;
   }
   output.Add(new(tab.SessionId,tab.TabId,Wire.Now(),state,reason==null && state=="alive"?"healthy":"unknown",state=="alive"?agents:0,reason));
  }
  return output.ToArray();
 }
 public static void Validate(Watch w){
  if(w.Tabs==null || w.Rules==null || w.Tabs.Length>1000 || w.Rules.Length>100)throw new NativeError("VALIDATION","Watch exceeds limits.");
  var ids=new HashSet<string>(); var roots=new HashSet<string>();
  foreach(var t in w.Tabs){
   Wire.Id(t.SessionId); Wire.Id(t.TabId); Wire.Identity(t.Registration.Shell); Wire.Id(t.Registration.OperationId);
   if(t.SessionId!=t.Registration.SessionId || t.TabId!=t.Registration.TabId || !ids.Add(t.TabId) || !roots.Add(Key(t.Registration.Shell)))throw new NativeError("VALIDATION","Duplicate or inconsistent watched registration.");
   Windows.Shell(Path.GetFileName(t.Registration.ShellExecutable).Equals("pwsh.exe",StringComparison.OrdinalIgnoreCase)?"pwsh":"windows-powershell",t.Registration.ShellExecutable);
  }
  foreach(var r in w.Rules){
   Wire.Id(r.Id);
   if(r.ExecutableBasenames==null || r.ExecutablePaths==null || r.ScriptPathSuffixes==null || r.ExecutableBasenames.Length+r.ExecutablePaths.Length==0 || r.ExecutableBasenames.Length>30 || r.ExecutablePaths.Length>30 || r.ScriptPathSuffixes.Length>30)throw new NativeError("VALIDATION","Invalid process rule limits.");
   if(r.ExecutableBasenames.Concat(r.ExecutablePaths).Concat(r.ScriptPathSuffixes).Any(s=>string.IsNullOrWhiteSpace(s) || s.Length>32760 || s.Contains('\0')))throw new NativeError("VALIDATION","Invalid process rule value.");
   if(r.ExecutableBasenames.Any(s=>s.Contains('/') || s.Contains('\\')) || r.ExecutablePaths.Any(s=>!Path.IsPathFullyQualified(s)))throw new NativeError("VALIDATION","Invalid executable constraint.");
   if(r.ScriptPathSuffixes.Any(s=>s.Split(['\\','/']).Any(c=>c is "." or "..")))throw new NativeError("VALIDATION","Script suffixes must contain path components, not traversal.");
  }
 }
}
