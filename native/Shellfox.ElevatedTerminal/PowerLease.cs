namespace Shellfox.ElevatedTerminal;

internal sealed record PowerSnapshot(Guid Scheme, uint Ac, uint Dc, bool AcOwned, bool DcOwned, bool RefreshPending);
internal interface IPowerJournal
{
    PowerSnapshot? Current { get; }
    void Save(PowerSnapshot value);
    void Clear();
}

// Every global write has a durable intent first. Recovery is idempotent even
// when the process dies between a Windows write and the following journal save.
internal sealed class PowerLease(IPowerNative native, IPowerJournal journal)
{
    private IDisposable? requests;
    internal void Enable()
    {
        if (journal.Current != null || requests != null) throw new InvalidOperationException("Recovery required.");
        var scheme = native.ActiveScheme();
        var ac = native.ReadLid(scheme, true);
        var dc = native.ReadLid(scheme, false);
        if (ac > 3 || dc > 3) throw new InvalidDataException("Unsupported lid action.");
        journal.Save(new(scheme, ac, dc, false, false, false));
        requests = native.Requests();
        foreach (var isAc in new[] { true, false })
        {
            var state = journal.Current!;
            if ((isAc ? ac : dc) == 0) continue;
            if (native.ActiveScheme() != scheme) throw new InvalidOperationException("Power plan changed.");
            // Do not overwrite a user edit made after our original read.
            if (native.ReadLid(scheme, isAc) != (isAc ? ac : dc)) throw new InvalidOperationException("Lid setting changed.");
            journal.Save(isAc ? state with { AcOwned = true, RefreshPending = true } : state with { DcOwned = true, RefreshPending = true });
            native.WriteLid(scheme, isAc, 0);
        }
        if (journal.Current!.RefreshPending) native.RefreshIfActive(scheme);
        if (native.ActiveScheme() != scheme || native.ReadLid(scheme, true) != 0 || native.ReadLid(scheme, false) != 0)
            throw new InvalidOperationException("Power plan changed.");
        journal.Save(journal.Current! with { RefreshPending = false });
        native.MousePulse();
    }
    internal void Pulse() => native.MousePulse();
    internal void Restore()
    {
        // Keep requests alive until rollback succeeds. Do not keep moving the
        // mouse while rollback is pending, and never discard a failed journal.
        foreach (var ac in new[] { true, false })
        {
            var state = journal.Current;
            if (state == null || !(ac ? state.AcOwned : state.DcOwned)) continue;
            var value = native.ReadLid(state.Scheme, ac);
            if (value == 0)
            {
                journal.Save(state with { RefreshPending = true });
                native.WriteLid(state.Scheme, ac, ac ? state.Ac : state.Dc);
            }
            // A different value belongs to a later edit. Leave it alone. The
            // original value also means an interrupted restore already worked.
            state = journal.Current!;
            journal.Save(ac ? state with { AcOwned = false } : state with { DcOwned = false });
        }
        if (journal.Current is { } pending)
        {
            if (pending.RefreshPending) native.RefreshIfActive(pending.Scheme);
            journal.Save(pending with { RefreshPending = false });
            journal.Clear();
        }
        requests?.Dispose();
        requests = null;
    }
}

internal enum PowerPhase { Ready, Active, Stopping, Stopped }
internal sealed class PowerController(PowerLease lease)
{
    internal PowerPhase Phase { get; private set; } = PowerPhase.Ready;
    internal bool Failed { get; private set; }
    private long pulseAt;
    internal void Go(string action, long now)
    {
        if (Phase != PowerPhase.Ready || action is not ("enable" or "recover")) throw new InvalidDataException("Unexpected GO.");
        Phase = PowerPhase.Stopping;
        try
        {
            lease.Restore();
            if (action == "enable")
            {
                lease.Enable();
                Phase = PowerPhase.Active;
                pulseAt = now + 30000;
            }
            else Phase = PowerPhase.Stopped;
        }
        catch { Failed = true; } // Tick retries rollback, never retries enabling.
    }
    internal void Stop()
    {
        // Before GO there are no owned mutations, including stale recovery.
        Phase = Phase == PowerPhase.Ready ? PowerPhase.Stopped : PowerPhase.Stopping;
    }
    internal void Tick(long now, bool ownerAlive)
    {
        if (!ownerAlive && Phase is not (PowerPhase.Stopping or PowerPhase.Stopped)) Stop();
        if (Phase == PowerPhase.Active && now >= pulseAt)
        {
            try { lease.Pulse(); pulseAt = now + 30000; }
            catch { Failed = true; Phase = PowerPhase.Stopping; }
        }
        if (Phase != PowerPhase.Stopping) return;
        try { lease.Restore(); Phase = PowerPhase.Stopped; }
        catch { Failed = true; }
    }
}
