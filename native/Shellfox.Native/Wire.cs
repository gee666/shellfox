using System.Text.Json;
using System.Text.Json.Serialization;

namespace Shellfox.Native;

public record Identity(int Pid, string StartTime);
public record Registration(string SessionId, string TabId, string OperationId, Identity Shell, string ShellExecutable, string Cwd, string RegisteredAt);
public record Target(string Kind, string WindowName, string Hwnd, Identity Owner, string SessionId, string MarkerPrefix, string Verification);
public record Launch(string SessionId, string TabId, string OperationId, string Cwd, string ShellId, string ShellExecutable, string WindowName, string TitleMarker, Target? ExistingTarget);
public record Init(string UserDataDir, string HelperDir, string ShellScriptDir, string? PackagedExecutable);
public record Rule(string Id, string Label, bool Enabled, string[] ExecutableBasenames, string[] ExecutablePaths, string[] ScriptPathSuffixes);
public record WatchTab(string SessionId, string TabId, Registration Registration);
public record Watch(WatchTab[] Tabs, Rule[] Rules);
public record Observation(string SessionId, string TabId, string ObservedAt, string Root, string Health, int Agents, string? Reason);
public record ExplorerRequest(bool Installed, string ExecutablePath);
public record RegisterRequest(int Version, string TicketId, string Token, int ShellPid, string? WtSession=null);
public record Frame(int Version, string Id, string Kind, string Method, JsonElement Payload);
public sealed class NativeError(string code, string message) : Exception(message) { public string Code { get; } = code; }
public static class Wire {
 public const int MaxFrame = 1024 * 1024;
 public static readonly JsonSerializerOptions Json = new() {
  PropertyNamingPolicy=JsonNamingPolicy.CamelCase,
  PropertyNameCaseInsensitive=false,
  UnmappedMemberHandling=JsonUnmappedMemberHandling.Disallow
 };
 public static object Ok(object value) => new { ok=true, value };
 public static object Fail(string code, string message, bool retryable=false) => new { ok=false, error=new { code, message=message[..Math.Min(message.Length,1000)], retryable } };
 public static T Read<T>(JsonElement payload) => payload.Deserialize<T>(Json) ?? throw new NativeError("VALIDATION","Missing payload.");
 public static void Id(string value) { if(!Guid.TryParseExact(value,"D",out _)) throw new NativeError("VALIDATION","Expected UUID in D format."); }
 public static void Identity(Identity value) { if(value.Pid<=0 || !ulong.TryParse(value.StartTime,out var ticks) || ticks==0) throw new NativeError("VALIDATION","Invalid process identity."); }
 public static string Now() => DateTime.UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffffffZ",System.Globalization.CultureInfo.InvariantCulture);
 // Byte-bounded framing, unlike ReadLine which can allocate an arbitrary attacker-controlled string.
 public static async Task<string?> Line(Stream stream, CancellationToken ct) {
  using var bytes=new MemoryStream(); var one=new byte[1];
  while(await stream.ReadAsync(one,ct)>0){
   if(one[0]==10) return new System.Text.UTF8Encoding(false,true).GetString(bytes.ToArray()).TrimEnd('\r');
   if(bytes.Length>=MaxFrame) throw new NativeError("VALIDATION","Native frame exceeds 1 MiB.");
   bytes.WriteByte(one[0]);
  }
  if(bytes.Length!=0) throw new NativeError("VALIDATION","Truncated native frame.");
  return null;
 }
}
