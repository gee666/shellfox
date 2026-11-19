using System.ComponentModel;
using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class LauncherStartupTests
{
    private const string Request = "{\"type\":\"launch\",\"cwd\":\"C:\\\\fixture\",\"env\":{\"EXAMPLE\":\"private-value\"},\"cols\":80,\"rows\":24}\n";

    [Fact]
    public async Task DenialIsDistinctFromALauncherThatWasStartedButCouldNotConnect()
    {
        var denied = await Run(_ => Task.FromException<Process>(new Win32Exception(1223, "private-value")));
        Assert.Equal("cancelled", denied[0].GetProperty("code").GetString());
        Assert.Equal("Windows administrator approval was denied or dismissed.", denied[0].GetProperty("message").GetString());
        Assert.Equal("exit", Wire.Type(denied[1])); // No launch was dispatched.

        var failed = await Run(_ =>
        {
            // No elevation: just a regular process simulating an early helper exit.
            var info = new ProcessStartInfo(Path.Combine(Environment.SystemDirectory, "cmd.exe"))
            {
                UseShellExecute = false, CreateNoWindow = true,
                Arguments = $"/d /c exit /b {(int)StartupStage.OpenLauncher}",
            };
            return Task.FromResult(Process.Start(info) ?? throw new InvalidOperationException());
        });
        Assert.Equal("failed", failed[0].GetProperty("code").GetString());
        Assert.Equal("Elevated helper started but failed at opening the launcher process.", failed[0].GetProperty("message").GetString());
        Assert.Equal("exit", Wire.Type(failed[1]));
    }

    [Fact]
    public async Task InteractiveHandshakeForwardsLaunchOnlyToTheAuthenticatedPeer()
    {
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        Task? peer = null;
        var frames = await Run(info =>
        {
            var args = info.Arguments.Split(' ');
            Assert.Equal("serve", args[0]);
            Assert.Equal("runas", info.Verb);
            peer = SimulatePeer(args[1], deadline.Token);
            return Task.FromResult(Process.GetCurrentProcess());
        });
        await Assert.IsAssignableFrom<Task>(peer).WaitAsync(deadline.Token);
        Assert.Collection(frames, ready => Assert.Equal("ready", Wire.Type(ready)), exit => Assert.Equal("exit", Wire.Type(exit)));
    }

    private static async Task SimulatePeer(string name, CancellationToken cancel)
    {
        using var client = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous, TokenImpersonationLevel.Anonymous);
        await client.ConnectAsync(cancel);
        using var parent = Process.GetCurrentProcess();
        LocalPipe.Verify(client, parent, true);
        var launch = await Wire.ReadFrame(client, cancel) ?? throw new EndOfStreamException();
        Assert.Equal(Request.TrimEnd(), launch.GetRawText());
        await Wire.Send(client, Wire.Object(("type", "ready"), ("pid", 42), ("birth", "123")), false, cancel);
        await Wire.Send(client, Wire.Object(("type", "exit"), ("exitCode", 0)), false, cancel);
    }

    private static async Task<List<JsonElement>> Run(Func<ProcessStartInfo, Task<Process>> start)
    {
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        using var input = new OpenInput();
        using var output = new MemoryStream();
        using var parent = Process.GetCurrentProcess();
        await Launcher.RunPipe(input, output, parent, deadline, isolated: false, startElevated: start)
            .WaitAsync(TimeSpan.FromSeconds(8));
        return Encoding.UTF8.GetString(output.ToArray()).Split('\n', StringSplitOptions.RemoveEmptyEntries)
            .Select(row => Wire.Parse(Encoding.UTF8.GetBytes(row))).ToList();
    }

    private sealed class OpenInput() : MemoryStream(Encoding.UTF8.GetBytes(Request), writable: false)
    {
        public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancel = default)
        {
            if (Position < Length) return Read(buffer.Span);
            await Task.Delay(Timeout.Infinite, cancel);
            return 0;
        }
    }
}
