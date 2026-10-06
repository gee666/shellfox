using System.Diagnostics;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
namespace Shellfox.Native;

public record LinuxTicket(string Id,string Token,string Socket,string Helper,Launch Request,DateTimeOffset Expires,long IssuedTicks);
public sealed class LinuxBroker : IDisposable {
 readonly CancellationTokenSource stop=new();readonly SemaphoreSlim output=new(1,1);readonly Stream stdout=Console.OpenStandardOutput();
 readonly Dictionary<string,LinuxTicket> tickets=new();readonly object ticketLock=new();readonly HashSet<string> sessions=new();
 readonly Dictionary<string,string> owners=new();Watch watch=new([],[]);Init? init;Socket? server;
 string socketName="\0shellfox-"+Guid.NewGuid().ToString("N");string helper="",bash="/usr/bin/bash";
 [StructLayout(LayoutKind.Sequential)] struct Credentials {public int Pid;public uint Uid;public uint Gid;}
 [DllImport("libc",SetLastError=true)] static extern int getsockopt(int fd,int level,int option,out Credentials value,ref uint length);
 async Task Send(object value){var bytes=Encoding.UTF8.GetBytes(JsonSerializer.Serialize(value,Wire.Json)+"\n");if(bytes.Length>Wire.MaxFrame)throw new NativeError("NATIVE_UNAVAILABLE","Frame limit exceeded.");await output.WaitAsync();try{await stdout.WriteAsync(bytes);await stdout.FlushAsync();}finally{output.Release();}}
 async Task Event(object value)=>await Send(new {version=1,kind="event",@event=value});
 public async Task Run(){using var stdin=Console.OpenStandardInput();try{while(!stop.IsCancellationRequested){var line=await Wire.Line(stdin,stop.Token);if(line==null)break;var frame=JsonSerializer.Deserialize<Frame>(line,Wire.Json)!;Wire.Id(frame.Id);if(frame.Version!=1||frame.Kind!="request")break;object result;try{result=Wire.Ok(await Dispatch(frame.Method,frame.Payload));}catch(NativeError e){result=Wire.Fail(e.Code,e.Message);}catch(Exception){result=Wire.Fail("NATIVE_UNAVAILABLE","Linux operation failed. Check dependencies and local permissions.");}await Send(new {version=1,kind="response",id=frame.Id,result});}}catch(Exception){}finally{Dispose();}}
 async Task<object> Dispatch(string method,JsonElement p){
  if(method=="initialize")return Initialize(Wire.Read<Init>(p));if(init==null)throw new NativeError("NATIVE_UNAVAILABLE","Initialize first.");
  switch(method){
   case "launch":return await Launch(Wire.Read<Launch>(p));
   case "setWatch":{var next=Wire.Read<Watch>(p);if(next.Tabs.Length>1000||next.Rules.Length>100)throw new NativeError("VALIDATION","Watch limit exceeded.");foreach(var t in next.Tabs){Wire.Id(t.SessionId);Wire.Id(t.TabId);if(t.SessionId!=t.Registration.SessionId||t.TabId!=t.Registration.TabId||t.Registration.Shell.Pid<=0)throw new NativeError("VALIDATION","Invalid root.");}Volatile.Write(ref watch,next);return new {configured=true};}
   case "getExplorerIntegration":return new {supported=false,installed=false,folderItemInstalled=false,backgroundInstalled=false,reason="Explorer/Nautilus integration is not provided on Ubuntu."};
   case "focus":case "verifyTarget":case "setExplorerIntegration":throw new NativeError("UNSUPPORTED","Ubuntu mode creates GNOME Terminal windows and tracks registered Bash roots. Focus, reopen, tab adoption and file-manager integration are not supported.");
   default:throw new NativeError("UNSUPPORTED","Unknown Linux method.");
  }
 }
 object Initialize(Init input){
  if(!OperatingSystem.IsLinux()||RuntimeInformation.ProcessArchitecture!=Architecture.X64||Proc.getuid()==0||Proc.getuid()!=Proc.geteuid())throw new NativeError("UNSUPPORTED","Use Ubuntu/Linux x64 as a normal non-root user.");
  helper=Path.Combine(input.HelperDir,"Shellfox.Linux");if(!File.Exists(helper)||!File.Exists(Path.Combine(input.ShellScriptDir,"bootstrap.bash"))||Proc.Canonical(helper)!=Proc.Canonical(Environment.ProcessPath!))throw new NativeError("DEPENDENCY_MISSING","Linux helper/bootstrap missing or wrong executable.");
  Directory.CreateDirectory(input.UserDataDir);var ticketDir=Path.Combine(input.UserDataDir,"launch-tickets-linux");Directory.CreateDirectory(ticketDir);File.SetUnixFileMode(ticketDir,UnixFileMode.UserRead|UnixFileMode.UserWrite|UnixFileMode.UserExecute);
  if((File.GetUnixFileMode(ticketDir)&(UnixFileMode.GroupRead|UnixFileMode.GroupWrite|UnixFileMode.OtherRead|UnixFileMode.OtherWrite))!=0)throw new NativeError("AUTH_FAILED","Ticket directory cannot enforce private Unix permissions. Use local Linux userData.");
  bash=Proc.Canonical("/bin/bash");init=input;server=new Socket(AddressFamily.Unix,SocketType.Stream,ProtocolType.Unspecified);server.Bind(new UnixDomainSocketEndPoint(socketName));server.Listen(8);_=Task.Run(Accept);_=Task.Run(Poll);
  var available=File.Exists("/usr/bin/gnome-terminal")&&File.Exists(bash)&&!string.IsNullOrEmpty(Environment.GetEnvironmentVariable("DBUS_SESSION_BUS_ADDRESS"))&&(!string.IsNullOrEmpty(Environment.GetEnvironmentVariable("DISPLAY"))||!string.IsNullOrEmpty(Environment.GetEnvironmentVariable("WAYLAND_DISPLAY")));
  return new {platform="linux",arch="x64",adapterId="gnome-terminal",available,terminalVersion=(string?)null,capabilities=new {createWindow=available,addTab=false,focusWindow=false,activateTab=false,splitPane=false,attachExisting=false,closeTerminal=false,commandExitStatus=false,processTracking=true,explorerContextMenu=false},shells=new[]{new {id="bash",executable=bash,available=File.Exists(bash),reason=(string?)null}},reasons=new[]{"Ubuntu test mode: GNOME Terminal + Bash, initial windows and scoped process tracking only. No safe focus/reopen/tab adoption API is implemented.","Requires gnome-terminal, an interactive display and a session D-Bus address. Detached/remote/elevated/multiplexer activity and short-lived processes are not guaranteed."}};
 }
 public static string[] Arguments(Launch r,string helper,string ticket,string script)=>["--window","--title",r.TitleMarker,"--working-directory",r.Cwd,"--","/usr/bin/env","SHELLFOX_TICKET="+ticket,"SHELLFOX_HELPER="+helper,"/usr/bin/bash","--rcfile",script,"-i"];
 async Task<object> Launch(Launch r){
  if(!OperatingSystem.IsLinux())throw new NativeError("UNSUPPORTED","Linux is required.");
  Wire.Id(r.SessionId);Wire.Id(r.TabId);Wire.Id(r.OperationId);
  if(r.ExistingTarget!=null)throw new NativeError("UNSUPPORTED","Appending is disabled on GNOME Terminal.");
  if(r.ShellId!="bash"||Proc.Canonical(r.ShellExecutable)!=bash||r.TitleMarker!="SHELLFOX:"+r.SessionId+":"+r.TabId)throw new NativeError("VALIDATION","Only discovered Bash and generated IDs are allowed.");
  Proc.DirectoryPath(r.Cwd);if(!File.Exists("/usr/bin/gnome-terminal"))throw new NativeError("DEPENDENCY_MISSING","Install GNOME Terminal before creating a session.");
  if(!sessions.Add(r.SessionId))throw new NativeError("RETRY_CONFIRM_REQUIRED","This session was already dispatched. Inspect the terminal; no automatic retry.");
  var id=Guid.NewGuid().ToString();var ticket=new LinuxTicket(id,Convert.ToHexString(RandomNumberGenerator.GetBytes(32)),socketName,helper,r,DateTimeOffset.UtcNow.AddSeconds(15),Proc.NowTicks);
  var path=Path.Combine(init!.UserDataDir,"launch-tickets-linux",id+".json");using(var f=new FileStream(path,new FileStreamOptions{Mode=FileMode.CreateNew,Access=FileAccess.Write,UnixCreateMode=UnixFileMode.UserRead|UnixFileMode.UserWrite})){var b=Encoding.UTF8.GetBytes(JsonSerializer.Serialize(ticket,Wire.Json));f.Write(b);}
  lock(ticketLock)tickets[id]=ticket;
  try{var start=new ProcessStartInfo("/usr/bin/gnome-terminal"){UseShellExecute=false};foreach(var a in Arguments(r,helper,path,Path.Combine(init.ShellScriptDir,"bootstrap.bash")))start.ArgumentList.Add(a);using var process=Process.Start(start)??throw new Exception();}
  catch(Exception){Expire(id);throw new NativeError("LAUNCH_FAILED","GNOME Terminal could not start.");}
  _=Task.Run(async()=>{try{await Task.Delay(15000,stop.Token);if(Expire(id))await Event(new {type="operation-error",sessionId=r.SessionId,tabId=r.TabId,operationId=r.OperationId,error=new {code="REGISTRATION_TIMEOUT",message="Bash registration not confirmed. A native window may exist; inspect it before retrying.",retryable=false}});}catch(OperationCanceledException){}});
  await Task.CompletedTask;return new {operationId=r.OperationId,dispatch="started",target=(object?)null};
 }
 bool Expire(string id){lock(ticketLock){if(!tickets.Remove(id))return false;File.Delete(Path.Combine(init!.UserDataDir,"launch-tickets-linux",id+".json"));return true;}}
 async Task Accept(){while(!stop.IsCancellationRequested){try{var client=await server!.AcceptAsync(stop.Token);_=Task.Run(()=>RegisterClient(client));}catch(OperationCanceledException){break;}catch(Exception){break;}}}
 async Task RegisterClient(Socket socket){using(socket)using(var stream=new NetworkStream(socket,false))using(var ct=CancellationTokenSource.CreateLinkedTokenSource(stop.Token)){
  ct.CancelAfter(8000);try{
   uint length=12;if(getsockopt((int)socket.Handle,1,17,out var peer,ref length)!=0||peer.Uid!=Proc.getuid())throw new NativeError("AUTH_FAILED","Registration peer not current user.");
   var frame=JsonSerializer.Deserialize<RegisterRequest>((await Wire.Line(stream,ct.Token))!,Wire.Json)!;
   LinuxTicket ticket;lock(ticketLock){if(frame.Version!=1||!tickets.TryGetValue(frame.TicketId,out ticket!)||DateTimeOffset.UtcNow>=ticket.Expires||frame.Token.Length!=64||!CryptographicOperations.FixedTimeEquals(Convert.FromHexString(frame.Token),Convert.FromHexString(ticket.Token)))throw new NativeError("AUTH_FAILED","Invalid, expired or replayed ticket.");}
   var client=Proc.Read(peer.Pid);var root=Proc.Read(frame.ShellPid);
   if(client.Parent!=root.Identity.Pid||client.Executable!=Proc.Canonical(helper)||root.Executable!=bash||Proc.Stat(File.ReadAllText($"/proc/{frame.ShellPid}/stat")).Ticks<ticket.IssuedTicks||!Proc.Same(root.Identity,Proc.Read(root.Identity.Pid).Identity)||!Proc.Same(client.Identity,Proc.Read(peer.Pid).Identity))throw new NativeError("AUTH_FAILED","Shell/client identity does not match dispatch.");
   if(!Expire(ticket.Id))throw new NativeError("AUTH_FAILED","Ticket already consumed.");
   var reg=new Registration(ticket.Request.SessionId,ticket.Request.TabId,ticket.Request.OperationId,root.Identity,root.Executable,ticket.Request.Cwd,Wire.Now());
   await Event(new {type="registered",registration=reg,target=(object?)null});await stream.WriteAsync(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(Wire.Ok(new {registered=true}),Wire.Json)+"\n"),ct.Token);
  }catch(Exception){try{await stream.WriteAsync(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(Wire.Fail("AUTH_FAILED","Registration rejected."),Wire.Json)+"\n"),ct.Token);}catch(Exception){}}
 }}
 async Task Poll(){while(!stop.IsCancellationRequested){try{await Task.Delay(2000,stop.Token);var current=Volatile.Read(ref watch);if(current.Tabs.Length==0)continue;var result=Observe(current);await output.WaitAsync();try{if(!ReferenceEquals(current,Volatile.Read(ref watch)))continue;var bytes=Encoding.UTF8.GetBytes(JsonSerializer.Serialize(new {version=1,kind="event",@event=new {type="observations",items=result}},Wire.Json)+"\n");if(bytes.Length>Wire.MaxFrame)throw new Exception();await stdout.WriteAsync(bytes);await stdout.FlushAsync();}finally{output.Release();}}catch(OperationCanceledException){break;}catch(Exception){await Event(new {type="observations",items=Volatile.Read(ref watch).Tabs.Select(t=>new Observation(t.SessionId,t.TabId,Wire.Now(),"unavailable","unknown",0,"Linux process snapshot unavailable.")).ToArray()});}}}
 Observation[] Observe(Watch w){
  var inaccessible=new List<int>();var rows=new Dictionary<int,LinuxProcess>();foreach(var d in Directory.EnumerateDirectories("/proc")){if(int.TryParse(Path.GetFileName(d),out var pid)){try{rows[pid]=Proc.Read(pid);}catch(Exception){try{inaccessible.Add(Proc.Stat(File.ReadAllText(d+"/stat")).Parent);}catch(Exception){}}}if(rows.Count>20000)throw new Exception();}
  var roots=w.Tabs.ToDictionary(t=>Proc.Key(t.Registration.Shell),t=>t.TabId);var live=rows.Values.Select(p=>Proc.Key(p.Identity)).ToHashSet();foreach(var key in owners.Keys.ToArray())if(!live.Contains(key)||!roots.ContainsKey(owners[key]))owners.Remove(key);
  string? Owner(LinuxProcess p,HashSet<int> seen){if(!seen.Add(p.Identity.Pid))return null;var key=Proc.Key(p.Identity);if(roots.ContainsKey(key)){owners[key]=key;return key;}if(rows.TryGetValue(p.Parent,out var parent)&&BigIntegerBirth(parent.Identity)<=BigIntegerBirth(p.Identity)){var owner=Owner(parent,seen);if(owner!=null)return owners[key]=owner;}return owners.GetValueOrDefault(key);}
  foreach(var p in rows.Values)Owner(p,new());
  return w.Tabs.Select(t=>{string state="alive";string? reason=null;int count=0;try{if(!Proc.Same(Proc.Read(t.Registration.Shell.Pid).Identity,t.Registration.Shell))state="exited";}catch(NativeError e){state=e.Code=="NOT_FOUND"?"exited":"unavailable";reason=state=="unavailable"?e.Message:null;}if(state=="alive"){if(!rows.ContainsKey(t.Registration.Shell.Pid))reason="Root snapshot unavailable.";if(inaccessible.Any(parent=>rows.TryGetValue(parent,out var p)&&owners.GetValueOrDefault(Proc.Key(p.Identity))==Proc.Key(t.Registration.Shell)))reason="A descendant is inaccessible or elevated.";foreach(var p in rows.Values.Where(p=>p.Identity!=t.Registration.Shell&&owners.GetValueOrDefault(Proc.Key(p.Identity))==Proc.Key(t.Registration.Shell))){try{if(w.Rules.Any(r=>r.Enabled&&Proc.Match(r,p)))count++;}catch(NativeError e){reason=e.Message;}}}return new Observation(t.SessionId,t.TabId,Wire.Now(),state,state=="alive"&&reason==null?"healthy":"unknown",count,reason);}).ToArray();
 }
 static System.Numerics.BigInteger BigIntegerBirth(Identity p)=>System.Numerics.BigInteger.Parse(p.StartTime);
 public static async Task<int> Register(string file,int shellPid){try{if(new FileInfo(file).Length>Wire.MaxFrame)return 1;var t=JsonSerializer.Deserialize<LinuxTicket>(await File.ReadAllTextAsync(file),Wire.Json)!;using var ct=new CancellationTokenSource(8000);using var s=new Socket(AddressFamily.Unix,SocketType.Stream,ProtocolType.Unspecified);await s.ConnectAsync(new UnixDomainSocketEndPoint(t.Socket),ct.Token);using var stream=new NetworkStream(s,false);await stream.WriteAsync(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(new RegisterRequest(1,t.Id,t.Token,shellPid),Wire.Json)+"\n"),ct.Token);using var reply=JsonDocument.Parse((await Wire.Line(stream,ct.Token))!);return reply.RootElement.GetProperty("ok").GetBoolean()?0:1;}catch(Exception){Console.Error.WriteLine("Shellfox registration unavailable; Bash remains usable.");return 1;}}
 public void Dispose(){if(stop.IsCancellationRequested)return;stop.Cancel();server?.Dispose();lock(ticketLock){foreach(var id in tickets.Keys.ToArray())Expire(id);}}
}
