using System.ComponentModel;
using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Principal;
using Shellfox.ElevatedTerminal;

if (!OperatingSystem.IsWindowsVersionAtLeast(10, 0, 17763))
{
    if (args.Length > 0 && args[0] == "pipe-run") Console.Error.WriteLine("Shellfox pipe-run requires Windows 10 version 1809 or newer.");
    return 2;
}
try
{
    if (args is ["self-test"]) return await SelfTest.Run();
    if (args is ["self-test", "--profiler-dll", var dll] && Path.IsPathFullyQualified(dll) && File.Exists(dll)) return await SelfTest.Run(dll);
    if (args is ["probe"]) { Console.WriteLine("shellfox-elevated-conpty-v1"); return 0; }
    if (args is ["probe-isolated"]) { Console.WriteLine(IsolatedLaunch.Probe); return 0; }
    if (args is ["probe-power"]) { Console.WriteLine(PowerProtocol.Probe); return 0; }
    if (args is ["power-guardian"]) return PowerGuardian.Run();
    if (args is ["pipe-run", var config]) return PipeRun.Run(config);
    if (args.Length > 0 && args[0] == "pipe-run") throw new InvalidDataException("Expected one config path.");
    if (args is ["launch"]) return await Launcher.Run();
    if (args is ["launch-isolated"]) return await Launcher.Run(isolated: true);
    if (args is ["serve" or "serve-isolated", var name, var pid, var birth] && name.StartsWith("shellfox-admin-", StringComparison.Ordinal) && name.Length == 47 &&
        Guid.TryParseExact(name[15..], "N", out _) && int.TryParse(pid, out var parentPid) && parentPid > 0 && long.TryParse(birth, out var parentBirth))
        return await Helper.Run(name, parentPid, parentBirth, isolated: args[0] == "serve-isolated");
    return 2;
}
catch (Exception error)
{
    if (args.Length > 0 && args[0] == "self-test") Console.Error.WriteLine(error);
    if (args.Length > 0 && args[0] == "pipe-run") Console.Error.WriteLine(error is Win32Exception win ? $"Shellfox pipe-run failed, Windows error {win.NativeErrorCode}." : "Shellfox pipe-run failed.");
    return 1;
}

