using System.Diagnostics;
using System.IO.Pipes;
using System.Text;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class IsolatedWireTests
{
    [Fact]
    public async Task AuthenticatedPrivatePipeKeepsIsolatedLaunchAndExitFramesUnchanged()
    {
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        var name = "shellfox-admin-" + Guid.NewGuid().ToString("N");
        using var server = LocalPipe.Create(name);
        using var client = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous, System.Security.Principal.TokenImpersonationLevel.Anonymous);
        using var self = Process.GetCurrentProcess();
        var accept = server.WaitForConnectionAsync(deadline.Token);
        await client.ConnectAsync(deadline.Token);
        await accept;
        LocalPipe.Verify(server, self, false);
        LocalPipe.Verify(client, self, true);
        using var parent = LocalPipe.OpenParent(self.Id, self.StartTime.ToUniversalTime().ToFileTimeUtc());
        Assert.Throws<UnauthorizedAccessException>(() => LocalPipe.OpenParent(self.Id, self.StartTime.ToUniversalTime().ToFileTimeUtc() + 1));
        // Fake peer only. No helper dispatch or process creation.
        var launch = Wire.Parse(Encoding.UTF8.GetBytes("{\"type\":\"launch-isolated\",\"isolationRoot\":\"C:\\\\owned-fixture\",\"cwd\":\"C:\\\\owned-fixture\",\"env\":{},\"cols\":80,\"rows\":24}"));
        await Wire.Send(server, launch, false, deadline.Token);
        var received = await Wire.ReadFrame(client, deadline.Token);
        Assert.Equal(launch.GetRawText(), received!.Value.GetRawText());
        await Wire.Send(server, Wire.Object(("type", "close")), false, deadline.Token);
        var close = await Wire.ReadFrame(client, deadline.Token);
        Wire.Fields(close!.Value, "type");
        Assert.Equal("close", Wire.Type(close.Value));
        await Wire.Send(client, Wire.Object(("type", "exit"), ("exitCode", 17)), false, deadline.Token);
        var exit = await Wire.ReadFrame(server, deadline.Token);
        Wire.Fields(exit!.Value, "type", "exitCode");
        Assert.Equal(17, exit.Value.GetProperty("exitCode").GetInt32());
        using var host = new MemoryStream();
        await Wire.Send(host, exit.Value, true, deadline.Token);
        host.Position = 0;
        Assert.Equal(exit.Value.GetRawText(), (await Wire.ReadLine(host, deadline.Token))!.Value.GetRawText());
    }
    [Fact]
    public async Task OversizedAndTruncatedIsolatedFramesAreRejectedBeforeDispatch()
    {
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        using var oversized = new MemoryStream(new byte[] { 1, 0, 4, 0 });
        await Assert.ThrowsAsync<InvalidDataException>(() => Wire.ReadFrame(oversized, deadline.Token));
        using var truncated = new MemoryStream(new byte[] { 20, 0, 0, 0, (byte)'{' });
        await Assert.ThrowsAsync<EndOfStreamException>(() => Wire.ReadFrame(truncated, deadline.Token));
        using var line = new MemoryStream(Encoding.UTF8.GetBytes("{\"type\":\"launch-isolated\"}"));
        await Assert.ThrowsAsync<EndOfStreamException>(() => Wire.ReadLine(line, deadline.Token));
    }
}
