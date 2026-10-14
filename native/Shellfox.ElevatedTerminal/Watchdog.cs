namespace Shellfox.ElevatedTerminal;

// Cancellation tokens cannot interrupt a synchronous native call or a blocked
// console WriteFile. Process death closes every non-inheritable job handle.
// Watchdog death is NOT a cleanup acknowledgement to the host.
internal static class Watchdog
{
    internal static Timer Start(int milliseconds) => new(_ => Environment.Exit(1), null, milliseconds, Timeout.Infinite);
    internal static async Task Close(ConPty terminal)
    {
        // Arm before calling Close: acquiring its resize/close lock can itself
        // block before a Task exists for WaitAsync to time out.
        using var kill = Start(8000);
        try { await terminal.Close().WaitAsync(TimeSpan.FromSeconds(7)); }
        catch { Environment.Exit(1); throw; }
    }
}
