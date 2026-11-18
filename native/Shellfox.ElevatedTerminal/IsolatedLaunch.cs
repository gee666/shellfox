using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;

namespace Shellfox.ElevatedTerminal;

// This is a profile-free shell launch, not an account or filesystem sandbox.
// Rust owns and prepares the root. The helper never creates state directories.
internal static class IsolatedLaunch
{
    internal const string Probe = "{\"protocol\":\"shellfox-elevated-isolated\",\"version\":1,\"shell\":\"windows-powershell\",\"noProfile\":true}";
    internal static string SystemDirectory => Environment.GetFolderPath(Environment.SpecialFolder.System);
    internal static string Executable => Path.Combine(SystemDirectory, "WindowsPowerShell", "v1.0", "powershell.exe");
    // PSReadLine does not honor a history-path environment variable by itself.
    // This constant bootstrap reads only validated data, never payload code, and
    // exits if history redirection fails rather than using account history.
    // Restricted PSModulePath can hide an already-loaded module from discovery.
    private const string Bootstrap = "try { $env:PSModulePath = Join-Path $PSHOME 'Modules'; if ((Get-Module PSReadLine -ErrorAction Stop) -or (Get-Module -ListAvailable PSReadLine -ErrorAction Stop)) { Import-Module PSReadLine -ErrorAction Stop; Set-PSReadLineOption -HistorySavePath $env:PSREADLINE_HISTORYFILE -ErrorAction Stop } } catch { exit 1 }";
    internal static string CommandLine(bool isolated) => $"\"{Executable}\" -NoLogo" +
        (isolated ? " -NoProfile -NoExit -Command \"" + Bootstrap + "\"" : "");

    // Closed vocabulary matches the Rust experiment's environment_paths builder.
    // Values for all writable state are fixed relative to isolationRoot.
    internal static readonly IReadOnlyDictionary<string, string> Directories = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
    {
        ["HOME"] = "home", ["USERPROFILE"] = "home", ["ZDOTDIR"] = "home",
        ["APPDATA"] = "config", ["XDG_CONFIG_HOME"] = "config",
        ["LOCALAPPDATA"] = "data", ["XDG_DATA_HOME"] = "data",
        ["XDG_CACHE_HOME"] = "cache", ["npm_config_cache"] = "cache",
        ["XDG_STATE_HOME"] = "state", ["TMP"] = "temp", ["TEMP"] = "temp", ["TMPDIR"] = "temp",
        ["PI_CODING_AGENT_DIR"] = "pi-agent", ["PI_CODING_AGENT_SESSION_DIR"] = "pi-sessions",
        ["npm_config_prefix"] = "npm-prefix", ["PNPM_HOME"] = "pnpm-home",
    };
    internal static readonly IReadOnlyDictionary<string, string> Files = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
    {
        ["HISTFILE"] = "state/shell-history", ["PSREADLINE_HISTORYFILE"] = "state/powershell-history",
        ["npm_config_userconfig"] = "config/npmrc", ["npm_config_globalconfig"] = "config/npm-globalrc",
        ["GIT_CONFIG_GLOBAL"] = "config/gitconfig", ["GIT_CONFIG_SYSTEM"] = "config/gitconfig-system",
    };

