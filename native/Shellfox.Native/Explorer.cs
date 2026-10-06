using Microsoft.Win32;

namespace Shellfox.Native;

public sealed class Explorer {
 public const string Owner="Shellfox/native-v1";
 public static readonly string[] Keys=[@"Software\Classes\Directory\shell\Shellfox",@"Software\Classes\Directory\Background\shell\Shellfox"];
 readonly string? packagedExecutable;
 readonly string[] keys;
 // Compatibility identifiers are used only for ownership-checked cleanup.
 internal const string LegacyOwner="PiManager/native-v1";
 internal static readonly string[] LegacyKeys=[@"Software\Classes\Directory\shell\PiManager",@"Software\Classes\Directory\Background\shell\PiManager"];
 readonly string[] legacyKeys;
 public Explorer(string? packagedExecutable) : this(packagedExecutable,Keys,LegacyKeys) {}
 // Test-only dependency injection. No broker request accepts registry key paths.
 internal Explorer(string? packagedExecutable,string[] keys,string[]? legacyKeys=null){ this.packagedExecutable=packagedExecutable; this.keys=keys; this.legacyKeys=legacyKeys??[]; }
 void CleanupLegacy(){
  foreach(var name in legacyKeys){
   using var key=Registry.CurrentUser.OpenSubKey(name);
   if(key?.GetValue("PiManagerOwner") as string!=LegacyOwner)continue;
   key.Dispose();Registry.CurrentUser.DeleteSubKeyTree(name,false);
  }
 }
 public static string Command(string exe,bool background){
  if(!Path.IsPathFullyQualified(exe) || exe.Contains('"') || exe.Contains('\0') || !exe.EndsWith(".exe",StringComparison.OrdinalIgnoreCase))throw new NativeError("VALIDATION","Expected absolute packaged executable path.");
  return "\""+exe+"\" --new-session --cwd \""+(background?"%V":"%1")+"\\.\"";
 }
 public object Status(){
  CleanupLegacy();
  var owned=keys.Select(k=>{using var key=Registry.CurrentUser.OpenSubKey(k); using var command=key?.OpenSubKey("command"); return key?.GetValue("ShellfoxOwner") as string==Owner && command?.GetValue("") is string cmd && packagedExecutable!=null && cmd==Command(packagedExecutable,k==keys[1]);}).ToArray();
  return new { supported=packagedExecutable!=null, installed=owned.All(v=>v),folderItemInstalled=owned[0],backgroundInstalled=owned[1],reason=packagedExecutable==null?"Explorer integration requires a packaged installation. Legacy items may appear under Show more options.":null };
 }
 public object Set(ExplorerRequest request){
  CleanupLegacy();
  if(packagedExecutable==null)throw new NativeError("UNSUPPORTED","Explorer integration requires a packaged installation.");
  if(!Path.GetFullPath(request.ExecutablePath).Equals(Path.GetFullPath(packagedExecutable),StringComparison.OrdinalIgnoreCase) || !File.Exists(packagedExecutable))throw new NativeError("VALIDATION","Explorer command must use this installed application's executable.");
  // Check both keys before modifying either. Never overwrite another application's verb.
  if(request.Installed)foreach(var path in keys){ using var key=Registry.CurrentUser.OpenSubKey(path); if(key!=null && key.GetValue("ShellfoxOwner") as string!=Owner)throw new NativeError("AUTH_FAILED","An Explorer verb with this name belongs to another application. It was not changed."); }
  if(request.Installed){
   for(var i=0;i<keys.Length;i++){
    using var key=Registry.CurrentUser.CreateSubKey(keys[i]);
    key.SetValue("ShellfoxOwner",Owner); key.SetValue("","Open in Shellfox");
    key.SetValue("MUIVerb","Open in Shellfox");
    if(i==0)key.SetValue("MultiSelectModel","Single");else key.DeleteValue("MultiSelectModel",false);
    key.SetValue("Icon",packagedExecutable);
    using var command=key.CreateSubKey("command"); command.SetValue("",Command(packagedExecutable,i==1));
   }
  } else {
   foreach(var path in keys){ using var key=Registry.CurrentUser.OpenSubKey(path); if(key?.GetValue("ShellfoxOwner") as string==Owner){ key.Dispose(); Registry.CurrentUser.DeleteSubKeyTree(path,false); } }
  }
  return Status();
 }
}
