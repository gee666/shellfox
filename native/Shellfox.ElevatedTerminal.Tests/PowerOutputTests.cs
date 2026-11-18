using System.Diagnostics;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class PowerOutputTests
{
    [Fact]
    public void BlockedStatusPipeCannotPreventRollbackAndQueueIsBounded()
    {
        var stream = new BlockedStream();
        var output = new PowerOutput(stream);
        try
        {
            for (var i = 0; i < 1000; i++) output.Send(Wire.Object(("type", "power-error"), ("version", 1), ("nonce", "fixture"), ("code", "failed")));
            var native = new PowerFakeNative(); var journal = new PowerMemoryJournal();
            var controller = new PowerController(new(native, journal)); controller.Go("enable", 0);
            var time = Stopwatch.StartNew(); controller.Stop(); controller.Tick(1, false);
            Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.Null(journal.Current); Assert.Equal(0, native.HeldRequests);
            Assert.True(time.Elapsed < TimeSpan.FromSeconds(1));
        }
        finally { stream.Release(); output.Dispose(); }
    }
    [Fact]
    public void BrokenStatusPipeDoesNotTurnRollbackIntoAPowerOperationFailure()
    {
        using var output = new PowerOutput(new BrokenStream());
        output.Send(Wire.Object(("type", "power-active"), ("version", 1), ("nonce", "fixture")));
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal();
        var controller = new PowerController(new(native, journal)); controller.Go("enable", 0);
        controller.Stop(); controller.Tick(1, false);
        Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.False(controller.Failed);
    }
    private sealed class BlockedStream : MemoryStream
    {
        private readonly TaskCompletionSource released = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public override async ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default) => await released.Task.WaitAsync(cancellationToken);
        internal void Release() => released.TrySetResult();
    }
    private sealed class BrokenStream : MemoryStream
    {
        public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default) => ValueTask.FromException(new IOException("closed fixture pipe"));
    }
}
