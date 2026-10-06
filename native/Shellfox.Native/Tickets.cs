using System.IO.Pipes;
using System.Management;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Json;

namespace Shellfox.Native;

public record Ticket(string TicketId,string Token,string PipeName,string HelperPath,string Cwd,DateTimeOffset ExpiresAt,string SessionId,string TabId,string OperationId,string ShellId,string ShellExecutable,string Kind="launch",string? TitleMarker=null,BoundWindow? Binding=null);
public sealed record Pending(Launch Request,Ticket Ticket,string File,DateTimeOffset IssuedAt);
public sealed record Authenticated(Pending Pending,Windows.ProcessInfo Shell);
public sealed class Tickets : IDisposable {
 readonly Dictionary<string,Pending> pending=new();
 readonly object gate=new();
 readonly string directory;
 readonly string helperPath;
 public string PipeName { get; }="shellfox-"+Guid.NewGuid().ToString("N");
 public Tickets(string userData,string helper){ directory=Path.Combine(userData,"launch-tickets"); helperPath=helper; Directory.CreateDirectory(directory); RestrictDirectory(directory); }
 static FileSystemSecurity Acl(bool dir){
  FileSystemSecurity acl=dir?new DirectorySecurity():new FileSecurity();
  acl.SetAccessRuleProtection(true,false);
  foreach(var sid in new[]{new SecurityIdentifier(Windows.UserSid),new SecurityIdentifier(WellKnownSidType.LocalSystemSid,null)})
   acl.AddAccessRule(new FileSystemAccessRule(sid,FileSystemRights.FullControl,dir?InheritanceFlags.ContainerInherit|InheritanceFlags.ObjectInherit:InheritanceFlags.None,PropagationFlags.None,AccessControlType.Allow));
  return acl;
 }
 static void RestrictDirectory(string path) => new DirectoryInfo(path).SetAccessControl((DirectorySecurity)Acl(true));
 public Pending Create(Launch request,BoundWindow? binding=null){
  var id=Guid.NewGuid().ToString("D"); var now=DateTimeOffset.UtcNow;
  var ticket=new Ticket(id,Convert.ToHexString(RandomNumberGenerator.GetBytes(32)),PipeName,helperPath,request.Cwd,now.AddSeconds(15),request.SessionId,request.TabId,request.OperationId,request.ShellId,request.ShellExecutable,binding==null?"launch":"adopt",request.TitleMarker,binding);
  if(binding!=null)ticket=ticket with {ExpiresAt=now.AddMinutes(2)};
  var file=Path.Combine(directory,id+".json");
  // The directory ACL protects creation; apply a non-inherited file ACL before exposing it to a shell.
  File.WriteAllText(file,JsonSerializer.Serialize(ticket,Wire.Json),new UTF8Encoding(false));
  new FileInfo(file).SetAccessControl((FileSecurity)Acl(false));
  var entry=new Pending(request,ticket,file,now);
  lock(gate)pending.Add(id,entry);
  return entry;
 }
 public static bool Auth(string expected,string actual,DateTimeOffset expires,DateTimeOffset now){
  if(now>=expires || actual.Length!=64)return false;
  try{return CryptographicOperations.FixedTimeEquals(Convert.FromHexString(expected),Convert.FromHexString(actual));}catch(FormatException){return false;}
 }
 public Authenticated Authenticate(RegisterRequest request,int clientPid){
  if(request.Version!=1 || request.ShellPid<=0)throw new NativeError("AUTH_FAILED","Invalid registration.");
  Wire.Id(request.TicketId);
  lock(gate){
   if(!pending.TryGetValue(request.TicketId,out var entry) || !Auth(entry.Ticket.Token,request.Token,entry.Ticket.ExpiresAt,DateTimeOffset.UtcNow))throw new NativeError("AUTH_FAILED","Registration ticket is invalid, expired or consumed.");
   var shell=Windows.Process(request.ShellPid); var client=Windows.Process(clientPid);
   if(!shell.SameUser || shell.Elevated || !client.SameUser || client.Elevated || !shell.Executable.Equals(entry.Request.ShellExecutable,StringComparison.OrdinalIgnoreCase) || !client.Executable.Equals(helperPath,StringComparison.OrdinalIgnoreCase) || entry.Ticket.Kind=="launch" && ulong.Parse(shell.Identity.StartTime)<(ulong)entry.IssuedAt.UtcDateTime.ToFileTimeUtc())throw new NativeError("AUTH_FAILED","Registration process identity does not match this launch.");
   using var lookup=new ManagementObject($"Win32_Process.Handle='{clientPid}'"); lookup.Get();
   if(Convert.ToInt32(lookup["ParentProcessId"])!=request.ShellPid || lookup["CreationDate"] is not string created || !Tracker.WmiCreationMatches(client.Identity,created) || !Windows.Same(client.Identity,Windows.Process(clientPid).Identity) || !Windows.Same(shell.Identity,Windows.Process(request.ShellPid).Identity))throw new NativeError("AUTH_FAILED","Registration helper is not a verified child of the claimed shell.");
   if(request.WtSession!=null)Wire.Id(request.WtSession);
   if(entry.Ticket.Kind=="adopt"){
    if(request.WtSession==null || entry.Ticket.Binding==null || Windows.Life(entry.Ticket.Binding.Target)!="alive")throw new NativeError("AUTH_FAILED","Explicit adoption requires a live bound WT window and WT_SESSION.");
    var target=Windows.Bind(entry.Request);
    if(target==null || target.Hwnd!=entry.Ticket.Binding.Target.Hwnd || !Windows.Same(target.Owner,entry.Ticket.Binding.Target.Owner))throw new NativeError("TARGET_LOST","The selected terminal's challenge title was not confirmed in the intended window. Rename a title-suppressed tab to the supplied marker and prepare a new ticket; no membership was changed.");
   }
   pending.Remove(request.TicketId); File.Delete(entry.File); return new(entry,shell);
  }
 }
 public bool Expire(string id){ lock(gate){ if(!pending.Remove(id,out var entry))return false; File.Delete(entry.File); return true; } }
 public void Dispose(){ lock(gate){ foreach(var p in pending.Values){try{File.Delete(p.File);}catch(IOException){} }pending.Clear();} }
 public static async Task<int> Register(string file,int shellPid){
  try{
   var info=new FileInfo(file); if(info.Length>65536)throw new NativeError("AUTH_FAILED","Ticket is too large.");
   var ticket=JsonSerializer.Deserialize<Ticket>(await File.ReadAllTextAsync(file),Wire.Json) ?? throw new NativeError("AUTH_FAILED","Missing ticket.");
   using var ct=new CancellationTokenSource(TimeSpan.FromSeconds(8));
   using var pipe=new NamedPipeClientStream(".",ticket.PipeName,PipeDirection.InOut,PipeOptions.Asynchronous|PipeOptions.CurrentUserOnly);
   await pipe.ConnectAsync(ct.Token);
   var request=JsonSerializer.Serialize(new RegisterRequest(1,ticket.TicketId,ticket.Token,shellPid,Environment.GetEnvironmentVariable("WT_SESSION")),Wire.Json)+"\n";
   await pipe.WriteAsync(Encoding.UTF8.GetBytes(request),ct.Token); await pipe.FlushAsync(ct.Token);
   var reply=await Wire.Line(pipe,ct.Token); using var json=JsonDocument.Parse(reply ?? "{}");
   if(json.RootElement.GetProperty("ok").GetBoolean()){
    // One-shot stdout only, captured by the integration script. Never print ticket/token data.
    Console.WriteLine(json.RootElement.GetProperty("value").GetRawText());return 0;
   }
   Console.Error.WriteLine("Shellfox registration rejected. Confirm the supplied tab marker in the intended native window; prepare a new ticket if needed.");return 1;
  }catch(Exception){ Console.Error.WriteLine("Shellfox registration unavailable. This shell remains usable."); return 1; }
 }
}