internal static class Launcher
{
    internal static async Task<int> Run(bool isolated = false)
    {
        using var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(120));
        using var input = new BufferedStream(Console.OpenStandardInput(), 4096);
        using var output = Console.OpenStandardOutput();
        using var parent = Process.GetCurrentProcess();
        return await RunPipe(input, output, parent, cancel, isolated);
    }
    private static async Task<int> RunPipe(Stream input, Stream output, Process parent, CancellationTokenSource cancel, bool isolated)
    {
        var name = "shellfox-admin-" + Guid.NewGuid().ToString("N");
        using var pipe = LocalPipe.Create(name);
        // Closing the pipe must not depend on stdout accepting more bytes. A
        // stalled host cannot keep its elevated job alive after CLOSE/EOF.
        var isolatedExit = new IsolatedExitPhase();
        using var abort = new Timer(_ =>
        {
            if (isolated) isolatedExit.UnlessWaiting(() => Environment.Exit(1));
            else Environment.Exit(1);
        }, null, Timeout.Infinite, Timeout.Infinite);
        using var disconnect = cancel.Token.Register(() =>
        {
            pipe.Dispose();
            if (isolated) isolatedExit.UnlessWaiting(() => abort.Change(3000, Timeout.Infinite));
            else abort.Change(3000, Timeout.Infinite);
        });
        Process? child = null;
        IOriginalPeerExit? originalPeer = null;
        Task<Process>? isolatedStart = null;
        var exited = false;
        var launchSent = false;
        var authenticated = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var writeGate = new SemaphoreSlim(1);
        async Task Send(object value)
        {
            await writeGate.WaitAsync(cancel.Token);
            try { await Wire.Send(output, value, true, cancel.Token); }
            finally { writeGate.Release(); }
        }
        void RetainIsolatedPeer() => isolatedExit.Begin(() =>
        {
            abort.Change(Timeout.Infinite, Timeout.Infinite);
            cancel.CancelAfter(Timeout.InfiniteTimeSpan);
        });
        async Task<IOriginalPeerExit?> AcquireIsolatedPeer()
        {
            if (child == null)
            {
                try { child = await (isolatedStart ?? throw new InvalidOperationException("No peer attempt.")); }
                catch (Win32Exception error) when (error.NativeErrorCode == 1223) { return null; } // Only ShellExecute decline proves no peer.
            }
            return originalPeer ??= new OriginalPeerExit(child);
        }
        async Task SendIsolated(object value, CancellationToken token)
        {
            await writeGate.WaitAsync(token);
            try { await Wire.Send(output, value, true, token); }
            finally { writeGate.Release(); }
        }
        try
        {
            var launch = await Wire.ReadLine(input, cancel.Token) ?? throw new EndOfStreamException();
            ConPty.ValidateLaunch(launch, isolated);
            // Keep reading CLOSE/EOF while ShellExecute waits on the secure desktop.
            var commands = Task.Run(async () =>
            {
                try
                {
                    while (await Wire.ReadLine(input, cancel.Token) is { } command)
                    {
                        if (isolated && isolatedExit.Waiting) await Task.Delay(Timeout.Infinite, cancel.Token);
                        if (Wire.Type(command) == "close")
                        {
                            Wire.Fields(command, "type");
                            if (!authenticated.Task.IsCompletedSuccessfully) { cancel.Cancel(); return; }
                            if (isolated) isolatedExit.UnlessWaiting(() => cancel.CancelAfter(TimeSpan.FromSeconds(10)));
                            else cancel.CancelAfter(TimeSpan.FromSeconds(10));
                            await Wire.Send(pipe, command, false, cancel.Token);
                            // Wait for the authenticated cleanup/exit reply.
                            await Task.Delay(Timeout.Infinite, cancel.Token);
                        }
                        if (!authenticated.Task.IsCompletedSuccessfully) throw new InvalidDataException("Input before authentication.");
                        await Wire.Send(pipe, command, false, cancel.Token);
                    }
                }
                finally { cancel.Cancel(); }
            });
            var connect = pipe.WaitForConnectionAsync(cancel.Token);
            var start = Task.Run(() => Process.Start(new ProcessStartInfo(Environment.ProcessPath!)
            {
                UseShellExecute = true, Verb = "runas", WindowStyle = ProcessWindowStyle.Hidden,
                Arguments = $"{(isolated ? "serve-isolated" : "serve")} {name} {parent.Id} {parent.StartTime.ToUniversalTime().ToFileTimeUtc()}"
            }) ?? throw new InvalidOperationException("UAC did not return a helper handle."));
            // If cancellation wins, close the endpoint now. A delayed UAC approval
            // cannot launch a shell: the helper cannot authenticate/connect.
            if (isolated) isolatedStart = start;
            else _ = start.ContinueWith(t => { if (t.Status == TaskStatus.RanToCompletion && cancel.IsCancellationRequested) t.Result.Dispose(); }, TaskScheduler.Default);
            child = await start.WaitAsync(cancel.Token);
            if (isolated) originalPeer = new OriginalPeerExit(child);
            await connect;
            LocalPipe.Verify(pipe, child, false);
            launchSent = true; // A partial frame might reach the authenticated peer.
            await Wire.Send(pipe, launch, false, cancel.Token);
            // Disable the startup timer before publishing authentication. A
            // concurrent CLOSE must not have its own deadline overwritten.
            cancel.CancelAfter(Timeout.InfiniteTimeSpan);
            authenticated.SetResult();
            while (await Wire.ReadFrame(pipe, cancel.Token) is { } message)
            {
                if (isolated && Wire.Type(message) == "exit")
                {
                    IsolatedPeerExit.Validate(message);
                    RetainIsolatedPeer();
                    // CLOSE/host EOF can cancel the command bridge, not this
                    // retained wait or a late strong reply on independent tokens.
                    var forwarded = await IsolatedPeerExit.Forward(pipe,
                        originalPeer ?? throw new InvalidOperationException("Missing original peer."), message, SendIsolated);
                    exited = true;
                    return forwarded ? 0 : 1;
                }
                await Send(message);
                if (Wire.Type(message) == "exit") { exited = true; return 0; }
            }
            throw new EndOfStreamException("Elevated helper disconnected.");
        }
        catch (Exception error)
        {
            var code = error is Win32Exception win && win.NativeErrorCode == 1223 ? "cancelled" : cancel.IsCancellationRequested ? "cancelled" : "failed";
            if (isolated && isolatedStart != null) RetainIsolatedPeer();
            cancel.Cancel(); // Disconnect first, before potentially blocked stdout.
            // Error strings do not include launch environment or user input.
            using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(2));
            var failure = Wire.Object(("type", "error"), ("code", code), ("message", code == "cancelled" ? "Administrator PowerShell was cancelled or timed out." : "The elevated terminal helper could not start or disconnected."));
            try
            {
                if (isolated) await SendIsolated(failure, deadline.Token).WaitAsync(deadline.Token);
                else await Wire.Send(output, failure, true, deadline.Token);
            }
            catch { }
            return 1;
        }
        finally
        {
            // Retain late UAC results and the original peer even on startup or
            // transport failure. Peer death alone never confirms a launched Job.
            var noLaunchPeerExited = false;
            if (isolated && isolatedStart != null && !exited)
            {
                RetainIsolatedPeer();
                await IsolatedPeerExit.Confirm(pipe, new PendingOriginalPeerExit(AcquireIsolatedPeer()), SendIsolated);
                noLaunchPeerExited = !launchSent;
            }
            cancel.Cancel(); pipe.Dispose();
            // A delayed UAC approval cannot launch after the endpoint is gone.
            // Once the launch frame was sent, process death kills the job but
            // cannot prove all asynchronous termination finished. Only the
            // helper's zero-member job acknowledgement can confirm that case.
            if (!exited && !launchSent && (!isolated || isolatedStart == null || noLaunchPeerExited))
            {
                try
                {
                    using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(2));
                    var noLaunch = Wire.Object(("type", "exit"), ("exitCode", 1));
                    if (isolated) await SendIsolated(noLaunch, deadline.Token).WaitAsync(deadline.Token);
                    else await Wire.Send(output, noLaunch, true, deadline.Token);
                }
                catch { /* Host retains unconfirmed cleanup ownership. */ }
            }
            child?.Dispose();
        }
    }
}

internal static class Helper
{
    internal static async Task<int> Run(string name, int parentPid, long parentBirth, bool isolated = false)
    {
        using var identity = WindowsIdentity.GetCurrent();
        if (!new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator)) throw new UnauthorizedAccessException("Elevation required.");
        using var parent = LocalPipe.OpenParent(parentPid, parentBirth);
        using var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        // Anonymous impersonation level: the medium-integrity pipe server never
        // receives an impersonation-capable elevated client token.
        using var pipe = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous, TokenImpersonationLevel.Anonymous);
        await pipe.ConnectAsync(cancel.Token);
        LocalPipe.Verify(pipe, parent, true);
        _ = parent.WaitForExitAsync(cancel.Token).ContinueWith(_ => cancel.Cancel(), TaskScheduler.Default);
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
        cancel.CancelAfter(TimeSpan.FromSeconds(15));
        var initialized = 0;
        _ = Task.Run(async () => { await Task.Delay(15000); if (Volatile.Read(ref initialized) == 0) Environment.Exit(1); });
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
            await Send(Wire.Object(("type", "ready"), ("pid", terminal.Pid), ("birth", terminal.Birth.ToString())));
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
