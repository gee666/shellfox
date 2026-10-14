using System.ComponentModel;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Principal;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

internal static class LocalPipe
{
    [StructLayout(LayoutKind.Sequential)] private struct SecurityAttributes { public int Length; public IntPtr Descriptor; public int Inherit; }
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(string sddl, uint revision, out IntPtr descriptor, out uint size);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern SafePipeHandle CreateNamedPipe(string name, uint mode, uint pipeMode, uint instances, uint outSize, uint inSize, uint timeout, ref SecurityAttributes attributes);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetNamedPipeClientProcessId(SafePipeHandle pipe, out uint pid);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint pid);

    internal static NamedPipeServerStream Create(string name)
    {
        var sid = WindowsIdentity.GetCurrent().User?.Value ?? throw new InvalidOperationException("No user SID.");
        // Protected DACL, no Everyone/Users grant. Administrators permits
        // over-the-shoulder UAC credentials; exact live peer PID still applies.
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor($"D:P(A;;GA;;;{sid})(A;;GA;;;SY)(A;;GA;;;BA)S:(ML;;NW;;;ME)", 1, out var descriptor, out _)) throw new Win32Exception();
        try
        {
            var attributes = new SecurityAttributes { Length = Marshal.SizeOf<SecurityAttributes>(), Descriptor = descriptor };
            // FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED | PIPE_ACCESS_DUPLEX.
            // PIPE_REJECT_REMOTE_CLIENTS, byte mode, exactly one instance.
            var handle = CreateNamedPipe(@"\\.\pipe\" + name, 0x40080003, 8, 1, 65536, 65536, 0, ref attributes);
            if (handle.IsInvalid) { handle.Dispose(); throw new Win32Exception(); }
            try { return new NamedPipeServerStream(PipeDirection.InOut, true, false, handle); }
            catch { handle.Dispose(); throw; }
        }
        finally { LocalFree(descriptor); }
    }
    internal static void Verify(PipeStream pipe, Process expected, bool server)
    {
        uint pid;
        var ok = server ? GetNamedPipeServerProcessId(pipe.SafePipeHandle, out pid) : GetNamedPipeClientProcessId(pipe.SafePipeHandle, out pid);
        // The Process instance holds a live OS handle, not a reusable numeric PID.
        if (!ok || expected.HasExited || pid != expected.Id) throw new UnauthorizedAccessException("Unexpected pipe peer.");
    }
    internal static Process OpenParent(int pid, long birth)
    {
        var parent = Process.GetProcessById(pid);
        try
        {
            _ = parent.Handle;
            if (parent.HasExited || parent.StartTime.ToUniversalTime().ToFileTimeUtc() != birth) throw new UnauthorizedAccessException("Parent identity changed.");
            return parent;
        }
        catch { parent.Dispose(); throw; }
    }
}
