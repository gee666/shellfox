using System.Text;
using System.Text.Json;

namespace Shellfox.Native;

// WT has no documented atomic existing-only window dispatch. HWND/title verification cannot
// establish its internal name lookup or prevent the lookup from creating a new window.
public sealed class WindowLaunchPolicy {
 public const string AppendReason="Adding tabs is disabled: Windows Terminal's CLI can create an unexpected window when its target lookup fails. Select tabs in Windows Terminal manually; the manager will not dispatch or retry an append.";
 readonly string directory;
 public WindowLaunchPolicy(string userData){directory=Path.Combine(userData,"native-window-intents");Directory.CreateDirectory(directory);}
 public static void RequireInitial(Launch request){
  if(request.ExistingTarget!=null)throw new NativeError("UNSUPPORTED",AppendReason);
 }
 public void Reserve(Launch request){
  RequireInitial(request);Wire.Id(request.SessionId);Wire.Id(request.OperationId);
  var path=Path.Combine(directory,request.SessionId+".json");
  FileStream file;
  try{file=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.None);}
  catch(IOException) when(File.Exists(path)){throw new NativeError("UNSUPPORTED","A window launch was already reserved for this session, possibly before a crash or uncertain launch. Check that terminal manually and create a new session. Reusing its window intent is disabled.");}
  using(file){
   var bytes=Encoding.UTF8.GetBytes(JsonSerializer.Serialize(new {version=1,request.SessionId,request.OperationId,reservedAt=Wire.Now()},Wire.Json));
   file.Write(bytes);file.Flush(flushToDisk:true);
  }
  // Keep the reservation across helper/manager restarts. Do not turn an uncertain dispatch
  // into a second initial launch by accepting existingTarget:null. New sessions use new UUIDs.
 }
 public void ReserveReplacement(Launch request,BoundWindow previous){
  RequireInitial(request);SessionWindows.Validate(previous);Wire.Id(request.OperationId);
  if(previous.Target.SessionId!=request.SessionId || request.OperationId==previous.Generation)throw new NativeError("VALIDATION","Replacement needs the same session and a new operation.");
  // One successor per confirmed previous generation, even across crash/restart. Never clear
  // this reservation on timeout: that could create duplicate replacement windows.
  var path=Path.Combine(directory,request.SessionId+"-after-"+previous.Generation+".json");
  try{using var f=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.None);var bytes=Encoding.UTF8.GetBytes(JsonSerializer.Serialize(new {version=1,request.SessionId,request.OperationId,previousGeneration=previous.Generation},Wire.Json));f.Write(bytes);f.Flush(true);}
  catch(IOException) when(File.Exists(path)){throw new NativeError("RETRY_CONFIRM_REQUIRED","A replacement for this window generation was already dispatched or reserved. Check Windows Terminal manually; no automatic retry is allowed.");}
 }
 public static string[] SelectorArguments()=>["--window","new"];
}
