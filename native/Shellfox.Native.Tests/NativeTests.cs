using System.Text;
using System.Text.Json;
using Shellfox.Native;
using Xunit;

namespace Shellfox.Native.Tests;

public class NativeTests {
 const string Session="00000000-0000-4000-8000-000000000001";
 const string Tab="00000000-0000-4000-8000-000000000002";
 static Identity Id(int pid,int time)=>new(pid,time.ToString());
 static WatchTab Root(int pid,int time,string tab=Tab)=>new(Session,tab,new(Session,tab,Guid.NewGuid().ToString(),Id(pid,time),@"C:\Program Files\PowerShell\7\pwsh.exe",@"C:\test",Wire.Now()));
 static ProcessRow Row(int pid,int parent,int time,string? command=null)=>new(Id(pid,time),pid,parent,@"C:\tools\node.exe",command,true);
 static Rule Rule(params string[] suffixes)=>new(Guid.NewGuid().ToString(),"test",true,["node.exe"],[],suffixes);
 [Fact] public void WmiCreationIsCrossCheckedBeforeUsingPidFields(){
  var p=Windows.Process(Environment.ProcessId);
  using var wmi=new System.Management.ManagementObject($"Win32_Process.Handle='{Environment.ProcessId}'");wmi.Get();
  var created=(string)wmi["CreationDate"];
  Assert.True(Tracker.WmiCreationMatches(p.Identity,created));
  Assert.False(Tracker.WmiCreationMatches(p.Identity with {StartTime=(ulong.Parse(p.Identity.StartTime)+100).ToString()},created));
  Assert.False(Tracker.WmiCreationMatches(p.Identity,"not-a-date"));
 }
 [Fact] public void ExitedProcessWithRetainedHandleIsNotAlive(){
  var shell=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows),"System32","WindowsPowerShell","v1.0","powershell.exe");
  var start=new System.Diagnostics.ProcessStartInfo(shell){UseShellExecute=false,CreateNoWindow=true};
  foreach(var arg in new[]{"-NoProfile","-NonInteractive","-Command","exit 0"})start.ArgumentList.Add(arg);
  using var child=System.Diagnostics.Process.Start(start)!;var pid=child.Id;
  Assert.True(child.WaitForExit(5000));
  var error=Assert.Throws<NativeError>(()=>Windows.Process(pid));Assert.Equal("NOT_FOUND",error.Code);
 }
 [Fact] public void ExactIdentityDoesNotRoundFileTime(){ Assert.False(Windows.Same(new(4,"134355986344672888"),new(4,"134355986344672889"))); }
 [Fact] public void OwnershipRetainedAfterIntermediateExit(){
  var tracker=new Tracker();var watch=new Watch([Root(1,100)],[]);
  tracker.ResolveOwners(watch,[Row(1,0,100),Row(2,1,110),Row(3,2,120)]);
  var owners=tracker.ResolveOwners(watch,[Row(1,0,100),Row(3,2,120)]);
  Assert.Equal(Tab,owners[Tracker.Key(Id(3,120))]);
 }
 [Fact] public void RecycledPidCannotAcquireOldDescendants(){
  var tracker=new Tracker();var watch=new Watch([Root(1,100)],[]);
  tracker.ResolveOwners(watch,[Row(1,0,100),Row(2,1,110)]);
  var owners=tracker.ResolveOwners(watch,[Row(1,0,100),Row(2,99,200),Row(3,2,150)]);
  Assert.False(owners.ContainsKey(Tracker.Key(Id(2,200))));
  Assert.False(owners.ContainsKey(Tracker.Key(Id(3,150))));
 }
 [Fact] public void NestedRegisteredRootsOwnNearestChildren(){
  var nested=Guid.NewGuid().ToString();var tracker=new Tracker();
  var owners=tracker.ResolveOwners(new([Root(1,100),Root(2,110,nested)],[]),[Row(1,0,100),Row(2,1,110),Row(3,2,120),Row(4,1,120)]);
  Assert.Equal(nested,owners[Tracker.Key(Id(3,120))]);Assert.Equal(Tab,owners[Tracker.Key(Id(4,120))]);
 }
 [Fact] public void ChangedShellIdentityDoesNotInheritOldAgentsEvenForSameTab(){
  var tracker=new Tracker();
  tracker.ResolveOwners(new([Root(1,100)],[]),[Row(1,0,100),Row(2,1,110),Row(3,2,120)]);
  var current=tracker.ResolveOwners(new([Root(1,200)],[]),[Row(1,0,200),Row(3,2,120)]);
  Assert.False(current.ContainsKey(Tracker.Key(Id(3,120))));
 }
 [Fact] public void PreviousParentGenerationWinsOverBackwardClockOrder(){
  var other=Guid.NewGuid().ToString();var tracker=new Tracker();var watch=new Watch([Root(1,100),Root(9,100,other)],[]);
  tracker.ResolveOwners(watch,[Row(1,0,100),Row(9,0,100),Row(2,1,110),Row(3,2,120)]);
  var current=tracker.ResolveOwners(watch,[Row(1,0,100),Row(9,0,100),Row(2,9,115),Row(3,2,120)]);
  Assert.Equal(Tab,current[Tracker.Key(Id(3,120))]);Assert.Equal(other,current[Tracker.Key(Id(2,115))]);
 }
 [Fact] public void DeadIdentitiesRemovedAndUnrelatedProcessesUnowned(){
  var tracker=new Tracker();var w=new Watch([Root(1,100)],[]);
  tracker.ResolveOwners(w,[Row(1,0,100),Row(2,1,110)]);
  var owners=tracker.ResolveOwners(w,[Row(1,0,100),Row(9,8,120)]);
  Assert.False(owners.ContainsKey(Tracker.Key(Id(2,110))));Assert.False(owners.ContainsKey(Tracker.Key(Id(9,120))));
 }
 [Theory]
 [InlineData("node.exe \"C:\\x y\\@mariozechner\\pi-coding-agent\\dist\\cli.js\"",true)]
 [InlineData("node.exe C:\\other.js pi",false)]
 [InlineData("node.exe --eval \"pi-coding-agent/dist/cli.js\"",false)]
 [InlineData("node.exe --eval=console.log(1) C:\\pi-coding-agent\\dist\\cli.js",false)]
 [InlineData("node.exe --print=1 C:\\pi-coding-agent\\dist\\cli.js",false)]
 [InlineData("node.exe -econsole.log(1) C:\\pi-coding-agent\\dist\\cli.js",false)]
 [InlineData("node.exe C:\\notpi-coding-agent\\dist\\cli.js",false)]
 [InlineData("node.exe --require C:\\pi-coding-agent\\dist\\cli.js C:\\other.js",false)]
 [InlineData("node.exe -- C:\\pi-coding-agent\\dist\\cli.js",true)]
 [InlineData("node.exe --title C:\\pi-coding-agent\\dist\\cli.js C:\\other.js",false)]
 [InlineData("node.exe --unknown-option C:\\pi-coding-agent\\dist\\cli.js",false)]
 public void ScriptMatchingIsNotArbitraryArgumentSearch(string command,bool expected){ Assert.Equal(expected,Tracker.Matches(Rule("pi-coding-agent/dist/cli.js"),Row(1,0,100,command))); }
 [Fact] public void ContractUuidsMayUseUppercaseHex(){ Wire.Id(Guid.NewGuid().ToString().ToUpperInvariant()); Assert.Throws<NativeError>(()=>Wire.Id("not-a-uuid")); }
 [Theory]
 [InlineData("pwsh.exe -NoLogo -File \"C:\\agent\\cli.ps1\"",true)]
 [InlineData("powershell.exe -ExecutionPolicy Bypass -File C:\\agent\\cli.ps1",true)]
 [InlineData("pwsh.exe -Command Write-Output -File C:\\agent\\cli.ps1",false)]
 [InlineData("pwsh.exe -WorkingDirectory -File C:\\agent\\cli.ps1",false)]
 public void PowerShellScriptRulesUseFileSlotOnly(string command,bool expected){
  var exe=Windows.Args(command)[0];
  var rule=Rule("agent/cli.ps1") with {ExecutableBasenames=[exe]};
  var process=Row(1,0,100,command) with {Executable=@"C:\tools\"+exe};
  Assert.Equal(expected,Tracker.Matches(rule,process));
 }
 [Fact] public void UnsupportedRequiredScriptFormProducesUnknownNotWaiting(){
  var p=Windows.Process(Environment.ProcessId);var identity=p.Identity;
  var root=Root(identity.Pid,100) with {Registration=new(Session,Tab,Guid.NewGuid().ToString(),identity,p.Executable,@"C:\test",Wire.Now())};
  var child=new ProcessRow(new(5555,(ulong.Parse(identity.StartTime)+100).ToString()),5555,identity.Pid,@"C:\tools\node.exe","node.exe --unknown-option agent/cli.js",true);
  var snapshot=new List<ProcessRow>{new(identity,identity.Pid,0,p.Executable,"testhost.exe",true),child};
  var observed=new Tracker().Observe(new([root],[Rule("agent/cli.js")]),snapshot).Single();
  Assert.Equal("unknown",observed.Health);Assert.Equal(0,observed.Agents);
  if(!p.Elevated){Assert.Equal("alive",observed.Root);Assert.Contains("unsupported interpreter or option form",observed.Reason);}
 }
 [Fact] public void PnpmCmdNodeWithoutExeMatchesBundledPiSlotOnly(){
  var rule=Rule("@earendil-works/pi-coding-agent/dist/bundle/cli.js");
  Assert.True(Tracker.Matches(rule,Row(1,0,100,"node C:\\pnpm\\node_modules\\@earendil-works\\pi-coding-agent\\dist\\bundle\\cli.js")));
  Assert.False(Tracker.Matches(rule,Row(1,0,100,"node C:\\other.js @earendil-works/pi-coding-agent/dist/bundle/cli.js")));
 }
 [Fact] public void NodeAloneDoesNotMatchScriptRule(){ Assert.False(Tracker.Matches(Rule("agent/cli.js"),Row(1,0,100,"node.exe"))); }
 [Fact] public void ExecutablePathsTakePrecedence(){ var r=Rule() with {ExecutablePaths=[@"C:\real\node.exe"]};Assert.False(Tracker.Matches(r,Row(1,0,100,"node.exe"))); }
 [Fact] public void NativeArgumentTokenizationPreservesQuotedSpaceAndRoot(){
  var args=Windows.Args("\"C:\\Program Files\\Shellfox.exe\" --new-session --cwd \"C:\\.\"");
  Assert.Equal(@"C:\Program Files\Shellfox.exe",args[0]);Assert.Equal(@"C:\.",args[3]);
 }
 [Theory][InlineData(false,"%1")][InlineData(true,"%V")]
 public void ExplorerPlaceholderHasDotAfterSlash(bool background,string placeholder){ var command=Explorer.Command(@"C:\Program Files\Shellfox.exe",background);Assert.EndsWith("\""+placeholder+"\\.\"",command); }
 [Theory][InlineData(@"C:\space Ω ' & ; % [x]")][InlineData(@"C:\")]
 public void SubstitutedExplorerPathIsOneArgument(string folder){ var command=Explorer.Command(@"C:\Program Files\Shellfox.exe",false).Replace("%1",folder);var args=Windows.Args(command);Assert.Equal(4,args.Length);Assert.Equal(folder+"\\.",args[3]); }
 [Fact] public void InvalidExplorerExecutableRejected(){ Assert.Throws<NativeError>(()=>Explorer.Command("bad\".exe",false)); }
 [Fact] public void ExpiredAndWrongTokensFail(){ var token=new string('A',64);var now=DateTimeOffset.UtcNow;Assert.True(Tickets.Auth(token,token,now.AddSeconds(1),now));Assert.False(Tickets.Auth(token,token,now,now));Assert.False(Tickets.Auth(token,new string('B',64),now.AddSeconds(1),now));Assert.False(Tickets.Auth(token,"not-hex",now.AddSeconds(1),now)); }
 [Fact] public async Task OversizedAndTruncatedFramesRejected(){
  await Assert.ThrowsAsync<NativeError>(()=>Wire.Line(new MemoryStream(new byte[Wire.MaxFrame+1]),CancellationToken.None));
  await Assert.ThrowsAsync<NativeError>(()=>Wire.Line(new MemoryStream(Encoding.UTF8.GetBytes("{}")),CancellationToken.None));
  Assert.Equal("{}",await Wire.Line(new MemoryStream(Encoding.UTF8.GetBytes("{}\n")),CancellationToken.None));
 }
 [Fact] public void NativeDtoRejectsUnexpectedFields(){using var doc=JsonDocument.Parse("{\"installed\":true,\"executablePath\":\"C:\\\\app.exe\",\"command\":\"evil\"}");Assert.Throws<JsonException>(()=>Wire.Read<ExplorerRequest>(doc.RootElement));}
 [Fact] public void MarkersMustBeExactManagedTitles(){ Assert.True(Windows.Marker("SHELLFOX:"+Session+":"+Tab,"SHELLFOX:"+Session+":"));Assert.False(Windows.Marker("unmanaged SHELLFOX:"+Session+":"+Tab,"SHELLFOX:"+Session+":"));Assert.False(Windows.Marker("SHELLFOX:"+Session+":"+Tab+" renamed","SHELLFOX:"+Session+":")); }
 [Fact] public void StaleTargetFailsWithoutLaunchingAnything(){ var t=new Target("windows-terminal","shellfox-"+Session,"1",Id(1,100),Session,"SHELLFOX:"+Session+":","native-title"); Assert.Throws<NativeError>(()=>Windows.Verify(t)); }
 [Fact] public void MissingAndArbitraryShellsRejected(){ Assert.Throws<NativeError>(()=>Windows.Shell("pwsh",@"C:\fake\pwsh.exe"));Assert.Throws<NativeError>(()=>Windows.Shell("bash",@"C:\bash.exe")); }
 [Fact] public void InvalidRulesAndRemoteFoldersRejected(){ Assert.Throws<NativeError>(()=>Tracker.Validate(new([], [Rule() with {ExecutableBasenames=[]}])));Assert.Throws<NativeError>(()=>Windows.LocalDirectory(@"\\server\share")); }
}
