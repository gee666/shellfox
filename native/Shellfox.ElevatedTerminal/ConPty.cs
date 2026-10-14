using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Threading.Channels;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

internal sealed class ConPty : IDisposable
{
    private IntPtr console;
    private readonly SafeFileHandle job;
    private readonly SafeFileHandle process;
    private readonly FileStream input;
    internal FileStream Output { get; }
    internal int Pid { get; }
    internal long Birth { get; }
    internal uint ActiveProcesses { get { Check(QueryInformationJobObject(job, 1, out var info, (uint)Marshal.SizeOf<JobAccounting>(), IntPtr.Zero)); return info.ActiveProcesses; } }
    internal int ExitCode { get { Check(GetExitCodeProcess(process, out var code)); return unchecked((int)code); } }
    private int inputBytes;
    private long lastInput;
    private readonly Channel<(byte[] Data, long Id, Func<object, Task> Send)> writes = Channel.CreateBounded<(byte[], long, Func<object, Task>)>(new BoundedChannelOptions(128) { SingleReader = true, SingleWriter = true, FullMode = BoundedChannelFullMode.Wait });
    private readonly object closeLock = new();
    private Task? closing;

    internal static void ValidateLaunch(JsonElement value)
    {
        Wire.Fields(value, "type", "cwd", "env", "cols", "rows");
        if (Wire.Type(value) != "launch") throw new InvalidDataException("Expected launch.");
        var cwd = value.GetProperty("cwd").GetString() ?? "";
        if (cwd.Length is < 1 or > 32767 || cwd.Any(c => c < 32) || !Path.IsPathFullyQualified(cwd)) throw new InvalidDataException("Invalid working directory.");
        Dimensions(value);
        var env = value.GetProperty("env");
        if (env.ValueKind != JsonValueKind.Object) throw new InvalidDataException("Invalid environment.");
        var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var units = 1;
        foreach (var item in env.EnumerateObject())
        {
            var text = item.Value.GetString() ?? throw new InvalidDataException("Invalid environment value.");
            if (!names.Add(item.Name) || item.Name.Length is < 1 or > 32767 || item.Name.Contains('=') || item.Name.Contains('\0') || text.Contains('\0')) throw new InvalidDataException("Invalid environment entry.");
            units += item.Name.Length + text.Length + 2;
        }
        if (names.Count > 1024 || units > 32767) throw new InvalidDataException("Oversized environment.");
    }
    internal static bool SafeEnvironmentName(string name) =>
        !new[] { "COR_", "CORECLR_", "COMPLUS_", "DOTNET_" }.Any(prefix => name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase));
    internal static string BuildEnvironment(JsonElement env) =>
        string.Join("\0", env.EnumerateObject().Where(p => SafeEnvironmentName(p.Name)).OrderBy(p => p.Name, StringComparer.OrdinalIgnoreCase).Select(p => p.Name + "=" + p.Value.GetString())) + "\0\0";
    private static Coord Dimensions(JsonElement value)
    {
        int cols = value.GetProperty("cols").GetInt32(), rows = value.GetProperty("rows").GetInt32();
        if (cols is < 2 or > 500 || rows is < 2 or > 500) throw new InvalidDataException("Invalid dimensions.");
        return new Coord { X = (short)cols, Y = (short)rows };
    }
    internal ConPty(JsonElement launch)
    {
        ValidateLaunch(launch);
        SafeFileHandle? inRead = null, outWrite = null, inWrite = null, outRead = null, thread = null, createdProcess = null, createdJob = null;
        IntPtr attributes = IntPtr.Zero, environment = IntPtr.Zero;
        try
        {
            Check(CreatePipe(out inRead, out inWrite, IntPtr.Zero, 0));
            Check(CreatePipe(out outRead, out outWrite, IntPtr.Zero, 0));
            Marshal.ThrowExceptionForHR(CreatePseudoConsole(Dimensions(launch), inRead, outWrite, 0, out console));
            // Handles are non-inheritable. The child must join this job BEFORE
            // its first instruction. No breakaway permission is set on the job.
            createdJob = CreateJobObject(IntPtr.Zero, null);
            Check(!createdJob.IsInvalid);
            var limits = new JobLimits(); limits.Basic.LimitFlags = 0x2000; // KILL_ON_JOB_CLOSE
            Check(SetInformationJobObject(createdJob, 9, ref limits, (uint)Marshal.SizeOf<JobLimits>()));
            nuint size = 0;
            InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
            attributes = Marshal.AllocHGlobal((int)size);
            Check(InitializeProcThreadAttributeList(attributes, 1, 0, ref size));
            Check(UpdateProcThreadAttribute(attributes, 0, (IntPtr)0x20016, console, (nuint)IntPtr.Size, IntPtr.Zero, IntPtr.Zero));
            // Null standard handles with STARTF_USESTDHANDLES keep redirected
            // launcher stdio out of the shell. ConPTY supplies console handles;
            // otherwise PowerShell can read the app pipe and exit on its EOF.
            var start = new StartupInfoEx { Info = new StartupInfo { Size = Marshal.SizeOf<StartupInfoEx>(), Flags = 0x100 }, Attributes = attributes };
            // Never accept an executable or shell script in the IPC launch frame.
            var executable = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "WindowsPowerShell", "v1.0", "powershell.exe");
            var command = new StringBuilder($"\"{executable}\" -NoLogo");
            // Final privileged boundary, after inherited and session variables
            // were merged. Never let either source inject a CLR profiler, startup
            // hook or runtime override before the first interactive command.
            environment = Marshal.StringToHGlobalUni(BuildEnvironment(launch.GetProperty("env")));
            Check(CreateProcess(executable, command, IntPtr.Zero, IntPtr.Zero, false, 0x00080004 | 0x00000400, environment, launch.GetProperty("cwd").GetString()!, ref start, out var info));
            createdProcess = new SafeFileHandle(info.Process, true); thread = new SafeFileHandle(info.Thread, true);
            Check(AssignProcessToJobObject(createdJob, createdProcess));
            Check(GetProcessTimes(createdProcess, out var birth, out _, out _, out _));
            Birth = birth; Pid = (int)info.Pid;
            Check(ResumeThread(thread) != uint.MaxValue);
            process = createdProcess; createdProcess = null;
            job = createdJob; createdJob = null;
            input = new FileStream(inWrite, FileAccess.Write, 4096, false); inWrite = null;
            Output = new FileStream(outRead, FileAccess.Read, 4096, false); outRead = null;
            _ = Task.Run(async () =>
            {
                try
                {
                    await foreach (var write in writes.Reader.ReadAllAsync())
                    {
                        await input.WriteAsync(write.Data);
                        await input.FlushAsync();
                        Interlocked.Add(ref inputBytes, -write.Data.Length);
                        // Ack means the bytes reached the ConPTY pipe, not that a
                        // shell command executed. Pending application bytes stay
                        // bounded until this exact acknowledgement.
                        await write.Send(new { type = "ack", id = write.Id });
                    }
                }
                catch { await Close(); }
            });
        }
        catch
        {
            if (createdJob != null) TerminateJobObject(createdJob, 1);
            if (createdProcess != null) TerminateProcess(createdProcess, 1);
            // Setup failure has no output consumer yet. Never let a native
            // ConPTY flush block process death and kill-on-close job cleanup.
            if (console != IntPtr.Zero)
            {
                var failedConsole = console; console = IntPtr.Zero;
                if (!Task.Run(() => ClosePseudoConsole(failedConsole)).Wait(TimeSpan.FromSeconds(5))) Environment.Exit(1);
            }
            throw;
        }
        finally
        {
            if (attributes != IntPtr.Zero) { DeleteProcThreadAttributeList(attributes); Marshal.FreeHGlobal(attributes); }
            if (environment != IntPtr.Zero) Marshal.FreeHGlobal(environment);
            inRead?.Dispose(); outWrite?.Dispose(); inWrite?.Dispose(); outRead?.Dispose(); thread?.Dispose(); createdProcess?.Dispose(); createdJob?.Dispose();
        }
    }
    internal void QueueInput(JsonElement value, Func<object, Task> send)
    {
        Wire.Fields(value, "type", "id", "data");
        var id = value.GetProperty("id").GetInt64();
        var text = value.GetProperty("data").GetString() ?? "";
        var data = Wire.Utf8.GetBytes(text);
        if (id <= lastInput || data.Length is < 1 or > 65536 || text.Contains('\0')) throw new InvalidDataException("Invalid input.");
        if (Interlocked.Add(ref inputBytes, data.Length) > 131072 || !writes.Writer.TryWrite((data, id, send))) throw new InvalidDataException("Input queue full.");
        lastInput = id;
    }
    internal void Resize(JsonElement value)
    {
        Wire.Fields(value, "type", "cols", "rows");
        using var watchdog = Watchdog.Start(8000);
        lock (closeLock)
        {
            if (closing != null) return;
            Marshal.ThrowExceptionForHR(ResizePseudoConsole(console, Dimensions(value)));
        }
    }
    internal Task WaitForExit(CancellationToken cancel) => Task.Run(async () =>
    {
        while (WaitForSingleObject(process, 0) == 258) await Task.Delay(20, cancel);
        cancel.ThrowIfCancellationRequested();
    });
    internal Task Close()
    {
        lock (closeLock) return closing ??= Task.Run(() =>
        {
            writes.Writer.TryComplete();
            Check(TerminateJobObject(job, 1));
            var deadline = Environment.TickCount64 + 5000;
            while (true)
            {
                Check(QueryInformationJobObject(job, 1, out var accounting, (uint)Marshal.SizeOf<JobAccounting>(), IntPtr.Zero));
                if (accounting.ActiveProcesses == 0 && WaitForSingleObject(process, 0) == 0) break;
                if (Environment.TickCount64 >= deadline) throw new TimeoutException("Job termination did not finish.");
                Thread.Sleep(20);
            }
            // Kernel job termination is now confirmed for all members. Keep output
            // draining while ClosePseudoConsole flushes; never kill by PID.
            if (console != IntPtr.Zero) { ClosePseudoConsole(console); console = IntPtr.Zero; }
        });
    }
    public void Dispose()
    {
        // Close already ran in Helper.finally. If it failed, job disposal still
        // applies KILL_ON_JOB_CLOSE to every member.
        job.Dispose(); process.Dispose(); input.Dispose(); Output.Dispose();
    }
    private static void Check(bool value) { if (!value) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    [StructLayout(LayoutKind.Sequential)] private struct Coord { public short X, Y; }
    [StructLayout(LayoutKind.Sequential)] private struct StartupInfo { public int Size; public IntPtr Reserved, Desktop, Title; public uint X, Y, XSize, YSize, XChars, YChars, Fill, Flags; public short Show, ReservedSize; public IntPtr Reserved2, In, Out, Error; }
    [StructLayout(LayoutKind.Sequential)] private struct StartupInfoEx { public StartupInfo Info; public IntPtr Attributes; }
    [StructLayout(LayoutKind.Sequential)] private struct ProcessInfo { public IntPtr Process, Thread; public uint Pid, Tid; }
    [StructLayout(LayoutKind.Sequential)] private struct BasicLimits { public long ProcessTime, JobTime; public uint LimitFlags; public nuint MinWorkingSet, MaxWorkingSet; public uint ActiveProcessLimit; public nuint Affinity; public uint Priority, Scheduling; }
    [StructLayout(LayoutKind.Sequential)] private struct JobAccounting { public long UserTime, KernelTime, PeriodUserTime, PeriodKernelTime; public uint PageFaults, TotalProcesses, ActiveProcesses, TerminatedProcesses; }
    [StructLayout(LayoutKind.Sequential)] private struct IoCounters { public ulong ReadOperations, WriteOperations, OtherOperations, ReadBytes, WriteBytes, OtherBytes; }
    [StructLayout(LayoutKind.Sequential)] private struct JobLimits { public BasicLimits Basic; public IoCounters Io; public nuint ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory; }
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool CreatePipe(out SafeFileHandle read, out SafeFileHandle write, IntPtr attributes, uint size);
    [DllImport("kernel32.dll")] private static extern int CreatePseudoConsole(Coord size, SafeFileHandle input, SafeFileHandle output, uint flags, out IntPtr console);
    [DllImport("kernel32.dll")] private static extern int ResizePseudoConsole(IntPtr console, Coord size);
    [DllImport("kernel32.dll")] private static extern void ClosePseudoConsole(IntPtr console);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CreateProcess(string application, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref StartupInfoEx startup, out ProcessInfo info);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, uint flags, ref nuint size);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, nuint size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll")] private static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint ResumeThread(SafeFileHandle thread);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern SafeFileHandle CreateJobObject(IntPtr attributes, string? name);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool SetInformationJobObject(SafeFileHandle job, int infoClass, ref JobLimits limits, uint size);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool QueryInformationJobObject(SafeFileHandle job, int infoClass, out JobAccounting accounting, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool AssignProcessToJobObject(SafeFileHandle job, SafeFileHandle process);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateJobObject(SafeFileHandle job, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateProcess(SafeFileHandle process, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetProcessTimes(SafeFileHandle process, out long creation, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetExitCodeProcess(SafeFileHandle process, out uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint WaitForSingleObject(SafeFileHandle process, uint milliseconds);
}
