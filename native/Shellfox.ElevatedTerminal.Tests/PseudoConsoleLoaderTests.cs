using System.IO.Compression;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

// The elevated path differs only in the trusted principals (Administrators
// instead of the current user) and in its fixed directories, so an
// unelevated test can cover it without UAC.
public sealed class PseudoConsoleLoaderTests : IDisposable
{
    private readonly string root, source, target;
    private readonly SecurityIdentifier[] trusted = [.. PseudoConsoles.Administrators, WindowsIdentity.GetCurrent().User!];

    public PseudoConsoleLoaderTests()
    {
        var project = new DirectoryInfo(Environment.CurrentDirectory);
        while (project != null && !File.Exists(Path.Combine(project.FullName, "vendor/conpty/README.md"))) project = project.Parent;
        if (project == null) throw new InvalidOperationException("Run tests inside the worktree.");
        root = Path.Combine(project.FullName, "tmp", "conpty-loader-" + Guid.NewGuid().ToString("N"));
        source = Path.Combine(root, "bin");
        target = Path.Combine(root, "protected");
        Directory.CreateDirectory(source);
        var arch = RuntimeInformation.ProcessArchitecture == Architecture.Arm64 ? "arm64" : "x64";
        using var package = ZipFile.OpenRead(Path.Combine(project.FullName, $"vendor/conpty/microsoft.windows.console.conpty.{PseudoConsoles.Version}.nupkg"));
        package.GetEntry($"runtimes/win-{arch}/native/conpty.dll")!.ExtractToFile(Path.Combine(source, PseudoConsoles.Library));
        package.GetEntry($"build/native/runtimes/{arch}/OpenConsole.exe")!.ExtractToFile(Path.Combine(source, PseudoConsoles.Host));
    }

    public void Dispose()
    {
        // Unload this test's copy so its directory can go. Every Load took a reference.
        IntPtr module;
        while ((module = GetModuleHandleW(Copy(PseudoConsoles.Library))) != IntPtr.Zero) FreeLibrary(module);
        Directory.Delete(root, true);
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr GetModuleHandleW(string name);
    [DllImport("kernel32.dll")] private static extern bool FreeLibrary(IntPtr module);

    private string Copy(string name) => Path.Combine(target, $"ConPTY-{PseudoConsoles.Version}-{RuntimeInformation.ProcessArchitecture.ToString().ToLowerInvariant()}", name);

    [Fact]
    public async Task VerifiedCopyKeepsMouseReportingOnAPiStyleAlternateScreen()
    {
        var consoles = PseudoConsoles.Load(source, target, trusted);
        Assert.True(consoles.Bundled);
        Assert.True(File.Exists(Copy(PseudoConsoles.Host)));
        Assert.Contains("\x1b[?1000h", await AlternateScreen(consoles));
        // The inbox ConPTY drops the mode, which is the bug this avoids.
        Assert.DoesNotContain("\x1b[?1000h", await AlternateScreen(PseudoConsoles.Inbox));
    }

    [Fact]
    public void TamperedSourceFallsBackToKernel32()
    {
        var dll = Path.Combine(source, PseudoConsoles.Library);
        var bytes = File.ReadAllBytes(dll);
        bytes[^1] ^= 1;
        File.WriteAllBytes(dll, bytes);
        Assert.False(PseudoConsoles.Load(source, target, trusted).Bundled);
        Assert.False(File.Exists(Copy(PseudoConsoles.Library)));
    }

    [Fact]
    public void TamperedCopyIsReplacedWithTheVerifiedBytes()
    {
        Assert.True(PseudoConsoles.Load(source, target, trusted).Bundled);
        File.WriteAllBytes(Copy(PseudoConsoles.Host), [0x4d, 0x5a]);
        Assert.True(PseudoConsoles.Load(source, target, trusted).Bundled);
        Assert.Equal(File.ReadAllBytes(Path.Combine(source, PseudoConsoles.Host)), File.ReadAllBytes(Copy(PseudoConsoles.Host)));
    }

    [Fact]
    public void DirectoryOthersCanWriteFallsBackToKernel32()
    {
        // Created by this unelevated user, so Administrators alone don't own it.
        Directory.CreateDirectory(target);
        Assert.False(PseudoConsoles.Load(source, target, PseudoConsoles.Administrators).Bundled);
        var users = new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null);
        var security = new DirectorySecurity();
        security.AddAccessRule(new FileSystemAccessRule(users, FileSystemRights.Modify, AccessControlType.Allow));
        Assert.False(PseudoConsoles.WritableOnlyBy(security, trusted));
    }

    [Fact]
    public void CreatedDirectoriesAreWritableOnlyByTheTrustedPrincipals()
    {
        Assert.True(PseudoConsoles.Load(source, target, trusted).Bundled);
        var security = new DirectoryInfo(Path.GetDirectoryName(Copy(PseudoConsoles.Library))!).GetAccessControl();
        Assert.True(security.AreAccessRulesProtected);
        foreach (FileSystemAccessRule rule in security.GetAccessRules(true, true, typeof(SecurityIdentifier)))
            Assert.Contains((SecurityIdentifier)rule.IdentityReference, trusted);
    }

    // pi's full-screen renderer: alternate screen, then mouse reporting.
    private async Task<string> AlternateScreen(PseudoConsoles consoles)
    {
        var system = Environment.GetFolderPath(Environment.SpecialFolder.System);
        var windows = Path.GetDirectoryName(system)!;
        var env = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["SystemRoot"] = windows, ["WINDIR"] = windows, ["PATH"] = system,
            ["PSModulePath"] = Path.Combine(system, "WindowsPowerShell/v1.0/Modules"),
            ["USERPROFILE"] = root, ["APPDATA"] = root, ["LOCALAPPDATA"] = root, ["TEMP"] = root, ["TMP"] = root,
        };
        var launch = Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(new { type = "launch", cwd = root, env, cols = 80, rows = 24 }));
        using var terminal = new ConPty(launch, consoles,
            " -NoProfile -Command \"$e=[char]27; [Console]::Write($e+'[?1049h'+$e+'[?1000h'+$e+'[?1006hSFALT'); Start-Sleep 3\"");
        Assert.Equal(consoles.Bundled, terminal.Bundled);
        var output = new StringBuilder();
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        var pump = Task.Run(async () =>
        {
            var bytes = new byte[4096];
            while (true)
            {
                int count;
                try { count = await terminal.Output.ReadAsync(bytes, deadline.Token); } catch (IOException) { break; }
                if (count == 0) break;
                lock (output) output.Append(Encoding.UTF8.GetString(bytes, 0, count));
            }
        });
        try
        {
            while (!Snapshot(output).Contains("SFALT")) await Task.Delay(20, deadline.Token);
            return Snapshot(output);
        }
        finally { await terminal.Close(); }
    }

    private static string Snapshot(StringBuilder output) { lock (output) return output.ToString(); }
}
