using System.Text;
using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

// Validation only. Never construct a ConPty or run a shell, even non-elevated.
public sealed class IsolatedLaunchTests : IDisposable
{
    private readonly string root;
    private readonly Dictionary<string, string> env;
    public IsolatedLaunchTests()
    {
        var project = new DirectoryInfo(Environment.CurrentDirectory);
        while (project != null && !File.Exists(Path.Combine(project.FullName, "native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj"))) project = project.Parent;
        if (project == null) throw new InvalidOperationException("Run tests inside the worktree.");
        root = Path.Combine(project.FullName, "tmp", "isolated-helper-tests-space & ' " + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        var windows = Path.GetDirectoryName(IsolatedLaunch.SystemDirectory)!;
        env = new(StringComparer.OrdinalIgnoreCase)
        {
            ["SystemRoot"] = windows, ["WINDIR"] = windows, ["COMSPEC"] = Path.Combine(IsolatedLaunch.SystemDirectory, "cmd.exe"),
            ["PATH"] = IsolatedLaunch.SystemDirectory, ["PSModulePath"] = Path.Combine(IsolatedLaunch.SystemDirectory, "WindowsPowerShell/v1.0/Modules"),
            ["PATHEXT"] = ".COM;.EXE;.BAT;.CMD", ["TERM"] = "xterm-256color", ["COLORTERM"] = "truecolor",
        };
        foreach (var pair in IsolatedLaunch.Directories)
        {
            var directory = Path.Combine(root, pair.Value);
            Directory.CreateDirectory(directory);
            env.Add(pair.Key, directory);
        }
        foreach (var pair in IsolatedLaunch.Files) env.Add(pair.Key, Path.GetFullPath(Path.Combine(root, pair.Value)));
    }
    private JsonElement Launch(string type = "launch-isolated", int cols = 80, int rows = 24) => Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(new { type, isolationRoot = root, cwd = root, env, cols, rows }));
    private static JsonElement Change(JsonElement launch, string name, object? value)
    {
        var fields = launch.EnumerateObject().ToDictionary(p => p.Name, p => (object?)p.Value);
        fields[name] = value;
        return Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(fields));
    }
    private static void Reject(JsonElement value) => Assert.ThrowsAny<Exception>(() => ConPty.ValidateLaunch(value, isolated: true));

    [Fact]
    public void CapabilityIsExactTypedV1AndCommandLineNeverAcceptsPayloadArguments()
    {
        using var doc = JsonDocument.Parse(IsolatedLaunch.Probe);
        Wire.Fields(doc.RootElement, "protocol", "version", "shell", "noProfile");
        Assert.Equal("shellfox-elevated-isolated", doc.RootElement.GetProperty("protocol").GetString());
        Assert.Equal(1, doc.RootElement.GetProperty("version").GetInt32());
        Assert.Equal("windows-powershell", doc.RootElement.GetProperty("shell").GetString());
        Assert.True(doc.RootElement.GetProperty("noProfile").GetBoolean());
        var command = IsolatedLaunch.CommandLine(true);
        Assert.Equal($"\"{IsolatedLaunch.Executable}\" -NoLogo -NoProfile -NoExit -Command \"try {{ $env:PSModulePath = Join-Path $PSHOME 'Modules'; if ((Get-Module PSReadLine -ErrorAction Stop) -or (Get-Module -ListAvailable PSReadLine -ErrorAction Stop)) {{ Import-Module PSReadLine -ErrorAction Stop; Set-PSReadLineOption -HistorySavePath $env:PSREADLINE_HISTORYFILE -ErrorAction Stop }} }} catch {{ exit 1 }}\"", command);
        Assert.DoesNotContain(root, command);
        Assert.Equal($"\"{IsolatedLaunch.Executable}\" -NoLogo", IsolatedLaunch.CommandLine(false));
        Assert.True(Path.IsPathFullyQualified(IsolatedLaunch.Executable));
    }
    [Fact]
    public void LoadedPsReadLineCannotSkipTheExactHistoryOverrideWhenDiscoveryIsRestricted()
    {
        var command = IsolatedLaunch.CommandLine(true);
        Assert.Contains("if ((Get-Module PSReadLine -ErrorAction Stop) -or (Get-Module -ListAvailable PSReadLine -ErrorAction Stop))", command);
        Assert.Contains("Import-Module PSReadLine -ErrorAction Stop; Set-PSReadLineOption -HistorySavePath $env:PSREADLINE_HISTORYFILE -ErrorAction Stop", command);
        Assert.DoesNotContain(root, command); // Environment data never becomes script text.
        Assert.DoesNotContain("PSReadLine", IsolatedLaunch.CommandLine(false));
        Assert.DoesNotContain("-NoProfile", IsolatedLaunch.CommandLine(false));
    }
    [Fact]
    public void CuratedEnvironmentAndOwnedStateAreAcceptedWithoutStartingAnything()
    {
        File.WriteAllText(env["GIT_CONFIG_GLOBAL"], "owned fixture");
        ConPty.ValidateLaunch(Launch(), isolated: true);
        var block = ConPty.BuildEnvironment(Launch().GetProperty("env"));
        Assert.Contains("HOME=" + Path.Combine(root, "home") + "\0", block);
        Assert.EndsWith("\0\0", block);
        Assert.Equal("owned fixture", File.ReadAllText(env["GIT_CONFIG_GLOBAL"]));
        ConPty.ValidateLaunch(Launch(cols: 2, rows: 500), isolated: true);
    }
    [Theory]
    [InlineData("executable")]
    [InlineData("args")]
    [InlineData("command")]
    [InlineData("script")]
    [InlineData("isolated")]
    public void ExtraLaunchFieldsCannotInjectCommandsOrMode(string name) => Reject(Change(Launch(), name, "evil"));
    [Fact]
    public void ModesCannotBeCrossedOrDowngraded()
    {
        Reject(Launch(type: "launch"));
        Assert.Throws<InvalidDataException>(() => ConPty.ValidateLaunch(Launch()));
        var legacy = Wire.Parse(JsonSerializer.SerializeToUtf8Bytes(new { type = "launch", cwd = root, env, cols = 80, rows = 24 }));
        ConPty.ValidateLaunch(legacy);
        Reject(legacy);
    }
    [Theory]
    [InlineData("HOME")]
    [InlineData("USERPROFILE")]
    [InlineData("APPDATA")]
    [InlineData("TEMP")]
    [InlineData("GIT_CONFIG_GLOBAL")]
    [InlineData("PI_CODING_AGENT_DIR")]
    [InlineData("PSModulePath")]
    [InlineData("SystemRoot")]
    public void AccountOrOtherRootStateCannotReplacePrivateState(string key)
    {
        env[key] = IsolatedLaunch.SystemDirectory;
        Reject(Launch());
    }
    [Theory]
    [InlineData("DOTNET_STARTUP_HOOKS")]
    [InlineData("cOr_ENABLE_PROFILING")]
    [InlineData("NODE_OPTIONS")]
    [InlineData("ENV")]
    [InlineData("SSH_AUTH_SOCK")]
    [InlineData("CUSTOM_VALUE")]
    [InlineData("HOMEPATH")]
    public void EnvironmentIsClosedNotJustRuntimePrefixFiltered(string key)
    {
        env[key] = "blocked";
        Reject(Launch());
    }
    [Fact]
    public void MissingEnvironmentAndCaseInsensitiveDuplicatesFailClosed()
    {
        env.Remove("HOME");
        Reject(Launch());
        env["HOME"] = Path.Combine(root, "home");
        var raw = Launch().GetRawText().Replace("\"env\":{", "\"env\":{\"home\":\"bad\",");
        Reject(Wire.Parse(Encoding.UTF8.GetBytes(raw)));
        raw = Launch().GetRawText().Replace("\"type\":", "\"type\":\"launch-isolated\",\"type\":");
        Reject(Wire.Parse(Encoding.UTF8.GetBytes(raw)));
    }
    [Theory]
    [InlineData(".")]
    [InlineData("")]
    [InlineData("C:relative")]
    [InlineData("\\\\server\\share\\home")]
    [InlineData("\\\\?\\C:\\home")]
    [InlineData("C:\\home:stream")]
    [InlineData("C:\\home\\..\\other")]
    [InlineData("C:\\home.\\other")]
    public void AmbiguousRootPathsAreRejected(string path) => Reject(Change(Launch(), "isolationRoot", path));
    [Fact]
    public void DriveRootMissingDirectoryAndInvalidCwdAreRejected()
    {
        Reject(Change(Launch(), "isolationRoot", Path.GetPathRoot(root)));
        Reject(Change(Launch(), "cwd", Path.Combine(root, "missing")));
        Reject(Change(Launch(), "cwd", "relative"));
        Directory.Delete(env["HOME"]);
        Reject(Launch());
    }
    [Theory]
    [InlineData("")]
    [InlineData(".")]
    [InlineData(";C:\\Windows")]
    [InlineData("C:\\Windows;")]
    [InlineData("C:\\Windows;;C:\\Windows")]
    public void PathCannotImplicitlySearchCwd(string path)
    {
        env["PATH"] = path;
        Reject(Launch());
    }
    [Theory]
    [InlineData(1, 24)]
    [InlineData(80, 501)]
    public void IsolatedModeRetainsDimensionLimits(int cols, int rows) => Reject(Launch(cols: cols, rows: rows));
    [Fact]
    public void EnvironmentBlockSizeAndNullValidationStillApply()
    {
        env["PATH"] = new string('x', 32767);
        Reject(Launch());
        env["PATH"] = "a\0b";
        Reject(Launch());
    }
    [Fact]
    public void StateDirectoryRedirectAndFileHardlinkAreRejected()
    {
        var home = env["HOME"];
        Directory.Delete(home);
        var target = Path.Combine(root, "owned-other-home");
        Directory.CreateDirectory(target);
        // Junction creation needs no symlink privilege and touches fixtures only.
        Junction.Create(home, target);
        try { Reject(Launch()); }
        finally { Directory.Delete(home); }
        Directory.CreateDirectory(home);
        var source = Path.Combine(root, "owned-source");
        File.WriteAllText(source, "owned fixture");
        Assert.True(CreateHardLink(env["GIT_CONFIG_GLOBAL"], source, IntPtr.Zero));
        Reject(Launch());
        Assert.Equal("owned fixture", File.ReadAllText(source));
    }
    [Fact]
    public void StateFilesCannotBeDirectories()
    {
        Directory.CreateDirectory(env["GIT_CONFIG_GLOBAL"]);
        Reject(Launch());
    }
    public void Dispose() => Directory.Delete(root, recursive: true);

    [System.Runtime.InteropServices.DefaultDllImportSearchPaths(System.Runtime.InteropServices.DllImportSearchPath.System32)]
    [System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateHardLink(string name, string existing, IntPtr security);
}
