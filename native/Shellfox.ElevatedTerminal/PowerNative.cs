using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

internal interface IPowerNative
{
    Guid ActiveScheme();
    uint ReadLid(Guid scheme, bool ac);
    void WriteLid(Guid scheme, bool ac, uint value);
    void RefreshIfActive(Guid scheme);
    IDisposable Requests();
    void MousePulse();
}

internal sealed class PowerNative : IPowerNative
{
    private static readonly Guid Buttons = new("4f971e89-eebd-4455-a8de-9e59040e7347");
    private static readonly Guid Lid = new("5ca83367-6e45-459f-a27b-476b1d01c936");
    public Guid ActiveScheme()
    {
        Check(PowerGetActiveScheme(IntPtr.Zero, out var pointer));
        try { return Marshal.PtrToStructure<Guid>(pointer); }
        finally { LocalFree(pointer); }
    }
    public uint ReadLid(Guid scheme, bool ac)
    {
        Check(ac ? PowerReadACValueIndex(IntPtr.Zero, in scheme, in Buttons, in Lid, out var value) :
            PowerReadDCValueIndex(IntPtr.Zero, in scheme, in Buttons, in Lid, out value));
        return value;
    }
    public void WriteLid(Guid scheme, bool ac, uint value) => Check(ac ?
        PowerWriteACValueIndex(IntPtr.Zero, in scheme, in Buttons, in Lid, value) :
        PowerWriteDCValueIndex(IntPtr.Zero, in scheme, in Buttons, in Lid, value));
    public void RefreshIfActive(Guid scheme)
    {
        // Never switch back to an old plan. Windows has no compare-and-set API;
        // minimize the query/set gap, and never use SCHEME_CURRENT for writes.
        if (ActiveScheme() == scheme) Check(PowerSetActiveScheme(IntPtr.Zero, in scheme));
    }
    public IDisposable Requests()
    {
        var context = new ReasonContext { Version = 0, Flags = 1, Reason = Marshal.StringToHGlobalUni("Shellfox keep awake") };
        SafeFileHandle handle;
        try { handle = PowerCreateRequest(in context); }
        finally { Marshal.FreeHGlobal(context.Reason); }
        if (handle.IsInvalid) { handle.Dispose(); throw new Win32Exception(Marshal.GetLastWin32Error()); }
        try
        {
            // Kernel requests are tied to this owned handle, not a thread/pid.
            for (var type = 0; type < 3; type++)
                if (!PowerSetRequest(handle, type)) throw new Win32Exception(Marshal.GetLastWin32Error());
            return handle; // Closing clears every request, including partial setup.
        }
        catch { handle.Dispose(); throw; }
    }
    public void MousePulse()
    {
        if (!GetCursorPos(out var point)) throw new Win32Exception(Marshal.GetLastWin32Error());
        // Choose the first step toward the virtual desktop center, so the first
        // event does not clamp at its outer edge. Submit the reversible pair as
        // one batch. No configurable input or commands enter this helper.
        var center = GetSystemMetrics(76) + GetSystemMetrics(78) / 2;
        var dx = point.X >= center ? -1 : 1;
        var input = new[] { Move(dx), Move(-dx) };
        var sent = SendInput(2, input, Marshal.SizeOf<Input>());
        if (sent == 1) SendInput(1, new[] { Move(-dx) }, Marshal.SizeOf<Input>());
        if (sent != 2) throw new Win32Exception(Marshal.GetLastWin32Error());
    }
    private static Input Move(int dx) => new() { Type = 0, Data = new MouseInput { Dx = dx, Flags = 1 } };
    private static void Check(uint code) { if (code != 0) throw new Win32Exception((int)code); }
    internal static (int Reason, int Input) Layout() => (Marshal.SizeOf<ReasonContext>(), Marshal.SizeOf<Input>());
    [StructLayout(LayoutKind.Sequential)] private struct ReasonContext { public uint Version, Flags; public IntPtr Reason, Reserved1, Reserved2; }
    [StructLayout(LayoutKind.Sequential)] private struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] private struct MouseInput { public int Dx, Dy; public uint Data, Flags, Time; public nuint Extra; }
    // INPUT's union is as large as MOUSEINPUT on both supported architectures.
    [StructLayout(LayoutKind.Sequential)] private struct Input { public uint Type; public MouseInput Data; }
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("powrprof.dll")] private static extern uint PowerGetActiveScheme(IntPtr root, out IntPtr scheme);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("powrprof.dll")] private static extern uint PowerReadACValueIndex(IntPtr root, in Guid scheme, in Guid subgroup, in Guid setting, out uint value);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("powrprof.dll")] private static extern uint PowerReadDCValueIndex(IntPtr root, in Guid scheme, in Guid subgroup, in Guid setting, out uint value);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("powrprof.dll")] private static extern uint PowerWriteACValueIndex(IntPtr root, in Guid scheme, in Guid subgroup, in Guid setting, uint value);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("powrprof.dll")] private static extern uint PowerWriteDCValueIndex(IntPtr root, in Guid scheme, in Guid subgroup, in Guid setting, uint value);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("powrprof.dll")] private static extern uint PowerSetActiveScheme(IntPtr root, in Guid scheme);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr value);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern SafeFileHandle PowerCreateRequest(in ReasonContext context);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool PowerSetRequest(SafeFileHandle handle, int type);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("user32.dll", SetLastError = true)] private static extern bool GetCursorPos(out Point point);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("user32.dll")] private static extern int GetSystemMetrics(int index);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("user32.dll", SetLastError = true)] private static extern uint SendInput(uint count, Input[] input, int size);
}
