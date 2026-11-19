using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Principal;

namespace Shellfox.ElevatedTerminal;

internal static class Helper
{
    internal static async Task<int> Run(string name, int parentPid, long parentBirth, bool isolated = false)
    {
        var stage = StartupStage.Elevation;
        try { return await Serve(name, parentPid, parentBirth, isolated, next => stage = next); }
        catch when (!isolated && stage != StartupStage.Active) { return (int)stage; }
    }

    private static async Task<int> Serve(string name, int parentPid, long parentBirth, bool isolated, Action<StartupStage> stage)
    {
        using var identity = WindowsIdentity.GetCurrent();
        if (!new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator)) throw new UnauthorizedAccessException("Elevation required.");
        stage(StartupStage.OpenLauncher);
        using var parent = LocalPipe.OpenParent(parentPid, parentBirth);
        using var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        // Anonymous impersonation level: the medium-integrity pipe server never
        // receives an impersonation-capable elevated client token.
        stage(StartupStage.ConnectToLauncher);
        using var pipe = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous, TokenImpersonationLevel.Anonymous);
        await pipe.ConnectAsync(cancel.Token);
        stage(StartupStage.VerifyLauncher);
        LocalPipe.Verify(pipe, parent, true);
        _ = parent.WaitForExitAsync(cancel.Token).ContinueWith(t =>
        {
            if (t.IsCompletedSuccessfully) cancel.Cancel();
        }, TaskScheduler.Default);
        stage(StartupStage.ReadLaunch);
        var launch = await Wire.ReadFrame(pipe, cancel.Token) ?? throw new EndOfStreamException();
        // The invocation mode is not host environment state. Both peers bind it
        // to the launch type only after verifying the original pipe peer.
        try { ConPty.ValidateLaunch(launch, isolated); }
        catch
        {
            await Wire.Send(pipe, Wire.Object(("type", "error"), ("code", "failed"), ("message", "Invalid administrator working directory or environment.")), false, cancel.Token);
            await Wire.Send(pipe, Wire.Object(("type", "exit"), ("exitCode", 1)), false, cancel.Token);
            return 1; // No child process was created.
        }
        stage(StartupStage.CreatePowerShell);
        cancel.CancelAfter(TimeSpan.FromSeconds(15));
        var initialized = 0;
        _ = Task.Run(async () =>
        {
            await Task.Delay(15000);
            if (Volatile.Read(ref initialized) == 0) Environment.Exit(isolated ? 1 : (int)StartupStage.CreatePowerShell);
        });
        using var terminal = new ConPty(launch, isolated);
        Volatile.Write(ref initialized, 1);
        cancel.CancelAfter(Timeout.InfiniteTimeSpan);
        var gate = new SemaphoreSlim(1);
        async Task Send(object value)
        {
            await gate.WaitAsync(cancel.Token);
            try { await Wire.Send(pipe, value, false, cancel.Token); }
            finally { gate.Release(); }
        }
        try
        {
            stage(StartupStage.SendReady);
            await Send(Wire.Object(("type", "ready"), ("pid", terminal.Pid), ("birth", terminal.Birth.ToString(System.Globalization.CultureInfo.InvariantCulture))));
            stage(StartupStage.Active);
            var output = Task.Run(async () =>
            {
                var bytes = new byte[4096];
                while (true)
                {
                    int read;
                    try { read = await terminal.Output.ReadAsync(bytes); }
                    catch (IOException) { break; } // Anonymous pipe closes with ERROR_BROKEN_PIPE.
                    if (read == 0) break;
                    try { await Send(Wire.Object(("type", "data"), ("data", Convert.ToBase64String(bytes, 0, read)))); }
                    catch { /* Keep draining ConPTY during disconnected cleanup. */ }
                }
            });
            var commands = Task.Run(async () =>
            {
                try
                {
                    while (await Wire.ReadFrame(pipe, cancel.Token) is { } command)
                    {
                        switch (Wire.Type(command))
                        {
                            case "write": terminal.QueueInput(command, Send); break;
                            case "resize": terminal.Resize(command); break;
                            case "close":
                                Wire.Fields(command, "type"); await Watchdog.Close(terminal);
                                await Task.Delay(Timeout.Infinite, cancel.Token); return;
                            default: throw new InvalidDataException("Unknown command.");
                        }
                    }
                }
                finally { cancel.Cancel(); }
            });
            await terminal.WaitForExit(cancel.Token);
            // Kill all descendants before claiming exit, including children left
            // behind by a naturally exiting PowerShell. No PID-only tree kill.
            await Watchdog.Close(terminal);
            await output.WaitAsync(TimeSpan.FromSeconds(5), cancel.Token);
            await Send(Wire.Object(("type", "exit"), ("exitCode", terminal.ExitCode)));
            return 0;
        }
        finally
        {
            cancel.Cancel();
            try { await Watchdog.Close(terminal); }
            catch { Environment.Exit(1); } // Process death closes the job handle.
        }
    }
}
