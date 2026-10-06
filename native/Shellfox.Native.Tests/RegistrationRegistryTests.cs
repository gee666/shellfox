using Microsoft.Win32;
using Shellfox.Native;
using System.Security.AccessControl;
using System.Text.Json;
using Xunit;

namespace Shellfox.Native.Tests;

public class RegistrationRegistryTests {
 static string Scratch()=>Path.Combine(Directory.GetCurrentDirectory(),"tmp","native-unit",Guid.NewGuid().ToString());
 static Launch Launch()=>new(Guid.NewGuid().ToString(),Guid.NewGuid().ToString(),Guid.NewGuid().ToString(),@"C:\test","pwsh",Windows.Shells().GetValueOrDefault("pwsh")??@"C:\Program Files\PowerShell\7\pwsh.exe","unused","unused",null);
 [Fact] public void RestrictedTicketsExpireOnceAndRejectReplayAndForgedPid(){
  var dir=Scratch(); Directory.CreateDirectory(dir);
  using var tickets=new Tickets(dir,Environment.ProcessPath!);
  var p=tickets.Create(Launch());
  var acl=new FileInfo(p.File).GetAccessControl();
  Assert.True(acl.AreAccessRulesProtected);
  var owners=acl.GetAccessRules(true,true,typeof(System.Security.Principal.SecurityIdentifier)).Cast<FileSystemAccessRule>().Select(r=>r.IdentityReference.Value).ToArray();
  Assert.Equal(2,owners.Length);Assert.Contains(Windows.UserSid,owners);Assert.Contains("S-1-5-18",owners);
  Assert.Throws<NativeError>(()=>tickets.Authenticate(new(1,p.Ticket.TicketId,new string('B',64),Environment.ProcessId),Environment.ProcessId));
  Assert.Throws<NativeError>(()=>tickets.Authenticate(new(1,p.Ticket.TicketId,p.Ticket.Token,Environment.ProcessId),Environment.ProcessId));
  Assert.True(tickets.Expire(p.Ticket.TicketId));Assert.False(tickets.Expire(p.Ticket.TicketId));Assert.False(File.Exists(p.File));
  Assert.Throws<NativeError>(()=>tickets.Authenticate(new(1,p.Ticket.TicketId,p.Ticket.Token,Environment.ProcessId),Environment.ProcessId));
 }
 [Fact] public void BrokerDisposalDeletesPendingTicketsButDoesNotStopShells(){
  var dir=Scratch();Directory.CreateDirectory(dir);
  var tickets=new Tickets(dir,Environment.ProcessPath!);var p=tickets.Create(Launch());
  tickets.Dispose();Assert.False(File.Exists(p.File));Assert.Equal(Environment.ProcessId,Windows.Process(Environment.ProcessId).Identity.Pid);
 }
 [Fact] public void ManagedRegistryInstallUpdateRemoveUseBothVerbsAndPreserveForeignKeys(){
  var root=@"Software\ShellfoxNativeTests\"+Guid.NewGuid().ToString("N");
  string[] keys=[root+@"\Directory\Shellfox",root+@"\Background\Shellfox"];
  var dir=Scratch();Directory.CreateDirectory(dir);
  var first=Path.Combine(dir,"Shellfox Ω ' & ; % [first].exe");var next=Path.Combine(dir,"Shellfox updated.exe");
  File.WriteAllBytes(first,[]);File.WriteAllBytes(next,[]);
  try{
   var integration=new Explorer(first,keys);
   Assert.True(JsonSerializer.SerializeToElement(integration.Set(new(true,first)),Wire.Json).GetProperty("installed").GetBoolean());
   for(var i=0;i<keys.Length;i++){using var key=Registry.CurrentUser.OpenSubKey(keys[i]);using var command=key!.OpenSubKey("command");Assert.Equal(Explorer.Owner,key.GetValue("ShellfoxOwner"));Assert.Equal(Explorer.Command(first,i==1),command!.GetValue(""));}
   var updated=new Explorer(next,keys);
   updated.Set(new(true,next));
   using(var command=Registry.CurrentUser.OpenSubKey(keys[1]+@"\command"))Assert.Equal(Explorer.Command(next,true),command!.GetValue(""));
   using(var foreign=Registry.CurrentUser.CreateSubKey(keys[1]))foreign.SetValue("ShellfoxOwner","other-application");
   Assert.Throws<NativeError>(()=>updated.Set(new(true,next)));
   updated.Set(new(false,next));
   using(var removed=Registry.CurrentUser.OpenSubKey(keys[0]))Assert.Null(removed);
   using(var foreign=Registry.CurrentUser.OpenSubKey(keys[1]))Assert.Equal("other-application",foreign!.GetValue("ShellfoxOwner"));
  }finally{Registry.CurrentUser.DeleteSubKeyTree(root,false);}
 }
 [Fact] public void RenameCleanupRemovesOnlyLegacyOwnedVerbsOnStatusAndSet(){
  var root=@"Software\ShellfoxNativeTests\"+Guid.NewGuid().ToString("N");
  string[] keys=[root+@"\Folder",root+@"\Background"],legacy=[root+@"\OldFolder",root+@"\OldBackground"];
  var dir=Scratch();Directory.CreateDirectory(dir);var exe=Path.Combine(dir,"Shellfox.exe");File.WriteAllBytes(exe,[]);
  try{
   using(var owned=Registry.CurrentUser.CreateSubKey(legacy[0]))owned.SetValue("PiManagerOwner",Explorer.LegacyOwner);
   using(var foreign=Registry.CurrentUser.CreateSubKey(legacy[1]))foreign.SetValue("PiManagerOwner","foreign");
   var integration=new Explorer(exe,keys,legacy);integration.Status();
   using(var removed=Registry.CurrentUser.OpenSubKey(legacy[0]))Assert.Null(removed);
   using(var foreign=Registry.CurrentUser.OpenSubKey(legacy[1]))Assert.Equal("foreign",foreign!.GetValue("PiManagerOwner"));
   using(var owned=Registry.CurrentUser.CreateSubKey(legacy[0]))owned.SetValue("PiManagerOwner",Explorer.LegacyOwner);
   integration.Set(new(true,exe));
   using(var removed=Registry.CurrentUser.OpenSubKey(legacy[0]))Assert.Null(removed);
   using(var foreign=Registry.CurrentUser.OpenSubKey(legacy[1]))Assert.Equal("foreign",foreign!.GetValue("PiManagerOwner"));
  }finally{Registry.CurrentUser.DeleteSubKeyTree(root,false);}
 }
 [Fact] public void UnpackagedIntegrationCannotMutateRegistry(){var integration=new Explorer(null);Assert.Throws<NativeError>(()=>integration.Set(new(true,Environment.ProcessPath!)));}
}
