using System.Diagnostics;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;

namespace Shellfox.Native;

public sealed class Broker : IDisposable {
 readonly CancellationTokenSource stopped=new();
 readonly SemaphoreSlim output=new(1,1);
 readonly Stream stdout=Console.OpenStandardOutput();
 readonly Tracker tracker=new();
 readonly Dictionary<string,Target> targets=new();
 readonly object targetLock=new();
 readonly HashSet<string> operations=new();
 readonly System.Collections.Concurrent.ConcurrentDictionary<string,byte> awaitingRegistration=new();
 readonly RegistrationLedger registrations=new();
 readonly SessionWindows sessionWindows=new();
 Watch watch=new([],[]);
 Init? init;
 Tickets? tickets;
 Explorer? explorer;
 WindowLaunchPolicy? windowLaunches;
 string? wt;
 WindowLifetime? lifetime;
 public async Task Send(object message,Func<bool>? stillCurrent=null){
  var bytes=Encoding.UTF8.GetBytes(JsonSerializer.Serialize(message,Wire.Json)+"\n");
  if(bytes.Length>Wire.MaxFrame)throw new NativeError("NATIVE_UNAVAILABLE","Outbound frame exceeds supported limit.");
  await output.WaitAsync();try{if(stillCurrent!=null && !stillCurrent())return;await stdout.WriteAsync(bytes);await stdout.FlushAsync();}finally{output.Release();}
 }
 async Task Event(object e,Func<bool>? stillCurrent=null)=> await Send(new { version=1,kind="event",@event=e },stillCurrent);
 async Task WindowEvent(object e,Func<bool>? stillCurrent=null)=>await Send(new {version=1,kind="session-window-event",@event=e},stillCurrent);
 bool Unbound(string sessionId){lock(targetLock)return !targets.ContainsKey(sessionId);}
 async Task Destroyed(Target target){
  if(stopped.IsCancellationRequested)return;
  try{Windows.Verify(target);return;}catch(NativeError){}
  bool removed;lock(targetLock){removed=targets.TryGetValue(target.SessionId,out var bound) && ReferenceEquals(bound,target);if(removed)targets.Remove(target.SessionId);}
  if(removed && !stopped.IsCancellationRequested)await Event(new {type="target-lost",sessionId=target.SessionId,reason="The bound HWND was destroyed or its ownership changed. No replacement was launched."},()=>Unbound(target.SessionId));
 }
 public async Task Run(){
  using var input=Console.OpenStandardInput();
  try{
   while(!stopped.IsCancellationRequested){
    var line=await Wire.Line(input,stopped.Token); if(line==null)break;
    Frame frame;
    try{ frame=JsonSerializer.Deserialize<Frame>(line,Wire.Json) ?? throw new JsonException(); Wire.Id(frame.Id); if(frame.Version!=1 || frame.Kind!="request" || frame.Payload.ValueKind!=JsonValueKind.Object)throw new JsonException(); }
    catch(Exception){ await Event(new {type="unavailable",error=new {code="NATIVE_UNAVAILABLE",message="Invalid native protocol frame.",retryable=false}}); break; }
    object result;
    try { result=Wire.Ok(await Dispatch(frame.Method,frame.Payload)); }
    catch(NativeError e){result=Wire.Fail(e.Code,e.Message,e.Code is "FOCUS_DENIED" or "TARGET_LOST");}
    catch(JsonException){result=Wire.Fail("VALIDATION","Malformed native request payload.");}
    catch(Exception){result=Wire.Fail("NATIVE_UNAVAILABLE","Native operation failed. Verify local prerequisites and access permissions.");}
    await Send(new { version=1,id=frame.Id,kind="response",result });
    if(frame.Method=="dispose")break;
   }
  }catch(OperationCanceledException){}catch(Exception){ /* Broken transport shuts down without touching shells. */ }
  finally{Dispose();}
 }
 async Task<object> Dispatch(string method,JsonElement payload){
  if(method=="initialize")return Initialize(Wire.Read<Init>(payload));
  if(init==null)throw new NativeError("NATIVE_UNAVAILABLE","Native backend is not initialized.");
  switch(method){
   case "launch": return await Launch(Wire.Read<Launch>(payload));
   case "reopenWindow": {var r=Wire.Read<ReopenRequest>(payload);return await Launch(r.Request,r.Previous);}
   case "prepareRegistration": return Prepare(Wire.Read<PrepareRegistration>(payload));
   case "getWindowMembership": {var b=Wire.Read<BoundWindow>(payload);RequireCurrent(b);return SessionWindows.Snapshot(b,sessionWindows.Members(b.Target.SessionId),Windows.Life(b.Target));}
   case "focusSessionWindow": {var b=Wire.Read<BoundWindow>(payload);RequireCurrent(b);var focused=await Windows.FocusPhysical(b.Target);RequireCurrent(b);return focused;}
   case "restoreSessionWindows": {var r=Wire.Read<RestoreWindows>(payload);foreach(var b in r.Bindings)if(Windows.Life(b.Target)=="alive")Windows.Verify(b.Target);sessionWindows.Restore(r);return new {restored=true};}
   case "focus": return await Windows.Focus(Wire.Read<Target>(payload));
   case "verifyTarget": { var t=Windows.Verify(Wire.Read<Target>(payload)); lock(targetLock)targets[t.SessionId]=t; return t; }
   case "setWatch": { var next=Wire.Read<Watch>(payload); Tracker.Validate(next); Volatile.Write(ref watch,next); return new {configured=true}; }
   case "getExplorerIntegration": if(payload.EnumerateObject().Any())throw new NativeError("VALIDATION","Expected empty payload."); return explorer!.Status();
   case "setExplorerIntegration": return explorer!.Set(Wire.Read<ExplorerRequest>(payload));
   case "dispose": return new {disposed=true};
   default: throw new NativeError("UNSUPPORTED","Unknown native method.");
  }
 }
 object Initialize(Init input){
  if(init!=null)throw new NativeError("VALIDATION","Already initialized.");
  if(!OperatingSystem.IsWindows() || !Environment.Is64BitProcess)throw new NativeError("UNSUPPORTED","Native backend requires Windows x64.");
  var helper=Path.Combine(input.HelperDir,"Shellfox.Native.exe");
  if(!Path.IsPathFullyQualified(input.UserDataDir) || !Path.IsPathFullyQualified(input.HelperDir) || !Path.IsPathFullyQualified(input.ShellScriptDir) || !File.Exists(helper) || !File.Exists(Path.Combine(input.ShellScriptDir,"bootstrap.ps1")))throw new NativeError("DEPENDENCY_MISSING","Bundled native helper or PowerShell bootstrap is missing.");
  if(!Path.GetFullPath(helper).Equals(Environment.ProcessPath,StringComparison.OrdinalIgnoreCase))throw new NativeError("VALIDATION","Helper location does not match running broker.");
  if(input.PackagedExecutable!=null && (!Path.IsPathFullyQualified(input.PackagedExecutable) || !File.Exists(input.PackagedExecutable) || !input.PackagedExecutable.EndsWith(".exe",StringComparison.OrdinalIgnoreCase)))throw new NativeError("VALIDATION","Invalid packaged executable.");
  wt=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),"Microsoft","WindowsApps","wt.exe");
  var available=File.Exists(wt); var shells=Windows.Shells();
  tickets=new(input.UserDataDir,helper); explorer=new(input.PackagedExecutable);windowLaunches=new(input.UserDataDir);
  lifetime=new WindowLifetime(hwnd=>{
   Target[] candidates;lock(targetLock)candidates=targets.Values.Where(t=>t.Hwnd==hwnd).ToArray();
   foreach(var t in candidates)_=Task.Run(()=>Destroyed(t));
  });
  init=input;
  _=Task.Run(PipeLoop); _=Task.Run(Poll);
  return new {platform="win32",arch="x64",adapterId="windows-terminal",available=available && shells.Count>0,terminalVersion=(string?)null,
   capabilities=new {createWindow=available,addTab=false,focusWindow=available,activateTab=false,splitPane=false,attachExisting=false,closeTerminal=false,commandExitStatus=false,processTracking=true,explorerContextMenu=input.PackagedExecutable!=null},
   shells=new[]{"pwsh","windows-powershell"}.Select(id=>new {id,executable=shells.GetValueOrDefault(id)??(id=="pwsh"?Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),"PowerShell","7","pwsh.exe"):Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows),"System32","WindowsPowerShell","v1.0","powershell.exe")),available=shells.ContainsKey(id),reason=shells.ContainsKey(id)?null:"Supported standard-location PowerShell is not installed."}),
   reasons=new[]{WindowLaunchPolicy.AppendReason,"Initial binding/explicit registration requires a visible association marker. Automatic tab discovery/move detection is unavailable; moved/new PowerShell tabs require register-session.ps1. Session-window reopen/focus/adoption uses the extension API. Windows can deny focus; specific tab activation and exit codes remain unsupported.","Tracking is local, non-elevated registered PowerShell descendants only. Very short-lived, detached, WSL and remote processes are not guaranteed. Nonstandard agent layouts require process-rule configuration."}.Concat(available?Array.Empty<string>():new[]{"Windows Terminal wt.exe execution alias is missing."}) };
 }
 void RequireCurrent(BoundWindow binding){SessionWindows.Validate(binding);if(!sessionWindows.IsCurrent(binding))throw new NativeError("TARGET_LOST","This is not the current authenticated window generation.");}
 object Prepare(PrepareRegistration input){
  var r=input.Request;ValidateLaunch(r);RequireCurrent(input.Binding);
  if(r.ExistingTarget!=null || r.SessionId!=input.Binding.Target.SessionId)throw new NativeError("VALIDATION","Adoption requires a matching current session window.");
  if(Windows.Life(input.Binding.Target)!="alive")throw new NativeError("TARGET_LOST","The destination window is closed or inaccessible.");
  Windows.Shell(r.ShellId,r.ShellExecutable);Windows.LocalDirectory(r.Cwd);
  var script=Path.Combine(init!.ShellScriptDir,"register-session.ps1");if(!File.Exists(script))throw new NativeError("DEPENDENCY_MISSING","Explicit registration script is missing.");
  var pending=tickets!.Create(r,input.Binding);
  _=Task.Run(async()=>{try{await Task.Delay(TimeSpan.FromMinutes(2),stopped.Token);tickets.Expire(pending.Ticket.TicketId);}catch(OperationCanceledException){}});
  return new {ticketPath=pending.File,scriptPath=script,expiresAt=pending.Ticket.ExpiresAt.UtcDateTime.ToString("yyyy-MM-ddTHH:mm:ss.fffffffZ"),titleMarker=r.TitleMarker};
 }
 static void ValidateLaunch(Launch request){Wire.Id(request.SessionId);Wire.Id(request.TabId);Wire.Id(request.OperationId);if(request.WindowName!="shellfox-"+request.SessionId || request.TitleMarker!="SHELLFOX:"+request.SessionId+":"+request.TabId)throw new NativeError("VALIDATION","Launch names must be generated from UUIDs.");}
 async Task<object> Launch(Launch request,BoundWindow? previous=null){
  Wire.Id(request.SessionId);Wire.Id(request.TabId);Wire.Id(request.OperationId);
  if(request.WindowName!="shellfox-"+request.SessionId || request.TitleMarker!="SHELLFOX:"+request.SessionId+":"+request.TabId)throw new NativeError("VALIDATION","Launch names must be generated from session/tab UUIDs.");
  WindowLaunchPolicy.RequireInitial(request);
  if(previous==null){
   if(Volatile.Read(ref watch).Tabs.Any(t=>t.SessionId==request.SessionId))throw new NativeError("UNSUPPORTED",WindowLaunchPolicy.AppendReason);
   lock(targetLock)if(targets.ContainsKey(request.SessionId))throw new NativeError("UNSUPPORTED",WindowLaunchPolicy.AppendReason);
  }else{
   RequireCurrent(previous);
   if(previous.Target.SessionId!=request.SessionId || Windows.Life(previous.Target)!="closed")throw new NativeError("RETRY_CONFIRM_REQUIRED","Previous window is still live or closure cannot be confirmed. No replacement was dispatched.");
  }
  Windows.Shell(request.ShellId,request.ShellExecutable);
  Windows.LocalDirectory(request.Cwd);
  if(wt==null || !File.Exists(wt))throw new NativeError("DEPENDENCY_MISSING","Windows Terminal execution alias is missing.");
  if(Windows.List().Any(w=>Windows.Marker(w.Title,"SHELLFOX:"+request.SessionId+":")))throw new NativeError("TARGET_AMBIGUOUS","A window already exists for this session; an unverified launch is not retried automatically.");
  if(!operations.Add(request.OperationId))throw new NativeError("VALIDATION","Launch operation was already dispatched.");
  if(operations.Count>10000)throw new NativeError("VALIDATION","Launch-operation limit reached. Restart manager without relaunching live sessions.");
  if(previous==null)windowLaunches!.Reserve(request);else windowLaunches!.ReserveReplacement(request,previous);
  var pending=tickets!.Create(request);
  awaitingRegistration.TryAdd(request.OperationId,0);
  registrations.Begin(request.TabId,request.OperationId);
  try{
   string Data(string s)=>Convert.ToBase64String(Encoding.UTF8.GetBytes(s));
   var script=Path.Combine(init!.ShellScriptDir,"bootstrap.ps1");
   var code="$s=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+Data(script)+"'));$t=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+Data(pending.File)+"'));& $s -TicketPath $t;Remove-Variable s,t -ErrorAction SilentlyContinue";
   var start=new ProcessStartInfo(wt){UseShellExecute=false,CreateNoWindow=true};
   foreach(var arg in WindowLaunchPolicy.SelectorArguments().Concat(new[]{"new-tab","--title",request.TitleMarker,request.ShellExecutable,"-NoLogo","-NoExit","-ExecutionPolicy","Bypass","-EncodedCommand",Convert.ToBase64String(Encoding.Unicode.GetBytes(code))}))start.ArgumentList.Add(arg);
   // Always create a new dedicated window, never resolve a name/ID or the user's last window.
   // request.WindowName remains a compatibility intent tag, not a verified WT routing identity.
   // No user CWD, shell string, cmd.exe, semicolon batch, or token reaches the WT command line.
   using var launched=Process.Start(start) ?? throw new NativeError("LAUNCH_FAILED","Windows Terminal launcher could not be started.");
  }catch(Exception){awaitingRegistration.TryRemove(request.OperationId,out _);tickets.Expire(pending.Ticket.TicketId);throw new NativeError("LAUNCH_FAILED","Windows Terminal could not be started.");}
  _=Task.Run(async()=>{
   try{await Task.Delay(TimeSpan.FromSeconds(15),stopped.Token);tickets.Expire(pending.Ticket.TicketId);if(awaitingRegistration.TryRemove(request.OperationId,out _))await OperationError(request,"REGISTRATION_TIMEOUT","Shell registration was not confirmed within 15 seconds. A terminal may already exist. Check Windows Terminal before retrying.");}catch(OperationCanceledException){}
  });
  await Task.CompletedTask;
  return new {operationId=request.OperationId,dispatch="started",target=(Target?)null};
 }
 async Task OperationError(Launch request,string code,string message)=>await Event(new {type="operation-error",sessionId=request.SessionId,tabId=request.TabId,operationId=request.OperationId,error=new {code,message,retryable=true}});
 async Task PipeLoop(){
  while(!stopped.IsCancellationRequested){
   try{
    var pipe=new NamedPipeServerStream(tickets!.PipeName,PipeDirection.InOut,8,PipeTransmissionMode.Byte,PipeOptions.Asynchronous|PipeOptions.CurrentUserOnly,4096,4096);
    await pipe.WaitForConnectionAsync(stopped.Token);
    _=Task.Run(()=>Connection(pipe));
   }catch(OperationCanceledException){break;}catch(Exception){await Task.Delay(250,stopped.Token);}
  }
 }
 async Task Connection(NamedPipeServerStream pipe){
  using(pipe)using(var ct=CancellationTokenSource.CreateLinkedTokenSource(stopped.Token)){
   ct.CancelAfter(TimeSpan.FromSeconds(8));
   try{
    if(!Windows.GetNamedPipeClientProcessId(pipe.SafePipeHandle,out var clientPid))throw new NativeError("AUTH_FAILED","Cannot verify registration client.");
    var line=await Wire.Line(pipe,ct.Token);
    var request=JsonSerializer.Deserialize<RegisterRequest>(line??"",Wire.Json)??throw new NativeError("AUTH_FAILED","Invalid registration frame.");
    var authenticated=tickets!.Authenticate(request,(int)clientPid);
    var pending=authenticated.Pending;
    var shell=authenticated.Shell;
    var registration=new Registration(pending.Request.SessionId,pending.Request.TabId,pending.Request.OperationId,shell.Identity,shell.Executable,pending.Request.Cwd,Wire.Now());
    if(pending.Ticket.Kind=="adopt"){
     var binding=pending.Ticket.Binding!;RequireCurrent(binding);
     var adoption=sessionWindows.Adopt(registration,request.WtSession,binding);
     await WindowEvent(new {type="adopted",member=adoption.Member,previous=adoption.Previous,source="manual"});
     await pipe.WriteAsync(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(Wire.Ok(adoption.Member.Registration),Wire.Json)+"\n"),ct.Token);
     return;
    }
    Target? target=null;
    for(var attempt=0;attempt<20 && target==null;attempt++){target=Windows.Bind(pending.Request);if(target==null)await Task.Delay(100,ct.Token);}
    var delivered=pending.Request.ExistingTarget==null || target!=null && target.Hwnd==pending.Request.ExistingTarget.Hwnd && Windows.Same(target.Owner,pending.Request.ExistingTarget.Owner);
    var accepted=registrations.Register(registration,()=>{if(delivered && target!=null)lock(targetLock)targets[target.SessionId]=target;});
    if(accepted && delivered && target!=null){
     var binding=new BoundWindow(target,registration.OperationId);sessionWindows.Remember(binding);
     var adoption=sessionWindows.Adopt(registration,request.WtSession,binding);
     await WindowEvent(new {type="bound",binding},()=>sessionWindows.IsCurrent(binding));
     await WindowEvent(new {type="adopted",member=adoption.Member,previous=adoption.Previous,source="launch"});
    }
    if(!delivered && pending.Request.ExistingTarget is { } expected){
     await Event(new {type="registered",registration,target=(Target?)null});
     var unexpected=target==null?"An unexpected window may exist.":"Unexpected verified HWND "+target.Hwnd+" was observed instead of "+expected.Hwnd+".";
     await OperationError(pending.Request,"TARGET_LOST","New shell registered but delivery to the expected HWND could not be verified. "+unexpected+" Inspect Windows Terminal; do not auto-retry.");
    }else{
     await Event(new {type="registered",registration,target});
    }
    awaitingRegistration.TryRemove(pending.Request.OperationId,out _);
    await pipe.WriteAsync(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(Wire.Ok(registration),Wire.Json)+"\n"),ct.Token);
   }catch(NativeError e){
    try{await pipe.WriteAsync(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(Wire.Fail(e.Code,e.Message),Wire.Json)+"\n"),ct.Token);}catch(Exception){}
   }catch(Exception){
    try{await pipe.WriteAsync(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(Wire.Fail("AUTH_FAILED","Registration was rejected or could not be verified."),Wire.Json)+"\n"),ct.Token);}catch(Exception){}
   }
  }
 }
 async Task Poll(){
  while(!stopped.IsCancellationRequested){
   try{
    await Task.Delay(2000,stopped.Token);
    var current=Volatile.Read(ref watch);
    if(current.Tabs.Length>0){
     Observation[] observed;
     try{observed=tracker.Observe(current,Tracker.Snapshot());}
     catch(Exception){observed=current.Tabs.Select(t=>new Observation(t.SessionId,t.TabId,Wire.Now(),"unavailable","unknown",0,"Process snapshot unavailable. No healthy waiting state can be inferred.")).ToArray();}
     // Check at the stdout write boundary. An old poll cannot close a retried tab after a
     // newer registration event, or publish results from a superseded full watch replacement.
     await Event(new {type="observations",items=observed},()=>ReferenceEquals(current,Volatile.Read(ref watch)) && registrations.Allows(current) && sessionWindows.Allows(current));
    }
    foreach(var binding in sessionWindows.Bindings()){
     if(Windows.Life(binding.Target)=="closed" && sessionWindows.MarkLost(binding))await WindowEvent(new {type="lost",binding,reason="Native window closed; the manager session survives. Reopen is an explicit new-window operation."},()=>sessionWindows.IsCurrent(binding));
    }
    foreach(var member in sessionWindows.AllMembers()){
     bool exited=false;try{exited=!Windows.Same(member.Registration.Shell,Windows.Process(member.Registration.Shell.Pid).Identity);}catch(NativeError e){exited=e.Code=="NOT_FOUND";}
     if(exited && sessionWindows.Remove(member))await WindowEvent(new {type="closed",member});
    }
    Target[] known;lock(targetLock)known=targets.Values.ToArray();
    foreach(var t in known){try{Windows.Verify(t);}catch(NativeError e){
     bool removed;lock(targetLock){removed=targets.TryGetValue(t.SessionId,out var bound) && ReferenceEquals(bound,t);if(removed)targets.Remove(t.SessionId);}
     if(removed)await Event(new {type="target-lost",sessionId=t.SessionId,reason=e.Message},()=>Unbound(t.SessionId));
    }}
   }catch(OperationCanceledException){break;}catch(Exception){break;}
  }
 }
 public void Dispose(){if(stopped.IsCancellationRequested)return;stopped.Cancel();lifetime?.Dispose();tickets?.Dispose();}
}
