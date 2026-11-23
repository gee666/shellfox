using System.Diagnostics;

namespace Shellfox.ElevatedTerminal;

internal static class Launcher
{
    internal static async Task<int> Run()
    {
        using var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        using var input = new BufferedStream(Console.OpenStandardInput(), 4096);
        using var output = Console.OpenStandardOutput();
        using var parent = Process.GetCurrentProcess();
        return await RunPipe(input, output, parent, cancel, StartElevated);
    }

    private static Task<Process> StartElevated(ProcessStartInfo info) => Task.Run(() =>
        Process.Start(info) ?? throw new InvalidOperationException("UAC did not return a helper handle."));

    internal static async Task<int> RunPipe(Stream input, Stream output, Process parent, CancellationTokenSource cancel,
        Func<ProcessStartInfo, Task<Process>> startElevated)
    {
        var name = "shellfox-admin-" + Guid.NewGuid().ToString("N");
        using var pipe = LocalPipe.Create(name);
        // Closing the pipe must not depend on stdout accepting more bytes. A
        // stalled host cannot keep its elevated job alive after CLOSE/EOF.
        using var abort = new Timer(_ => Environment.Exit(1), null, Timeout.Infinite, Timeout.Infinite);
        using var disconnect = cancel.Token.Register(() =>
        {
            pipe.Dispose();
            abort.Change(3000, Timeout.Infinite);
        });
        Process? child = null;
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
        try
        {
            var launch = await Wire.ReadLine(input, cancel.Token) ?? throw new EndOfStreamException();
            ConPty.ValidateLaunch(launch);
            // Let Windows own consent timing. Local CLOSE or EOF still
            // cancels our wait immediately.
            cancel.CancelAfter(Timeout.InfiniteTimeSpan);
            // Keep reading CLOSE/EOF while ShellExecute waits on the secure desktop.
            var commands = Task.Run(async () =>
            {
                try
                {
                    while (await Wire.ReadLine(input, cancel.Token) is { } command)
                    {
                        if (Wire.Type(command) == "close")
                        {
                            Wire.Fields(command, "type");
                            if (!authenticated.Task.IsCompletedSuccessfully) { cancel.Cancel(); return; }
                            cancel.CancelAfter(TimeSpan.FromSeconds(10));
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
                Arguments = FormattableString.Invariant($"serve {name} {parent.Id} {parent.StartTime.ToUniversalTime().ToFileTimeUtc()}")
            });
            // If cancellation wins, close the endpoint now. A delayed UAC approval
            // cannot launch a shell: the helper cannot authenticate/connect.
            try { child = await start.WaitAsync(cancel.Token); }
            catch (OperationCanceledException)
            {
                _ = start.ContinueWith(t =>
                {
                    if (t.IsCompletedSuccessfully) t.Result.Dispose();
                    else _ = t.Exception;
                }, TaskScheduler.Default);
                throw;
            }
            stage = StartupStage.Connect;
            startup.CancelAfter(TimeSpan.FromSeconds(15));
            // A helper that failed before connecting cannot send an error.
            var peerExit = child.WaitForExitAsync(startup.Token);
            await Task.WhenAny(connect, peerExit).WaitAsync(startup.Token);
            if (peerExit.IsCompletedSuccessfully && !connect.IsCompletedSuccessfully)
                throw new EndOfStreamException("Elevated helper exited before connecting.");
            await connect;
            stage = StartupStage.VerifyHelper;
            LocalPipe.Verify(pipe, child, false);
            stage = StartupStage.SendLaunch;
            launchSent = true; // A partial frame might reach the authenticated peer.
            await Wire.Send(pipe, launch, false, startup.Token);
            // This separate deadline cannot overwrite a concurrent CLOSE deadline.
            startup.CancelAfter(TimeSpan.FromSeconds(20));
            authenticated.SetResult();
            stage = StartupStage.Ready;
            while (await Wire.ReadFrame(pipe, stage == StartupStage.Ready ? startup.Token : cancel.Token) is { } message)
            {
                if (Wire.Type(message) == "ready")
                {
                    stage = StartupStage.Active;
                    startup.CancelAfter(Timeout.InfiniteTimeSpan);
                }
                await Send(message);
                if (Wire.Type(message) == "exit") { exited = true; return 0; }
            }
            throw new EndOfStreamException("Elevated helper disconnected.");
        }
        catch (Exception error)
        {
            var cancelled = cancel.IsCancellationRequested;
            if (child != null && !cancelled && stage is >= StartupStage.Connect and <= StartupStage.Ready)
                stage = await StartupFailure.ReadPeerStage(child, stage);
            cancel.Cancel(); // Disconnect first, before potentially blocked stdout.
            // Error strings do not include launch environment or user input.
            using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(2));
            try { await Wire.Send(output, StartupFailure.Frame(stage, error, cancelled), true, deadline.Token); }
            catch { }
            return 1;
        }
        finally
        {
            cancel.Cancel(); pipe.Dispose();
            // A delayed UAC approval cannot launch after the endpoint is gone.
            // Once the launch frame was sent, process death kills the job but
            // cannot prove all asynchronous termination finished. Only the
            // helper's zero-member job acknowledgement can confirm that case.
            if (!exited && !launchSent)
            {
                try
                {
                    using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(2));
                    await Wire.Send(output, Wire.Object(("type", "exit"), ("exitCode", 1)), true, deadline.Token);
                }
                catch { /* Host retains unconfirmed cleanup ownership. */ }
            }
            child?.Dispose();
        }
    }
}