    internal static void Validate(JsonElement launch)
    {
        var root = LocalPath(launch.GetProperty("isolationRoot").GetString());
        DirectoryPath(root);
        if (Same(root, Path.GetPathRoot(root)!)) throw new InvalidDataException("Isolation root cannot be a drive root.");
        DirectoryPath(LocalPath(launch.GetProperty("cwd").GetString()));
        var env = launch.GetProperty("env").EnumerateObject().ToDictionary(p => p.Name, p => p.Value.GetString()!, StringComparer.OrdinalIgnoreCase);
        var expected = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["SystemRoot"] = Path.GetDirectoryName(SystemDirectory)!, ["WINDIR"] = Path.GetDirectoryName(SystemDirectory)!,
            ["COMSPEC"] = Path.Combine(SystemDirectory, "cmd.exe"),
            ["PATHEXT"] = ".COM;.EXE;.BAT;.CMD", ["TERM"] = "xterm-256color", ["COLORTERM"] = "truecolor",
            ["PSModulePath"] = Path.Combine(SystemDirectory, "WindowsPowerShell", "v1.0", "Modules"),
        };
        foreach (var pair in Directories)
        {
            var directory = Path.Combine(root, pair.Value);
            DirectoryPath(directory);
            expected.Add(pair.Key, directory);
        }
        foreach (var pair in Files)
        {
            var file = Path.GetFullPath(Path.Combine(root, pair.Value));
            StateFile(file);
            expected.Add(pair.Key, file);
        }
        if (env.Count != expected.Count + 1 || !env.TryGetValue("PATH", out var search))
            throw new InvalidDataException("Invalid isolated environment fields.");
        foreach (var pair in expected)
        {
            if (!env.TryGetValue(pair.Key, out var value)) throw new InvalidDataException("Missing isolated environment field.");
            if (pair.Key is "PATHEXT" or "TERM" or "COLORTERM")
            {
                if (value != pair.Value) throw new InvalidDataException("Invalid isolated environment value.");
            }
            else if (!Same(LocalPath(value), pair.Value)) throw new InvalidDataException("Invalid isolated state path.");
        }
        // InstalledTools may opt into existing executable directories, but never
        // relative or empty entries that implicitly search the working directory.
        if (search.Length == 0) throw new InvalidDataException("Empty isolated PATH.");
        foreach (var directory in search.Split(';')) DirectoryPath(LocalPath(directory));
    }

    private static bool Same(string left, string right) => string.Equals(left, right, StringComparison.OrdinalIgnoreCase);
    private static string LocalPath(string? value)
    {
        // Ordinary drive paths only. Device/UNC/ADS and dot components need a
        // separate policy, not accidental Windows path normalization.
        if (value is null || value.Length is < 3 or > 32767 || !char.IsAsciiLetter(value[0]) || value[1] != ':' || value[2] is not ('\\' or '/') ||
            value.Skip(2).Any(c => c < 32 || c is ':' or '"' or '*' or '?' or '<' or '>' or '|'))
            throw new InvalidDataException("Expected an absolute local path.");
        foreach (var part in value[3..].Split(new[] { '/', '\\' }, StringSplitOptions.RemoveEmptyEntries))
            if (part is "." or ".." || part.EndsWith('.') || part.EndsWith(' ')) throw new InvalidDataException("Ambiguous local path.");
        return Path.TrimEndingDirectorySeparator(Path.GetFullPath(value));
    }

    private static void DirectoryPath(string path)
    {
        // Check ancestors too. A root below a junction is not a private root.
        for (var current = path; current != null; current = Path.GetDirectoryName(current))
        {
            var attributes = File.GetAttributes(current);
            if ((attributes & FileAttributes.ReparsePoint) != 0 || (attributes & FileAttributes.Directory) == 0)
                throw new InvalidDataException("Isolated directory is missing or redirected.");
        }
    }

    private static void StateFile(string path)
    {
        FileAttributes attributes;
        try { attributes = File.GetAttributes(path); }
        catch (FileNotFoundException) { return; }
        if ((attributes & (FileAttributes.ReparsePoint | FileAttributes.Directory)) != 0)
            throw new InvalidDataException("Isolated state file is redirected.");
        using var file = File.OpenHandle(path, FileMode.Open, FileAccess.Read, FileShare.Read);
        if (!GetFileInformationByHandle(file, out var info)) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (info.Links != 1 || (info.Attributes & (uint)(FileAttributes.ReparsePoint | FileAttributes.Directory)) != 0)
            throw new InvalidDataException("Isolated state file has shared aliases.");
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct FileInfo
    {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetFileInformationByHandle(SafeFileHandle file, out FileInfo info);
}
