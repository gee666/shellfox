using System.ComponentModel;
using System.Diagnostics;

namespace Shellfox.ElevatedTerminal;

internal static class Launcher
{
    internal static async Task<int> Run(bool isolated = false)
    {
        using var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(isolated ? 120 : 15));
        using var input = new BufferedStream(Console.OpenStandardInput(), 4096);
        using var output = Console.OpenStandardOutput();
        using var parent = Process.GetCurrentProcess();
        return await RunPipe(input, output, parent, cancel, isolated, StartElevated);
    }

    private static Task<Process> StartElevated(ProcessStartInfo info) => Task.Run(() =>
        Process.Start(info) ?? throw new InvalidOperationException("UAC did not return a helper handle."));

    internal static async Task<int> RunPipe(Stream input, Stream output, Process parent, CancellationTokenSource cancel,
        bool isolated, Func<ProcessStartInfo, Task<Process>> startElevated)
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
        var stage = StartupStage.LaunchRequest;
        using var startup = CancellationTokenSource.CreateLinkedTokenSource(cancel.Token);
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
            // Let Windows own consent timing. Local CLOSE or EOF still
            // cancels our wait immediately.
            if (!isolated) cancel.CancelAfter(Timeout.InfiniteTimeSpan);
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
            stage = StartupStage.Consent;
            var connect = pipe.WaitForConnectionAsync(startup.Token);
            var start = startElevated(new ProcessStartInfo(Environment.ProcessPath!)
            {
                UseShellExecute = true, Verb = "runas", WindowStyle = ProcessWindowStyle.Hidden,
                Arguments = FormattableString.Invariant($"{(isolated ? "serve-isolated" : "serve")} {name} {parent.Id} {parent.StartTime.ToUniversalTime().ToFileTimeUtc()}")
            });
            // If cancellation wins, close the endpoint now. A delayed UAC approval
            // cannot launch a shell: the helper cannot authenticate/connect.
            if (isolated) isolatedStart = start;
            try { child = await start.WaitAsync(cancel.Token); }
            catch (OperationCanceledException) when (!isolated)
            {
                _ = start.ContinueWith(t =>
                {
                    if (t.IsCompletedSuccessfully) t.Result.Dispose();
                    else _ = t.Exception;
                }, TaskScheduler.Default);
                throw;
            }
            stage = StartupStage.Connect;
            if (isolated) originalPeer = new OriginalPeerExit(child);
            else
            {
                startup.CancelAfter(TimeSpan.FromSeconds(15));
                // A helper that failed before connecting cannot send an error.
                var peerExit = child.WaitForExitAsync(startup.Token);
                await Task.WhenAny(connect, peerExit).WaitAsync(startup.Token);
                if (peerExit.IsCompletedSuccessfully && !connect.IsCompletedSuccessfully)
                    throw new EndOfStreamException("Elevated helper exited before connecting.");
            }
            await connect;
            stage = StartupStage.VerifyHelper;
            LocalPipe.Verify(pipe, child, false);
            stage = StartupStage.SendLaunch;
            launchSent = true; // A partial frame might reach the authenticated peer.
            await Wire.Send(pipe, launch, false, startup.Token);
            if (isolated) cancel.CancelAfter(Timeout.InfiniteTimeSpan);
            // This separate deadline cannot overwrite a concurrent CLOSE deadline.
            else startup.CancelAfter(TimeSpan.FromSeconds(20));
            authenticated.SetResult();
            stage = StartupStage.Ready;
            while (await Wire.ReadFrame(pipe, stage == StartupStage.Ready ? startup.Token : cancel.Token) is { } message)
            {
                if (Wire.Type(message) == "ready")
                {
                    stage = StartupStage.Active;
                    startup.CancelAfter(Timeout.InfiniteTimeSpan);
                }
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
            var cancelled = cancel.IsCancellationRequested;
            if (isolated && isolatedStart != null) RetainIsolatedPeer();
            if (!isolated && child != null && !cancelled && stage is >= StartupStage.Connect and <= StartupStage.Ready)
                stage = await StartupFailure.ReadPeerStage(child, stage);
            cancel.Cancel(); // Disconnect first, before potentially blocked stdout.
            // Error strings do not include launch environment or user input.
            using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(2));
            var failure = StartupFailure.Frame(isolated && stage != StartupStage.Consent ? StartupStage.Active : stage, error, cancelled);
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
