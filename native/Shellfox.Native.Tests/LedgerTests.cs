using Shellfox.Native;
using Xunit;

namespace Shellfox.Native.Tests;

public class LedgerTests {
 static Registration Registration(string tab,string op,int pid)=>new(Guid.NewGuid().ToString(),tab,op,new(pid,"100"),@"C:\Program Files\PowerShell\7\pwsh.exe",@"C:\test",Wire.Now());
 static Watch Watch(Registration r)=>new([new(r.SessionId,r.TabId,r)],[]);
 [Fact] public void OldPollCannotDescribeNewRegistrationOfSameTab(){
  var ledger=new RegistrationLedger();var tab=Guid.NewGuid().ToString();var first=Registration(tab,Guid.NewGuid().ToString(),1);var next=Registration(tab,Guid.NewGuid().ToString(),2);
  ledger.Begin(tab,first.OperationId);Assert.True(ledger.Register(first));Assert.True(ledger.Allows(Watch(first)));
  ledger.Begin(tab,next.OperationId);Assert.True(ledger.Register(next));Assert.False(ledger.Allows(Watch(first)));Assert.True(ledger.Allows(Watch(next)));
 }
 [Fact] public void LateObsoleteRegistrationCannotReplaceNewestIdentityOrTarget(){
  var ledger=new RegistrationLedger();var tab=Guid.NewGuid().ToString();var old=Registration(tab,Guid.NewGuid().ToString(),1);var current=Registration(tab,Guid.NewGuid().ToString(),2);
  ledger.Begin(tab,old.OperationId);ledger.Begin(tab,current.OperationId);ledger.Register(current);
  var remembered=false;Assert.False(ledger.Register(old,()=>remembered=true));Assert.False(remembered);Assert.True(ledger.Allows(Watch(current)));
 }
 [Fact] public void PersistedRootsCanBeObservedWithoutReregistering(){
  var ledger=new RegistrationLedger();Assert.True(ledger.Allows(Watch(Registration(Guid.NewGuid().ToString(),Guid.NewGuid().ToString(),1))));
 }
}
