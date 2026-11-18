using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

internal sealed class PowerMemoryJournal : IPowerJournal
{
    public PowerSnapshot? Current { get; private set; }
    internal readonly List<PowerSnapshot> Saved = new();
    internal Action? BeforeSave;
    internal bool FailClear;
    internal PowerMemoryJournal(PowerSnapshot? state = null) => Current = state;
    public void Save(PowerSnapshot value)
    {
        BeforeSave?.Invoke(); Saved.Add(value); Current = value;
    }
    public void Clear()
    {
        if (FailClear) throw new IOException("Injected clear failure.");
        Current = null;
    }
}
internal sealed class PowerFakeNative : IPowerNative
{
    internal readonly Guid Scheme = Guid.NewGuid();
    internal Guid Active;
    internal readonly Dictionary<(Guid, bool), uint> Values = new();
    internal readonly List<string> Calls = new();
    internal int HeldRequests, Pulses, Refreshes;
    internal Action<string>? Fault;
    internal IPowerJournal? Journal;
    internal PowerFakeNative()
    {
        Active = Scheme;
        Values[(Scheme, true)] = 1; Values[(Scheme, false)] = 2;
    }
    private void Call(string operation) { Calls.Add(operation); Fault?.Invoke(operation); }
    public Guid ActiveScheme() { Call("active"); return Active; }
    public uint ReadLid(Guid scheme, bool ac) { Call(ac ? "read-ac" : "read-dc"); return Values[(scheme, ac)]; }
    public void WriteLid(Guid scheme, bool ac, uint value)
    {
        if (Journal != null)
        {
            Assert.NotNull(Journal.Current);
            Assert.True(Journal.Current.RefreshPending);
            Assert.True(ac ? Journal.Current.AcOwned : Journal.Current.DcOwned);
            Assert.Equal(scheme, Journal.Current.Scheme);
        }
        Call((ac ? "write-ac-" : "write-dc-") + value);
        Values[(scheme, ac)] = value;
    }
    public void RefreshIfActive(Guid scheme)
    {
        Call("refresh");
        if (scheme == Active) Refreshes++;
    }
    public IDisposable Requests()
    {
        Call("requests"); HeldRequests++; return new Request(this);
    }
    public void MousePulse() { Call("mouse"); Pulses++; }
    private sealed class Request(PowerFakeNative parent) : IDisposable
    {
        private bool closed;
        public void Dispose() { if (!closed) { closed = true; parent.HeldRequests--; } }
    }
}
