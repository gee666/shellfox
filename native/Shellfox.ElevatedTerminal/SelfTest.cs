using System.Diagnostics;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;

namespace Shellfox.ElevatedTerminal;

internal static class SelfTest
{
    internal static async Task<int> Run(string? profilerDll = null)
    {
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        var env = Environment.GetEnvironmentVariables().Cast<System.Collections.DictionaryEntry>().Where(p => !((string)p.Key).Contains('=')).ToDictionary(p => (string)p.Key, p => (string)p.Value!);
        // Send unsanitized, mixed-case variables directly to the real native
        // boundary. An optional harmless profiler DLL proves it never loads.
        var marker = Path.GetFullPath(Path.Combine("tmp", "profiler-blocked-" + Guid.NewGuid().ToString("N") + ".txt"));
        env["cOr_ENABLE_PROFILING"] = "1"; env["COR_PROFILER"] = "{F252AD64-815F-4F97-B6EC-56808323820D}";
        env["COR_PROFILER_PATH"] = profilerDll ?? Path.GetFullPath("tmp/missing-profiler.dll");
        env["COR_PROFILER_PATH_32"] = env["COR_PROFILER_PATH"]; env["COR_PROFILER_PATH_64"] = env["COR_PROFILER_PATH"];
        env["CORECLR_ENABLE_PROFILING"] = "1"; env["CoReClR_PROFILER_PATH"] = env["COR_PROFILER_PATH"];
        env["DOTNET_STARTUP_HOOKS"] = "blocked"; env["COMPLUS_Version"] = "blocked";
        env["SHELLFOX_REVIEW_PROFILER_MARKER"] = marker; env["SHELLFOX_ENV_TEST"] = "kept";
        var launch = Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(new { type = "launch", cwd = Environment.CurrentDirectory, env, cols = 80, rows = 24 }));
        var block = ConPty.BuildEnvironment(launch.GetProperty("env"));
        foreach (var envName in env.Keys.Where(key => !ConPty.SafeEnvironmentName(key)))
            if (block.Contains(envName + "=", StringComparison.OrdinalIgnoreCase)) throw new Exception("Native CLR environment filter failed.");
        if (!block.Contains("SHELLFOX_ENV_TEST=kept\0")) throw new Exception("Normal session environment lost.");
        var apostrophes = new string('\'', 50000);
        var writeJson = "{\"type\":\"write\",\"id\":1,\"data\":\"" + apostrophes + "\"}";
        using (var roundtrip = new MemoryStream())
        {
            await Wire.Send(roundtrip, Wire.Parse(Encoding.UTF8.GetBytes(writeJson)), false, deadline.Token);
            if (roundtrip.Length != Encoding.UTF8.GetByteCount(writeJson) + 4) throw new Exception("Forwarded frame was inflated.");
            roundtrip.Position = 0;
            var frame = await Wire.ReadFrame(roundtrip, deadline.Token);
            if (frame?.GetProperty("data").GetString() != apostrophes) throw new Exception("Apostrophe frame roundtrip failed.");
        }
        using (var rejected = new MemoryStream())
        using (var tooLarge = JsonDocument.Parse(JsonSerializer.Serialize(new { type = "write", id = 1, data = new string('\x01', 50000) })))
        {
            try { await Wire.Send(rejected, tooLarge.RootElement, false, deadline.Token); throw new Exception("Accepted oversized serialized input."); }
            catch (InvalidDataException) { if (rejected.Length != 0) throw new Exception("Oversized frame was partially enqueued."); }
        }
        foreach (var invalid in new[] {
            "{\"type\":\"close\",\"type\":\"close\"}", "{\"type\":\"close\",\"command\":\"evil\"}"
        })
        {
            try { Wire.Fields(Wire.Parse(Encoding.UTF8.GetBytes(invalid)), "type"); throw new Exception("Accepted invalid fields."); }
            catch (InvalidDataException) { }
        }
        using (var oversized = new MemoryStream(new byte[] { 1, 0, 4, 0 }))
        {
            try { await Wire.ReadFrame(oversized, deadline.Token); throw new Exception("Accepted oversized frame."); }
            catch (InvalidDataException) { }
        }
        var name = "shellfox-admin-" + Guid.NewGuid().ToString("N");
        using (var server = LocalPipe.Create(name))
        using (var client = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous, System.Security.Principal.TokenImpersonationLevel.Anonymous))
        using (var self = Process.GetCurrentProcess())
        {
            var accept = server.WaitForConnectionAsync(deadline.Token);
            await client.ConnectAsync(deadline.Token); await accept;
            LocalPipe.Verify(server, self, false); LocalPipe.Verify(client, self, true);
            try { LocalPipe.OpenParent(self.Id, self.StartTime.ToUniversalTime().ToFileTimeUtc() + 1).Dispose(); throw new Exception("Accepted wrong birth."); }
            catch (UnauthorizedAccessException) { }
            try { using var collision = LocalPipe.Create(name); throw new Exception("Allowed pipe name collision."); }
            catch (System.ComponentModel.Win32Exception) { }
        }
        using var terminal = new ConPty(launch);
        var output = new StringBuilder();
        var pump = Task.Run(async () =>
        {
            var reader = new StreamReader(terminal.Output, Encoding.UTF8, false, 4096, true);
            var chars = new char[4096];
            while (true)
            {
                int count;
                try { count = await reader.ReadAsync(chars, deadline.Token); } catch (IOException) { break; }
                if (count == 0) break;
                lock (output) output.Append(chars, 0, count);
            }
        });
        terminal.Resize(Wire.Parse(Encoding.UTF8.GetBytes("{\"type\":\"resize\",\"cols\":100,\"rows\":500}")));
        foreach (var rows in new[] { 1, 501 })
        {
            try { terminal.Resize(Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(new { type = "resize", cols = 100, rows }))); throw new Exception("Accepted invalid rows."); }
            catch (InvalidDataException) { }
        }
        var ack = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        // Leave a hidden child behind the exiting root. Close must account for
        // this job member, not only the root shell PID.
        var command = "Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile -Command Start-Sleep 120'; Write-Output ('SHELLFOX_' + 'PTY_OK雪🙂'); Write-Output ('SHELLFOX_SIZE_' + $Host.UI.RawUI.WindowSize.Width); Write-Output ('SHELLFOX_ROWS_' + $Host.UI.RawUI.WindowSize.Height); exit 17\r";
        terminal.QueueInput(Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(new { type = "write", id = 1, data = command })), value => { ack.TrySetResult(); return Task.CompletedTask; });
        await ack.Task.WaitAsync(deadline.Token);
        await terminal.WaitForExit(deadline.Token);
        if (terminal.ExitCode != 17) throw new Exception($"PowerShell exit code not preserved: {terminal.ExitCode}. Captured: {output}");
        if (terminal.ActiveProcesses == 0) throw new Exception("Descendant fixture did not stay in the job.");
        await terminal.Close().WaitAsync(deadline.Token);
        if (terminal.ActiveProcesses != 0) throw new Exception("Owned descendants survived close.");
        await pump.WaitAsync(deadline.Token);
        lock (output)
        {
            if (!output.ToString().Contains("SHELLFOX_PTY_OK雪🙂")) throw new Exception("ConPTY Unicode output missing.");
            if (!output.ToString().Contains("SHELLFOX_SIZE_100") || !output.ToString().Contains("SHELLFOX_ROWS_500")) throw new Exception("ConPTY resize not reflected in the shell.");
        }
        if (File.Exists(marker)) throw new Exception("ConPTY loaded an injected CLR profiler.");
        Console.WriteLine("ConPTY Unicode I/O, 500-row resize, exit, job cleanup, peer identity, raw apostrophe framing and final CLR environment filtering passed.");
        if (profilerDll != null) Console.WriteLine("Harmless native profiler DLL did not load into the ConPTY shell.");
        return 0;
    }
}
