using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

internal static class PipeRun
{
    internal static int Run(string file) => Run(PipeRunConfig.Load(file));

    internal static int Run(PipeRunConfig config)
    {
        using var job = CreateJobObject(IntPtr.Zero, null);
        Check(!job.IsInvalid);
        var limits = new JobLimits(); limits.Basic.LimitFlags = 0x2000; // KILL_ON_JOB_CLOSE, no breakaway.
        Check(SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf<JobLimits>()));
        var stdio = new List<SafeFileHandle>();
        SafeFileHandle? process = null, thread = null;
        var initialized = false;
        IntPtr attributes = IntPtr.Zero, handles = IntPtr.Zero, jobs = IntPtr.Zero, environment = IntPtr.Zero;
        try
        {
            foreach (var id in new[] { -10, -11, -12 })
            {
                var source = GetStdHandle(id);
                if (source == IntPtr.Zero || source == new IntPtr(-1)) throw new InvalidOperationException("Standard pipe handle unavailable.");
                Check(DuplicateHandle(GetCurrentProcess(), source, GetCurrentProcess(), out var copy, 0, true, 2));
                stdio.Add(copy);
            }
            nuint size = 0;
            InitializeProcThreadAttributeList(IntPtr.Zero, 2, 0, ref size);
            attributes = Marshal.AllocHGlobal(checked((int)size));
            Check(InitializeProcThreadAttributeList(attributes, 2, 0, ref size));
            initialized = true;
            handles = Marshal.AllocHGlobal(IntPtr.Size * 3);
            for (var i = 0; i < 3; i++) Marshal.WriteIntPtr(handles, i * IntPtr.Size, stdio[i].DangerousGetHandle());
            Check(UpdateProcThreadAttribute(attributes, 0, (IntPtr)0x20002, handles, (nuint)(IntPtr.Size * 3), IntPtr.Zero, IntPtr.Zero));
            jobs = Marshal.AllocHGlobal(IntPtr.Size);
            Marshal.WriteIntPtr(jobs, job.DangerousGetHandle());
            // Atomic job assignment prevents even a suspended orphan if this
            // helper is terminated between CreateProcess and ResumeThread.
            Check(UpdateProcThreadAttribute(attributes, 0, (IntPtr)0x2000d, jobs, (nuint)IntPtr.Size, IntPtr.Zero, IntPtr.Zero));
            var start = new StartupInfoEx { Info = new StartupInfo {
                Size = Marshal.SizeOf<StartupInfoEx>(), Flags = 0x100,
                In = stdio[0].DangerousGetHandle(), Out = stdio[1].DangerousGetHandle(), Error = stdio[2].DangerousGetHandle()
            }, Attributes = attributes };
            environment = Marshal.StringToHGlobalUni(config.EnvironmentBlock());
            const uint flags = 0x00080000 | 0x00000004 | 0x00000400 | 0x08000000; // extended, suspended, Unicode, no console.
            Check(CreateProcess(config.Executable, new StringBuilder(config.CommandLine()), IntPtr.Zero, IntPtr.Zero, true, flags, environment, config.Cwd, ref start, out var info));
            process = new SafeFileHandle(info.Process, true); thread = new SafeFileHandle(info.Thread, true);
            Check(IsProcessInJob(process, job, out var assigned));
            if (!assigned) throw new InvalidOperationException("Child was not contained in its job.");
            Check(ResumeThread(thread) != uint.MaxValue);
            if (WaitForSingleObject(process, uint.MaxValue) != 0) throw new Win32Exception(Marshal.GetLastWin32Error());
            Check(GetExitCodeProcess(process, out var code));
            // Root exit is not tree exit. Descendants may still own the pipes.
            Check(TerminateJobObject(job, 1));
            var deadline = Environment.TickCount64 + 5000;
            while (true)
            {
                Check(QueryInformationJobObject(job, 1, out var accounting, (uint)Marshal.SizeOf<JobAccounting>(), IntPtr.Zero));
                if (accounting.ActiveProcesses == 0) break;
                if (Environment.TickCount64 >= deadline) throw new TimeoutException("Job termination did not finish.");
                Thread.Sleep(10);
            }
            return unchecked((int)code);
        }
        catch
        {
            TerminateJobObject(job, 1);
            if (process is not null) TerminateProcess(process, 1);
            throw;
        }
        finally
        {
            thread?.Dispose(); process?.Dispose();
            foreach (var handle in stdio) handle.Dispose();
            if (initialized) DeleteProcThreadAttributeList(attributes);
            if (attributes != IntPtr.Zero) Marshal.FreeHGlobal(attributes);
            if (handles != IntPtr.Zero) Marshal.FreeHGlobal(handles);
            if (jobs != IntPtr.Zero) Marshal.FreeHGlobal(jobs);
            if (environment != IntPtr.Zero) Marshal.FreeHGlobal(environment);
        }
    }

    private static void Check(bool value) { if (!value) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    [StructLayout(LayoutKind.Sequential)] private struct StartupInfo { public int Size; public IntPtr Reserved, Desktop, Title; public uint X, Y, XSize, YSize, XChars, YChars, Fill, Flags; public short Show, ReservedSize; public IntPtr Reserved2, In, Out, Error; }
    [StructLayout(LayoutKind.Sequential)] private struct StartupInfoEx { public StartupInfo Info; public IntPtr Attributes; }
    [StructLayout(LayoutKind.Sequential)] private struct ProcessInfo { public IntPtr Process, Thread; public uint Pid, Tid; }
    [StructLayout(LayoutKind.Sequential)] private struct BasicLimits { public long ProcessTime, JobTime; public uint LimitFlags; public nuint MinWorkingSet, MaxWorkingSet; public uint ActiveProcessLimit; public nuint Affinity; public uint Priority, Scheduling; }
    [StructLayout(LayoutKind.Sequential)] private struct JobAccounting { public long UserTime, KernelTime, PeriodUserTime, PeriodKernelTime; public uint PageFaults, TotalProcesses, ActiveProcesses, TerminatedProcesses; }
    [StructLayout(LayoutKind.Sequential)] private struct IoCounters { public ulong ReadOperations, WriteOperations, OtherOperations, ReadBytes, WriteBytes, OtherBytes; }
    [StructLayout(LayoutKind.Sequential)] private struct JobLimits { public BasicLimits Basic; public IoCounters Io; public nuint ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory; }
    [DllImport("kernel32.dll")] private static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError = true)] private static extern IntPtr GetStdHandle(int id);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool DuplicateHandle(IntPtr sourceProcess, IntPtr source, IntPtr targetProcess, out SafeFileHandle target, uint access, bool inherit, uint options);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern SafeFileHandle CreateJobObject(IntPtr attributes, string? name);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool SetInformationJobObject(SafeFileHandle job, int infoClass, ref JobLimits limits, uint size);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool QueryInformationJobObject(SafeFileHandle job, int infoClass, out JobAccounting accounting, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool IsProcessInJob(SafeFileHandle process, SafeFileHandle job, out bool assigned);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateJobObject(SafeFileHandle job, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateProcess(SafeFileHandle process, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, uint flags, ref nuint size);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, nuint size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll")] private static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CreateProcess(string application, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref StartupInfoEx startup, out ProcessInfo info);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint ResumeThread(SafeFileHandle thread);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint WaitForSingleObject(SafeFileHandle process, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetExitCodeProcess(SafeFileHandle process, out uint code);
}
