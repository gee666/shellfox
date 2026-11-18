using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class PowerLeaseTests
{
    [Fact]
    public void StartsOffAndCannotEnableWithoutExplicitGo()
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal();
        var controller = new PowerController(new(native, journal));
        controller.Tick(100000, true);
        Assert.Equal(PowerPhase.Ready, controller.Phase);
        Assert.Empty(native.Calls); Assert.Empty(journal.Saved);
        Assert.Throws<InvalidDataException>(() => controller.Go("true", 0));
        Assert.Empty(native.Calls);
    }
    [Fact]
    public void EnableRequiresDurableIntentAndStopRestoresBothIndicesBeforeReleasingRequests()
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal(); native.Journal = journal;
        var controller = new PowerController(new(native, journal));
        controller.Go("enable", 0);
        Assert.Equal(PowerPhase.Active, controller.Phase);
        Assert.Equal(1, native.HeldRequests); Assert.Equal(1, native.Pulses);
        Assert.Equal(0u, native.Values[(native.Scheme, true)]); Assert.Equal(0u, native.Values[(native.Scheme, false)]);
        Assert.False(journal.Saved[0].AcOwned); Assert.False(journal.Saved[0].DcOwned);
        controller.Tick(29999, true); Assert.Equal(1, native.Pulses);
        controller.Tick(30000, true); Assert.Equal(2, native.Pulses);
        controller.Stop(); controller.Tick(30001, true);
        Assert.Equal(PowerPhase.Stopped, controller.Phase);
        Assert.Equal(1u, native.Values[(native.Scheme, true)]); Assert.Equal(2u, native.Values[(native.Scheme, false)]);
        Assert.Equal(0, native.HeldRequests); Assert.Null(journal.Current);
    }
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void HostDeathOrClosedInputRollsBackWithoutAnyProcessKill(bool eof)
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal();
        var controller = new PowerController(new(native, journal));
        controller.Go("enable", 0);
        if (eof) controller.Stop();
        controller.Tick(1, eof);
        Assert.Equal(PowerPhase.Stopped, controller.Phase);
        Assert.Null(journal.Current); Assert.Equal(0, native.HeldRequests);
    }
    [Fact]
    public void LaterUserEditAndSchemeSelectionAreNeverUnconditionallyOverwritten()
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal();
        var lease = new PowerLease(native, journal); lease.Enable();
        native.Values[(native.Scheme, true)] = 3;
        var laterScheme = Guid.NewGuid(); native.Active = laterScheme;
        var refreshes = native.Refreshes;
        lease.Restore();
        Assert.Equal(laterScheme, native.Active); Assert.Equal(refreshes, native.Refreshes);
        Assert.Equal(3u, native.Values[(native.Scheme, true)]); Assert.Equal(2u, native.Values[(native.Scheme, false)]);
        Assert.DoesNotContain("write-ac-1", native.Calls);
    }
    [Fact]
    public void ExistingDoNothingSettingsAreNotClaimedOrWritten()
    {
        var native = new PowerFakeNative(); native.Values[(native.Scheme, true)] = 0; native.Values[(native.Scheme, false)] = 0;
        var journal = new PowerMemoryJournal(); var lease = new PowerLease(native, journal);
        lease.Enable(); lease.Restore();
        Assert.DoesNotContain(native.Calls, value => value.StartsWith("write-"));
        Assert.Equal(0, native.Refreshes);
    }
    [Fact]
    public void FailedRollbackRetainsJournalAndRequestsAndNeverContinuesMouseInput()
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal(); native.Journal = journal;
        var controller = new PowerController(new(native, journal)); controller.Go("enable", 0);
        native.Fault = op => { if (op == "write-dc-2") throw new IOException("denied"); };
        controller.Stop(); controller.Tick(1, true);
        Assert.Equal(PowerPhase.Stopping, controller.Phase); Assert.True(controller.Failed);
        Assert.NotNull(journal.Current); Assert.False(journal.Current.AcOwned); Assert.True(journal.Current.DcOwned);
        Assert.Equal(1, native.HeldRequests);
        controller.Tick(60000, false); Assert.Equal(1, native.Pulses);
        native.Fault = null; controller.Tick(90000, false);
        Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.Equal(0, native.HeldRequests); Assert.Null(journal.Current);
    }
    [Fact]
    public void RecoveryOnlyNeverAcquiresRequestsOrMovesMouse()
    {
        var native = new PowerFakeNative(); native.Values[(native.Scheme, true)] = 0;
        var journal = new PowerMemoryJournal(new(native.Scheme, 1, 2, true, true, true)); native.Journal = journal;
        var controller = new PowerController(new(native, journal)); controller.Go("recover", 0);
        Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.Null(journal.Current);
        Assert.Equal(1u, native.Values[(native.Scheme, true)]);
        Assert.DoesNotContain("requests", native.Calls); Assert.Equal(0, native.Pulses);
    }
    [Fact]
    public void UserEditDuringCaptureBeforeWriteIsNotClaimedForRollback()
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal();
        native.Fault = op => { if (op == "requests") native.Values[(native.Scheme, true)] = 0; };
        var controller = new PowerController(new(native, journal)); controller.Go("enable", 0); controller.Tick(1, true);
        Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.True(controller.Failed);
        Assert.Equal(0u, native.Values[(native.Scheme, true)]); Assert.DoesNotContain("write-ac-1", native.Calls);
    }
    [Fact]
    public void NoGlobalWritesOccurWhenFirstJournalFlushFails()
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal { BeforeSave = () => throw new IOException("disk full") };
        var controller = new PowerController(new(native, journal)); controller.Go("enable", 0);
        Assert.DoesNotContain("requests", native.Calls); Assert.DoesNotContain(native.Calls, value => value.StartsWith("write-"));
    }
    [Theory]
    [InlineData("requests")]
    [InlineData("write-ac-0")]
    [InlineData("write-dc-0")]
    [InlineData("refresh")]
    [InlineData("mouse")]
    public void FailedStartupRollsBackWhatItOwnsAndDoesNotRetryEnable(string failedOperation)
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal(); native.Journal = journal;
        var once = true;
        native.Fault = op => { if (once && op == failedOperation) { once = false; throw new IOException("injected"); } };
        var controller = new PowerController(new(native, journal)); controller.Go("enable", 0); controller.Tick(1, true);
        Assert.Equal(PowerPhase.Stopped, controller.Phase); Assert.True(controller.Failed);
        Assert.Equal(1u, native.Values[(native.Scheme, true)]); Assert.Equal(2u, native.Values[(native.Scheme, false)]);
        Assert.Null(journal.Current); Assert.Equal(0, native.HeldRequests);
    }
    [Fact]
    public void JournalDeleteAndSchemeRefreshFailuresRemainRetryable()
    {
        var native = new PowerFakeNative(); var journal = new PowerMemoryJournal(); var lease = new PowerLease(native, journal);
        lease.Enable();
        native.Fault = op => { if (op == "refresh") throw new IOException("refresh failed"); };
        Assert.Throws<IOException>(lease.Restore); Assert.NotNull(journal.Current); Assert.True(journal.Current.RefreshPending);
        native.Fault = null; journal.FailClear = true;
        Assert.Throws<IOException>(lease.Restore); Assert.NotNull(journal.Current); Assert.Equal(1, native.HeldRequests);
        journal.FailClear = false; lease.Restore(); Assert.Null(journal.Current); Assert.Equal(0, native.HeldRequests);
    }
}
