using System.Text.Json;
using System.Threading.Channels;

namespace Shellfox.ElevatedTerminal;

internal static class PowerGuardian
{
    internal static int Run()
    {
        // Do not synchronously dispose stdin while its reader may be blocked.
        // This single-purpose process exits after rollback and closes its pipes.
        var input = new BufferedStream(Console.OpenStandardInput(), 4096);
        using var startup = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        var init = PowerProtocol.Init(Task.Run(() => Wire.ReadLine(input, startup.Token)).WaitAsync(startup.Token).GetAwaiter().GetResult() ?? throw new EndOfStreamException());
        using var owner = new PowerOwner(init);
        // A machine-wide mutex serializes our global lid edits across roots and
        // users. Inaccessible/squatted mutexes fail closed. Never steal a lease.
        using var mutex = new Mutex(false, @"Global\Shellfox-PowerGuardian-v1");
        var held = false;
        try
        {
            try { held = mutex.WaitOne(0); }
            catch (AbandonedMutexException) { held = true; }
            if (!held) return 1;
            using var files = new PowerFiles(init.Root);
            using var journal = new PowerJournal(files);
            using var output = new PowerOutput(Console.OpenStandardOutput());
            var identity = PowerOwner.Self();
            output.Send(Wire.Object(("type", "power-ready"), ("version", 1), ("nonce", init.Nonce), ("pid", identity.Pid), ("birth", identity.Birth)));
            var commands = Commands(input);
            var controller = new PowerController(new PowerLease(new PowerNative(), journal));
            var go = false;
            var deadline = Environment.TickCount64 + 10000;
            var retryAt = 0L;
            var reportedPending = false;
            var reportedFailure = false;
            while (true)
            {
                try
                {
                    if (commands.TryRead(out var message))
                    {
                        if (message == null) controller.Stop();
                        else
                        {
                            var action = PowerProtocol.Command(message.Value, init.Nonce, controller.Phase == PowerPhase.Ready);
                            if (action == "stop") controller.Stop();
                            else
                            {
                                go = true;
                                controller.Go(action, Environment.TickCount64);
                                if (controller.Phase == PowerPhase.Active)
                                    output.Send(Wire.Object(("type", "power-active"), ("version", 1), ("nonce", init.Nonce)));
                            }
                        }
                    }
                    if (!owner.Alive || !go && Environment.TickCount64 >= deadline) controller.Stop();
                }
                catch { controller.Stop(); }
                if (controller.Phase != PowerPhase.Stopping || Environment.TickCount64 >= retryAt)
                {
                    controller.Tick(Environment.TickCount64, ownerAlive: true);
                    retryAt = controller.Phase == PowerPhase.Stopping ? Environment.TickCount64 + 30000 : 0;
                }
                if (controller.Failed && !reportedFailure)
                {
                    output.Send(Wire.Object(("type", "power-error"), ("version", 1), ("nonce", init.Nonce), ("code", "failed")));
                    reportedFailure = true;
                }
                if (controller.Phase == PowerPhase.Stopped)
                {
                    if (go) output.Send(Wire.Object(("type", "power-stopped"), ("version", 1), ("nonce", init.Nonce)));
                    else output.Send(Wire.Object(("type", "power-error"), ("version", 1), ("nonce", init.Nonce), ("code", "failed")));
                    return go && !controller.Failed ? 0 : 1;
                }
                if (controller.Phase == PowerPhase.Stopping && !reportedPending)
                {
                    output.Send(Wire.Object(("type", "power-error"), ("version", 1), ("nonce", init.Nonce), ("code", "rollback-pending")));
                    reportedPending = true;
                }
                Thread.Sleep(100);
            }
        }
        finally { if (held) mutex.ReleaseMutex(); }
    }
    private static ChannelReader<JsonElement?> Commands(Stream input)
    {
        var channel = Channel.CreateBounded<JsonElement?>(new BoundedChannelOptions(1) { SingleReader = true, SingleWriter = true });
        _ = Task.Run(async () =>
        {
            try
            {
                while (await Wire.ReadLine(input, CancellationToken.None) is { } message)
                    await channel.Writer.WriteAsync(message);
            }
            catch { /* Malformed/truncated input also requests rollback. */ }
            finally { await channel.Writer.WriteAsync(null); }
        });
        return channel.Reader;
    }
}

// A stalled/broken stdout must never delay rollback. Bounded best-effort status
// delivery; absence of a final reply is unconfirmed cleanup to the Rust owner.
internal sealed class PowerOutput : IDisposable
{
    private readonly Channel<object> queue = Channel.CreateBounded<object>(8);
    private readonly Task pump;
    internal PowerOutput(Stream output)
    {
        pump = Task.Run(async () =>
        {
            try
            {
                await foreach (var value in queue.Reader.ReadAllAsync())
                {
                    using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(2));
                    await Wire.Send(output, value, true, deadline.Token);
                }
            }
            catch { /* Rollback must finish even after the host dies. */ }
            finally { output.Dispose(); }
        });
    }
    internal void Send(object value) => queue.Writer.TryWrite(value);
    public void Dispose()
    {
        queue.Writer.TryComplete();
        try { pump.Wait(TimeSpan.FromSeconds(2)); } catch { }
    }
}
