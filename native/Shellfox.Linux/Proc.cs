using System.Runtime.InteropServices;
using System.Numerics;
using System.Globalization;
namespace Shellfox.Native;

public record LinuxProcess(Identity Identity,int Parent,string Executable,string[]? Args);
public static class Proc {
 [DllImport("libc")] public static extern uint getuid();
 [DllImport("libc")] public static extern uint geteuid();
 [DllImport("libc")] static extern long sysconf(int name);
 public static long ClockTicks=>sysconf(2); // _SC_CLK_TCK on Linux
 public static long NowTicks=>(long)(double.Parse(File.ReadAllText("/proc/uptime").Split(' ')[0],CultureInfo.InvariantCulture)*ClockTicks);
 static string Boot=>File.ReadAllText("/proc/sys/kernel/random/boot_id").Trim().Replace("-","");
 // Boot identity + exact kernel start ticks. Never rounded DateTime or a recycled PID alone.
 public static string Birth(string boot,long ticks)=>(BigInteger.Parse("0"+boot,NumberStyles.HexNumber)*BigInteger.Pow(10,20)+ticks).ToString(CultureInfo.InvariantCulture);
 public static (int Parent,long Ticks,char State) Stat(string text){
  var end=text.LastIndexOf(')');if(end<0)throw new NativeError("MONITOR_UNAVAILABLE","Malformed proc stat.");
  var fields=text[(end+2)..].Split(' ',StringSplitOptions.RemoveEmptyEntries);
  return(int.Parse(fields[1]),long.Parse(fields[19]),fields[0][0]);
 }
 public static LinuxProcess Read(int pid){
  try{
   var dir=$"/proc/{pid}";var before=Stat(File.ReadAllText(dir+"/stat"));
   if(before.State is 'Z' or 'X')throw new NativeError("NOT_FOUND","Process exited.");
   var uidLine=File.ReadAllLines(dir+"/status").First(l=>l.StartsWith("Uid:",StringComparison.Ordinal)).Split((char[]?)null,StringSplitOptions.RemoveEmptyEntries);
   if(uint.Parse(uidLine[1])!=getuid() || uint.Parse(uidLine[2])!=getuid())throw new NativeError("MONITOR_UNAVAILABLE","Other-user or elevated processes are outside tracking.");
   var exe=new FileInfo(dir+"/exe").ResolveLinkTarget(true)?.FullName??throw new NativeError("MONITOR_UNAVAILABLE","Executable inaccessible.");
   string[]? args=null;try{args=System.Text.Encoding.UTF8.GetString(File.ReadAllBytes(dir+"/cmdline")).Split('\0',StringSplitOptions.RemoveEmptyEntries);}catch(IOException){}catch(UnauthorizedAccessException){}
   var after=Stat(File.ReadAllText(dir+"/stat"));if(after.Ticks!=before.Ticks || after.Parent!=before.Parent)throw new NativeError("MONITOR_UNAVAILABLE","Process changed during snapshot.");
   return new(new(pid,Birth(Boot,before.Ticks)),before.Parent,exe,args);
  }catch(FileNotFoundException){throw new NativeError("NOT_FOUND","Process absent.");}catch(DirectoryNotFoundException){throw new NativeError("NOT_FOUND","Process absent.");}catch(UnauthorizedAccessException){throw new NativeError("MONITOR_UNAVAILABLE","Process inaccessible.");}
 }
 public static string Canonical(string path)=>new FileInfo(path).ResolveLinkTarget(true)?.FullName??Path.GetFullPath(path);
 public static string DirectoryPath(string path){
  if(!Path.IsPathFullyQualified(path)||!path.StartsWith('/')||path.StartsWith("//")||path.Contains('\0')||!Directory.Exists(path))throw new NativeError("VALIDATION","Choose an accessible absolute local folder.");
  // Do not silently launch through symlinks into a different mount/namespace.
  for(var d=new DirectoryInfo(path);d!=null;d=d.Parent)if(d.LinkTarget!=null)throw new NativeError("VALIDATION","Use the real local target folder, not a symlink.");
  Directory.EnumerateFileSystemEntries(path).Take(1).ToArray();return path;
 }
 public static bool Same(Identity a,Identity b)=>a==b;
 public static string Key(Identity p)=>p.Pid+":"+p.StartTime;
 public static string? Script(string[] args){
  if(args.Length<2)return null;
  for(var i=1;i<args.Length;i++){
   var a=args[i];if(a is "-e" or "--eval" or "-p" or "--print" || a.StartsWith("--eval=")||a.StartsWith("--print="))return null;
   if(a is "-r" or "--require" or "--import" or "--loader"){i++;continue;}
   if(a=="--")return i+1<args.Length?args[i+1]:null;
   if(a.StartsWith('-')){if(a.Contains('=') || a is "--no-warnings" or "--enable-source-maps")continue;throw new NativeError("MONITOR_UNAVAILABLE","Unsupported interpreter option form.");}
   return a;
  }return null;
 }
 public static bool Match(Rule r,LinuxProcess p){
  var executable=r.ExecutablePaths.Length>0?r.ExecutablePaths.Contains(p.Executable,StringComparer.Ordinal):r.ExecutableBasenames.Contains(Path.GetFileName(p.Executable),StringComparer.Ordinal);
  if(!executable)return false;if(r.ScriptPathSuffixes.Length==0)return true;
  if(p.Args==null)throw new NativeError("MONITOR_UNAVAILABLE","Relevant command line unavailable.");
  if(Path.GetFileName(p.Executable) is not "node" and not "nodejs")throw new NativeError("MONITOR_UNAVAILABLE","Script rules currently support Node launchers only.");
  var script=Script(p.Args);return script!=null&&r.ScriptPathSuffixes.Any(s=>script==s||script.EndsWith("/"+s,StringComparison.Ordinal));
 }
}
