using System.Text;
using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

// No Process/UAC creation. The only exit proof is the fake original owner's
// explicit signal, just as production requires the original process HANDLE.
public sealed class IsolatedPeerExitTests
{
    private static JsonElement Exit() => Wire.Parse(Encoding.UTF8.GetBytes("{\"type\":\"exit\",\"exitCode\":17}"));
    [Fact]
    public async Task PrivateEofReleasesPeerBeforeWaitingWhileHostInputStaysOpen()
    {
        using var hostInput = new MemoryStream();
        using var channel = new EofStream();
        var owner = new FakePeer();
        var messages = new List<JsonElement>();
        var operation = IsolatedPeerExit.Forward(channel, owner, Exit(), (value, token) =>
        {
            Assert.True(owner.Signaled); messages.Add(Wire.Parse(Wire.Bytes(value))); return Task.CompletedTask;
        });
        await channel.Eof.Task.WaitAsync(TimeSpan.FromSeconds(3));
        await owner.WaitCalled.Task.WaitAsync(TimeSpan.FromSeconds(3));
        Assert.False(operation.IsCompleted);
        Assert.Empty(messages);
        Assert.True(hostInput.CanRead);
        owner.Signal();
        Assert.True(await operation.WaitAsync(TimeSpan.FromSeconds(3)));
        Assert.Single(messages); Assert.Equal("exit", Wire.Type(messages[0]));
        Assert.Equal(17, messages[0].GetProperty("exitCode").GetInt32());
        Assert.Equal(1, owner.Waits);
    }
    [Fact]
    public async Task PeerThatCannotExitUntilItsReaderSeesEofDoesNotDeadlock()
    {
        using var channel = new EofStream();
        var owner = new FakePeer();
        var reader = Task.Run(async () => { await channel.Eof.Task; owner.Signal(); });
        var acknowledged = false;
        Assert.True(await IsolatedPeerExit.Forward(channel, owner, Exit(), (value, token) =>
        {
            Assert.True(owner.Signaled); acknowledged = true; return Task.CompletedTask;
        }).WaitAsync(TimeSpan.FromSeconds(3)));
        await reader; Assert.True(acknowledged);
    }
    [Fact]
    public async Task TimeoutReportsUnconfirmedButRetainsOwnerUntilLateOriginalExit()
    {
        using var channel = new EofStream();
        var owner = new FakePeer();
        var unconfirmed = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var frames = new List<JsonElement>();
        async Task<bool> OwnedOperation()
        {
            try
            {
                return await IsolatedPeerExit.Forward(channel, owner, Exit(), (value, token) =>
                {
                    var frame = Wire.Parse(Wire.Bytes(value)); frames.Add(frame);
                    if (Wire.Type(frame) == "error") unconfirmed.SetResult();
                    else Assert.True(owner.Signaled);
                    return Task.CompletedTask;
                }, TimeSpan.FromMilliseconds(10));
            }
            finally { owner.Disposed = true; }
        }
        var operation = OwnedOperation();
        await unconfirmed.Task.WaitAsync(TimeSpan.FromSeconds(3));
        Assert.False(operation.IsCompleted); Assert.False(owner.Disposed);
        Assert.Single(frames); Wire.Fields(frames[0], "type", "code", "message");
        Assert.Equal("failed", frames[0].GetProperty("code").GetString());
        owner.Signal(); Assert.True(await operation.WaitAsync(TimeSpan.FromSeconds(3)));
        Assert.True(owner.Disposed); Assert.Equal(new[] { "error", "exit" }, frames.Select(Wire.Type));
        Assert.Equal(1, owner.Waits); // A timeout never reopens/replaces the owner.
    }
    [Fact]
    public async Task HostCancellationCannotCancelOriginalWaitOrStrongLateReply()
    {
        using var channel = new EofStream(); using var host = new CancellationTokenSource();
        var owner = new FakePeer(); var final = false;
        var operation = IsolatedPeerExit.Forward(channel, owner, Exit(), (value, token) =>
        {
            Assert.False(token.IsCancellationRequested); final = true; return Task.CompletedTask;
        });
        await owner.WaitCalled.Task.WaitAsync(TimeSpan.FromSeconds(3));
        host.Cancel(); Assert.False(operation.IsCompleted);
        owner.Signal(); Assert.True(await operation.WaitAsync(TimeSpan.FromSeconds(3))); Assert.True(final);
    }
    [Fact]
    public async Task BrokenOutputDoesNotDisposePeerEarlyOrFabricateExit()
    {
        using var channel = new EofStream(); var owner = new FakePeer();
        var attempted = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var operation = IsolatedPeerExit.Forward(channel, owner, Exit(), (value, token) =>
        {
            attempted.TrySetResult(); throw new IOException("closed owned fixture output");
        }, TimeSpan.FromMilliseconds(10));
        await attempted.Task.WaitAsync(TimeSpan.FromSeconds(3)); Assert.False(operation.IsCompleted);
        owner.Signal(); Assert.False(await operation.WaitAsync(TimeSpan.FromSeconds(3)));
    }
    [Fact]
    public async Task FailedOriginalWaitIsUnconfirmedAndRetriesTheSameAdapter()
    {
        using var channel = new EofStream(); var owner = new FakePeer { FailFirst = true };
        var retried = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        owner.OnWait = count => { if (count == 2) retried.TrySetResult(); };
        var errors = 0;
        var operation = IsolatedPeerExit.Forward(channel, owner, Exit(), (value, token) =>
        {
            if (Wire.Type(Wire.Parse(Wire.Bytes(value))) == "error") errors++;
            return Task.CompletedTask;
        }, retry: TimeSpan.Zero);
        await retried.Task.WaitAsync(TimeSpan.FromSeconds(3));
        Assert.False(operation.IsCompleted); Assert.Equal(1, errors);
        owner.Signal(); Assert.True(await operation.WaitAsync(TimeSpan.FromSeconds(3))); Assert.Equal(2, owner.Waits);
    }
    [Fact]
    public async Task InvalidExitCannotClosePipeWaitOrAcknowledgeCleanup()
    {
        using var channel = new EofStream(); var owner = new FakePeer();
        var invalid = Wire.Parse(Encoding.UTF8.GetBytes("{\"type\":\"exit\",\"exitCode\":1,\"extra\":true}"));
        await Assert.ThrowsAsync<InvalidDataException>(() => IsolatedPeerExit.Forward(channel, owner, invalid, (_, _) => throw new Exception("must not send")));
        Assert.False(channel.Eof.Task.IsCompleted); Assert.Equal(0, owner.Waits);
    }
    [Fact]
    public async Task LateStartupResultRemainsOwnedUntilOriginalPeerExit()
    {
        using var channel = new EofStream(); var owner = new FakePeer();
        var started = new TaskCompletionSource<IOriginalPeerExit?>(TaskCreationOptions.RunContinuationsAsynchronously);
        var pending = new PendingOriginalPeerExit(started.Task);
        var unconfirmed = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var operation = IsolatedPeerExit.Confirm(channel, pending, (value, token) =>
        {
            unconfirmed.TrySetResult(); return Task.CompletedTask;
        }, TimeSpan.FromMilliseconds(10));
        await unconfirmed.Task.WaitAsync(TimeSpan.FromSeconds(3)); Assert.False(operation.IsCompleted);
        started.SetResult(owner);
        await owner.WaitCalled.Task.WaitAsync(TimeSpan.FromSeconds(3)); Assert.False(operation.IsCompleted);
        owner.Signal(); await operation.WaitAsync(TimeSpan.FromSeconds(3));
    }
    [Fact]
    public async Task ExplicitlyProvedNoPeerDoesNotWaitForANumericPid()
    {
        using var channel = new EofStream();
        var pending = new PendingOriginalPeerExit(Task.FromResult<IOriginalPeerExit?>(null));
        await IsolatedPeerExit.Confirm(channel, pending, (_, _) => throw new Exception("must not report failure")).WaitAsync(TimeSpan.FromSeconds(3));
        Assert.True(channel.Eof.Task.IsCompleted);
    }
    [Fact]
    public void FinalPhaseDisarmsBothQueuedAbortAndLaterCloseDeadlines()
    {
        var phase = new IsolatedExitPhase(); var calls = new List<string>();
        phase.UnlessWaiting(() => calls.Add("startup-abort-scheduled"));
        phase.Begin(() => calls.Add("disarmed"));
        phase.UnlessWaiting(() => calls.Add("queued-abort-fired"));
        phase.UnlessWaiting(() => calls.Add("late-close-deadline"));
        Assert.True(phase.Waiting); Assert.Equal(new[] { "startup-abort-scheduled", "disarmed" }, calls);
    }
    private sealed class FakePeer : IOriginalPeerExit
    {
        private readonly TaskCompletionSource exited = new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal readonly TaskCompletionSource WaitCalled = new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal bool Signaled, Disposed, FailFirst;
        internal int Waits;
        internal Action<int>? OnWait;
        public Task WaitForExit()
        {
            Waits++; WaitCalled.TrySetResult(); OnWait?.Invoke(Waits);
            if (FailFirst && Waits == 1) return Task.FromException(new IOException("injected original wait failure"));
            return exited.Task;
        }
        internal void Signal() { Signaled = true; exited.TrySetResult(); }
    }
    private sealed class EofStream : MemoryStream
    {
        internal readonly TaskCompletionSource Eof = new(TaskCreationOptions.RunContinuationsAsynchronously);
        protected override void Dispose(bool disposing) { if (disposing) Eof.TrySetResult(); base.Dispose(disposing); }
    }
}
