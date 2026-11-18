using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

internal interface IOriginalPeerExit
{
    Task WaitForExit();
}

// The Process came directly from ShellExecute, before authentication/launch.
// Cache its original handle. Never reopen by PID, poll birth or infer from time.
internal sealed class OriginalPeerExit : IOriginalPeerExit
{
    private readonly Process original;
    private readonly SafeProcessHandle handle;
    internal OriginalPeerExit(Process original) { this.original = original; handle = original.SafeHandle; }
    public Task WaitForExit() => Task.Factory.StartNew(() =>
    {
        var result = WaitForSingleObject(handle, uint.MaxValue);
        GC.KeepAlive(original);
        if (result != 0) throw new Win32Exception(Marshal.GetLastWin32Error());
    }, CancellationToken.None, TaskCreationOptions.LongRunning, TaskScheduler.Default);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(SafeProcessHandle process, uint milliseconds);
}

// A late ShellExecute result must stay owned after host cancellation. A null
// result is allowed only when the caller proved ShellExecute created no peer.
internal sealed class PendingOriginalPeerExit(Task<IOriginalPeerExit?> pending) : IOriginalPeerExit
{
    public async Task WaitForExit()
    {
        if (await pending is { } original) await original.WaitForExit();
    }
}

// Serializes abort scheduling with the transition to retained peer-exit waiting.
// A CLOSE/EOF callback or an already queued timer must not kill this owner later.
internal sealed class IsolatedExitPhase
{
    private readonly object gate = new();
    private bool waiting;
    internal bool Waiting { get { lock (gate) return waiting; } }
    internal void Begin(Action disarm)
    {
        lock (gate) { waiting = true; disarm(); }
    }
    internal void UnlessWaiting(Action action)
    {
        lock (gate) { if (!waiting) action(); }
    }
}

internal static class IsolatedPeerExit
{
    internal static void Validate(JsonElement exit)
    {
        Wire.Fields(exit, "type", "exitCode");
        if (Wire.Type(exit) != "exit") throw new InvalidDataException("Expected authenticated exit.");
        _ = exit.GetProperty("exitCode").GetInt32();
    }
    internal static async Task<bool> Forward(Stream privateChannel, IOriginalPeerExit original, JsonElement exit,
        Func<object, CancellationToken, Task> send, TimeSpan? deadline = null, TimeSpan? retry = null)
    {
        Validate(exit);
        await Confirm(privateChannel, original, send, deadline, retry);
        using var finalReply = new CancellationTokenSource(TimeSpan.FromSeconds(2));
        try { await send(exit, finalReply.Token).WaitAsync(finalReply.Token); return true; }
        catch { return false; } // Missing delivery is never a fabricated confirmation.
    }
    internal static async Task Confirm(Stream privateChannel, IOriginalPeerExit original,
        Func<object, CancellationToken, Task> status, TimeSpan? deadline = null, TimeSpan? retry = null)
    {
        // This proves peer exit only, not Job cleanup. Forward additionally
        // requires the caller's authenticated Job-zero frame. Close the private
        // command reader first; host stdin stays open until the stronger reply.
        try { privateChannel.Dispose(); } catch { /* Still require OS exit. */ }
        var reported = false;
        async Task Unconfirmed()
        {
            if (reported) return;
            reported = true;
            using var limit = new CancellationTokenSource(TimeSpan.FromSeconds(2));
            try
            {
                await status(Wire.Object(("type", "error"), ("code", "failed"),
                    ("message", "Administrator terminal peer exit is not confirmed.")), limit.Token).WaitAsync(limit.Token);
            }
            catch { /* A broken host cannot release the original peer owner. */ }
        }
        while (true)
        {
            try
            {
                var exited = original.WaitForExit();
                if (!reported)
                {
                    try { await exited.WaitAsync(deadline ?? TimeSpan.FromSeconds(8)); }
                    catch (TimeoutException) { await Unconfirmed(); }
                }
                // No host/close cancellation token. On timeout keep the launcher
                // alive with the original owner until a real kernel exit signal.
                await exited;
                return;
            }
            catch
            {
                await Unconfirmed();
                // Retry the same original handle. A delay is never exit proof.
                await Task.Delay(retry ?? TimeSpan.FromSeconds(1));
            }
        }
    }
}
