using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

internal sealed class PowerOwner : IDisposable
{
    private readonly SafeProcessHandle owner;
    internal PowerOwner(PowerInit init)
    {
        owner = new SafeProcessHandle((IntPtr)init.OwnerHandle, ownsHandle: true);
        try
        {
            // Original inherited process handle, not GetProcessById/OpenProcess.
            var pid = GetProcessId(owner);
            if (pid == 0 || pid == GetCurrentProcessId() || !GetProcessTimes(owner, out var birth, out _, out _, out _) || birth != init.OwnerBirth || !Alive)
                throw new UnauthorizedAccessException("Invalid original power owner.");
            // The guardian must survive its owner's Job teardown. Reject even
            // benign enclosing Jobs instead of guessing about nested Job limits.
            if (!IsProcessInJob(GetCurrentProcess(), IntPtr.Zero, out var inJob) || inJob)
                throw new UnauthorizedAccessException("Power guardian must run outside Jobs.");
            if (GetFileType(GetStdHandle(-10)) != 3 || GetFileType(GetStdHandle(-11)) != 3)
                throw new UnauthorizedAccessException("Private inherited pipes required.");
        }
        catch { owner.Dispose(); throw; }
    }
    internal bool Alive
    {
        get
        {
            var state = WaitForSingleObject(owner, 0);
            if (state == 258) return true;
            if (state == 0) return false;
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
    }
    internal static (uint Pid, string Birth) Self()
    {
        using var handle = new SafeProcessHandle(GetCurrentProcess(), ownsHandle: false);
        if (!GetProcessTimes(handle, out var birth, out _, out _, out _)) throw new Win32Exception(Marshal.GetLastWin32Error());
        return (GetCurrentProcessId(), birth.ToString(System.Globalization.CultureInfo.InvariantCulture));
    }
    public void Dispose() => owner.Dispose();
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern IntPtr GetCurrentProcess();
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern uint GetCurrentProcessId();
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint GetProcessId(SafeProcessHandle handle);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetProcessTimes(SafeProcessHandle handle, out long birth, out long exit, out long kernel, out long user);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint WaitForSingleObject(SafeProcessHandle handle, uint milliseconds);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern IntPtr GetStdHandle(int id);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint GetFileType(IntPtr handle);
}
