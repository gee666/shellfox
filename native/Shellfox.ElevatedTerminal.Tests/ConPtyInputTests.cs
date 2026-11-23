using System.Text;
using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class ConPtyInputTests
{
    [Fact]
    public async Task TerminalNulSurvivesFramingAndUtf8ByteLength()
    {
        using var stream = new MemoryStream();
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        var message = Wire.Object(("type", "write"), ("id", 1), ("data", "A\0B"));
        await Wire.Send(stream, message, false, deadline.Token);
        stream.Position = 0;
        var frame = (await Wire.ReadFrame(stream, deadline.Token))!.Value;
        Wire.Fields(frame, "type", "id", "data");
        Assert.Equal(new byte[] { 65, 0, 66 }, Wire.Utf8.GetBytes(frame.GetProperty("data").GetString()!));
    }

    [Fact]
    public async Task OwnedNoProfileConPtyReadsNulAndFollowingByteInRawMode()
    {
        // Real ConPTY and the active QueueInput path, not a simulated host.
        // All shell state and compiler scratch remain in this private test root.
        var project = new DirectoryInfo(Environment.CurrentDirectory);
        while (project != null && !File.Exists(Path.Combine(project.FullName, "native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj"))) project = project.Parent;
        if (project == null) throw new InvalidOperationException("Run tests inside the worktree.");
        var root = Path.Combine(project.FullName, "tmp", "nul-conpty-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try { await Probe(root); }
        finally { Directory.Delete(root, true); }
    }

    private static async Task Probe(string root)
    {
        // A minimal environment rooted in the test directory. No profile, and
        // PSReadLine must not save the test's commands to the account history.
        var system = Environment.GetFolderPath(Environment.SpecialFolder.System);
        var windows = Path.GetDirectoryName(system)!;
        var env = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["SystemRoot"] = windows, ["WINDIR"] = windows, ["COMSPEC"] = Path.Combine(system, "cmd.exe"),
            ["PATH"] = system, ["PSModulePath"] = Path.Combine(system, "WindowsPowerShell/v1.0/Modules"),
            ["PATHEXT"] = ".COM;.EXE;.BAT;.CMD", ["TERM"] = "xterm-256color",
            ["USERPROFILE"] = root, ["APPDATA"] = root, ["LOCALAPPDATA"] = root, ["TEMP"] = root, ["TMP"] = root,
        };
        var launch = Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(new { type = "launch", cwd = root, env, cols = 80, rows = 24 }));
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(45));
        using var terminal = new ConPty(launch, " -NoProfile -NoExit -Command \"Import-Module PSReadLine -ErrorAction SilentlyContinue; Set-PSReadLineOption -HistorySaveStyle SaveNothing -ErrorAction SilentlyContinue\"");
        var ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var output = new StringBuilder();
        var pump = Task.Run(async () =>
        {
            using var reader = new StreamReader(terminal.Output, Encoding.UTF8, false, 4096, true);
            var chars = new char[4096];
            while (true)
            {
                int count;
                try { count = await reader.ReadAsync(chars); } catch (IOException) { break; }
                if (count == 0) break;
                lock (output)
                {
                    output.Append(chars, 0, count);
                    if (output.Length > 65536) throw new InvalidDataException("Oversized raw-input probe output.");
                    if (output.ToString().Contains("SHELLFOX_RAW_READY")) ready.TrySetResult();
                }
            }
        });
        try
        {
            // SetConsoleMode disables cooked/echo/processed input and enables VT input.
            // ReadByte reads explicit byte lengths, so A-NUL-B cannot pass by truncation.
            const string source = """
                using System;
                using System.Runtime.InteropServices;
                public static class ShellfoxRawInputProbe {
                  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GetStdHandle(int n);
                  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetConsoleMode(IntPtr h, out uint m);
                  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetConsoleMode(IntPtr h, uint m);
                  public static void Run() {
                    var h = GetStdHandle(-10); uint old;
                    if (!GetConsoleMode(h, out old) || !SetConsoleMode(h, (old & ~7u) | 0x200u)) throw new Exception("raw input mode");
                    try {
                      Console.WriteLine("SHELLFOX_" + "RAW_READY");
                      var input = Console.OpenStandardInput();
                      int a = input.ReadByte(), b = input.ReadByte(), c = input.ReadByte();
                      Console.WriteLine("SHELLFOX_" + "RAW_RESULT_" + a + "," + b + "," + c);
                    } finally { SetConsoleMode(h, old); }
                  }
                }
                """;
            // The echoed command never contains either full expected output marker.
            var command = "Add-Type -TypeDefinition '" + source.Replace("\r", "").Replace("\n", " ") + "'; [ShellfoxRawInputProbe]::Run(); exit 0\r";
            terminal.QueueInput(Write(1, command), _ => Task.CompletedTask);
            await ready.Task.WaitAsync(deadline.Token);
            var ack = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            terminal.QueueInput(Write(2, "A\0B"), _ => { ack.TrySetResult(); return Task.CompletedTask; });
            await ack.Task.WaitAsync(deadline.Token);
            await terminal.WaitForExit(deadline.Token);
            Assert.Equal(0, terminal.ExitCode);
        }
        finally
        {
            await terminal.Close().WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal(0u, terminal.ActiveProcesses);
            await pump.WaitAsync(TimeSpan.FromSeconds(10));
        }
        Assert.Contains("SHELLFOX_RAW_RESULT_65,0,66", output.ToString());
    }
    private static JsonElement Write(long id, string data) =>
        Wire.Parse(Wire.Bytes(Wire.Object(("type", "write"), ("id", id), ("data", data))));
}
