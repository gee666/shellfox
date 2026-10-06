namespace Shellfox.Native;

// Keeps stale polls/registrations from an earlier launch of the same tab out of a new watch.
public sealed class RegistrationLedger {
 readonly object gate=new();
 readonly Dictionary<string,string> operations=new();
 readonly Dictionary<string,string> roots=new();
 public void Begin(string tabId,string operationId){lock(gate)operations[tabId]=operationId;}
 public bool Register(Registration registration,Action? rememberTarget=null){
  lock(gate){
   if(operations.GetValueOrDefault(registration.TabId)!=registration.OperationId)return false;
   roots[registration.TabId]=Tracker.Key(registration.Shell);
   rememberTarget?.Invoke();
   return true;
  }
 }
 public bool Allows(Watch watch){
  lock(gate)return watch.Tabs.All(t=>!roots.TryGetValue(t.TabId,out var identity) || identity==Tracker.Key(t.Registration.Shell));
 }
}
