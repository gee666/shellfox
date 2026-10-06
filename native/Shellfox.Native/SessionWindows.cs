using System.Text.Json;

namespace Shellfox.Native;

public record BoundWindow(Target Target,string Generation);
public record MemberRegistration(Registration Registration,string? WtSession,BoundWindow Binding);
public record ReopenRequest(Launch Request,BoundWindow Previous);
public record PrepareRegistration(Launch Request,BoundWindow Binding);
public record RestoreWindows(BoundWindow[] Bindings,MemberRegistration[] Members);
public record MembershipSnapshot(BoundWindow Binding,MemberRegistration[] Members,string WindowState,string Discovery,string Reason);

// Only authenticated roots enter this registry. No folder/title scan creates a member.
public sealed class SessionWindows {
 readonly object gate=new();
 readonly Dictionary<string,BoundWindow> windows=new();
 readonly Dictionary<string,MemberRegistration> members=new();
 readonly HashSet<string> losses=new();
 public static void Validate(BoundWindow binding){Windows.Validate(binding.Target);Wire.Id(binding.Generation);}
 public BoundWindow? Current(string sessionId){lock(gate)return windows.GetValueOrDefault(sessionId);}
 public BoundWindow[] Bindings(){lock(gate)return windows.Values.ToArray();}
 public bool IsCurrent(BoundWindow binding){lock(gate)return windows.TryGetValue(binding.Target.SessionId,out var current) && current==binding;}
 public void Remember(BoundWindow binding){Validate(binding);lock(gate)windows[binding.Target.SessionId]=binding;}
 public bool MarkLost(BoundWindow binding){lock(gate)return IsCurrent(binding) && losses.Add(binding.Generation);}
 public (MemberRegistration Member,MemberRegistration? Previous) Adopt(Registration registration,string? wtSession,BoundWindow binding){
  Validate(binding);Wire.Identity(registration.Shell);
  if(registration.SessionId!=binding.Target.SessionId)throw new NativeError("VALIDATION","Registration belongs to a different session.");
  if(wtSession!=null)Wire.Id(wtSession);
  lock(gate){
   if(!IsCurrent(binding))throw new NativeError("TARGET_LOST","Window generation changed before registration.");
   var key=Tracker.Key(registration.Shell);var previous=members.GetValueOrDefault(key);
   if(previous==null && members.Count>=1000)throw new NativeError("VALIDATION","Registered member limit reached.");
   if(wtSession!=null && members.Any(m=>m.Key!=key && m.Value.WtSession==wtSession))throw new NativeError("AUTH_FAILED","This WT connection is already registered to another shell root. Register from the original interactive PowerShell, not a nested shell.");
   if(previous?.WtSession!=null && wtSession!=previous.WtSession)throw new NativeError("AUTH_FAILED","The existing shell's WT_SESSION changed. No transfer was made.");
   // A confirmed move keeps the tab identity and changes its owning session/operation.
   if(previous!=null)registration=registration with {TabId=previous.Registration.TabId};
   var member=new MemberRegistration(registration,wtSession,binding);members[key]=member;
   return (member,previous);
  }
 }
 public MemberRegistration[] Members(string sessionId){lock(gate)return members.Values.Where(m=>m.Registration.SessionId==sessionId).ToArray();}
 public MemberRegistration[] AllMembers(){lock(gate)return members.Values.ToArray();}
 public bool Allows(Watch watch){lock(gate)return watch.Tabs.All(t=>!members.TryGetValue(Tracker.Key(t.Registration.Shell),out var m) || m.Registration.SessionId==t.SessionId && m.Registration.TabId==t.TabId && m.Registration.OperationId==t.Registration.OperationId);}
 public bool Remove(MemberRegistration member){lock(gate){var key=Tracker.Key(member.Registration.Shell);return members.TryGetValue(key,out var current) && current==member && members.Remove(key);}}
 public void Restore(RestoreWindows input){
  if(input.Bindings.Length>1000 || input.Members.Length>1000)throw new NativeError("VALIDATION","Too many restored windows/members.");
  foreach(var b in input.Bindings)Validate(b);
  if(input.Bindings.Select(b=>b.Target.SessionId).Distinct().Count()!=input.Bindings.Length)throw new NativeError("VALIDATION","Duplicate restored session binding.");
  foreach(var m in input.Members){Validate(m.Binding);Wire.Identity(m.Registration.Shell);if(m.WtSession!=null)Wire.Id(m.WtSession);if(!input.Bindings.Contains(m.Binding) || m.Registration.SessionId!=m.Binding.Target.SessionId)throw new NativeError("VALIDATION","Restored member has no matching binding.");}
  if(input.Members.Select(m=>Tracker.Key(m.Registration.Shell)).Distinct().Count()!=input.Members.Length)throw new NativeError("VALIDATION","A restored shell cannot belong to two sessions.");
  var connections=input.Members.Where(m=>m.WtSession!=null).Select(m=>m.WtSession).ToArray();if(connections.Distinct().Count()!=connections.Length)throw new NativeError("VALIDATION","A restored connection cannot have multiple shell roots.");
  lock(gate){if(windows.Count>0 || members.Count>0)throw new NativeError("VALIDATION","Restore must happen before live bindings/registrations; it cannot overwrite a replacement generation.");foreach(var b in input.Bindings)windows[b.Target.SessionId]=b;foreach(var m in input.Members)members[Tracker.Key(m.Registration.Shell)]=m;}
 }
 public static MembershipSnapshot Snapshot(BoundWindow binding,MemberRegistration[] members,string state)=>new(binding,members,state,"explicit-registration","Only authenticated registered PowerShell roots are listed. Manual new/moved tabs need register-session.ps1 in the selected tab. WT_SESSION identifies a connection/pane, not its window. Missing/renamed markers and unintegrated shells are not automatically discovered.");
}
