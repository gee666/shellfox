using Shellfox.Native;
using Xunit;

namespace Shellfox.Native.Tests;

public class WindowLaunchPolicyTests {
 static Launch Request()=>new(Guid.NewGuid().ToString(),Guid.NewGuid().ToString(),Guid.NewGuid().ToString(),@"C:\fixture","pwsh",@"C:\Program Files\PowerShell\7\pwsh.exe","unused","unused",null);
 static string Scratch()=>Path.Combine(Directory.GetCurrentDirectory(),"tmp","routing-policy",Guid.NewGuid().ToString());
 [Fact] public void ExplicitExistingTargetIsRejectedBeforeReservation(){
  var request=Request();var target=new Target("windows-terminal","shellfox-"+request.SessionId,"123",new(1,"100"),request.SessionId,"SHELLFOX:"+request.SessionId+":","native-title");
  var path=Scratch();var policy=new WindowLaunchPolicy(path);
  var error=Assert.Throws<NativeError>(()=>policy.Reserve(request with {ExistingTarget=target}));
  Assert.Equal("UNSUPPORTED",error.Code);Assert.Empty(Directory.GetFiles(Path.Combine(path,"native-window-intents")));
 }
 [Fact] public void NullTargetCannotBypassPriorReservationAfterRestart(){
  var request=Request();var path=Scratch();new WindowLaunchPolicy(path).Reserve(request);
  var restarted=new WindowLaunchPolicy(path);
  var error=Assert.Throws<NativeError>(()=>restarted.Reserve(request with {OperationId=Guid.NewGuid().ToString(),TabId=Guid.NewGuid().ToString()}));
  Assert.Equal("UNSUPPORTED",error.Code);Assert.Single(Directory.GetFiles(Path.Combine(path,"native-window-intents")));
 }
 [Fact] public void FreshSessionsRemainIndependentEvenWithSameCwd(){
  var policy=new WindowLaunchPolicy(Scratch());policy.Reserve(Request());policy.Reserve(Request());
 }
 [Fact] public async Task ConcurrentSameSessionReservationsHaveOnlyOneWinner(){
  var request=Request();var path=Scratch();var policies=new[]{new WindowLaunchPolicy(path),new WindowLaunchPolicy(path)};
  var outcomes=await Task.WhenAll(policies.Select(p=>Task.Run(()=>{try{p.Reserve(request);return "reserved";}catch(NativeError e){return e.Code;}})));
  Assert.Single(outcomes,o=>o=="reserved");Assert.Single(outcomes,o=>o=="UNSUPPORTED");
 }
 [Fact] public void InitialSelectorNeverUsesNamesIdsLastOrImplicitWindowingBehavior(){
  Assert.Equal(new[]{"--window","new"},WindowLaunchPolicy.SelectorArguments());
 }
}
