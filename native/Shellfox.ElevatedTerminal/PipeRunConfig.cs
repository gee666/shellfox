using System.Text;
using System.Text.Json;

namespace Shellfox.ElevatedTerminal;

internal sealed record PipeRunConfig(string Executable, string[] Args, string Cwd, Dictionary<string, string> Env)
{
    internal const int MaxBytes = 1024 * 1024;

    internal static PipeRunConfig Load(string file)
    {
        if (!Path.IsPathFullyQualified(file)) throw new InvalidDataException("Config path must be absolute.");
        try
        {
            if ((File.GetAttributes(file) & (FileAttributes.ReparsePoint | FileAttributes.Directory)) != 0)
                throw new InvalidDataException("Config must be a regular file.");
            using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.None);
            if (stream.Length is < 1 or > MaxBytes) throw new InvalidDataException("Invalid config size.");
            var bytes = new byte[(int)stream.Length];
            stream.ReadExactly(bytes);
            return Parse(bytes);
        }
        finally { File.Delete(file); }
    }

    internal static PipeRunConfig Parse(byte[] bytes)
    {
        if (bytes.Length is < 1 or > MaxBytes) throw new InvalidDataException("Invalid config size.");
        using var document = JsonDocument.Parse(bytes, new JsonDocumentOptions { MaxDepth = 4 });
        var root = document.RootElement;
        if (root.ValueKind != JsonValueKind.Object) throw new InvalidDataException("Invalid launch config.");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var item in root.EnumerateObject())
            if (!fields.Add(item.Name) || item.Name is not ("executable" or "args" or "cwd" or "env"))
                throw new InvalidDataException("Unexpected launch config field.");
        if (fields.Count != 4) throw new InvalidDataException("Missing launch config field.");
        var executable = AbsolutePath(root.GetProperty("executable"));
        if (!new[] { ".exe", ".com" }.Contains(Path.GetExtension(executable), StringComparer.OrdinalIgnoreCase))
            throw new InvalidDataException("Expected a native executable, not a shell script.");
        var cwd = AbsolutePath(root.GetProperty("cwd"));
        var args = root.GetProperty("args");
        if (args.ValueKind != JsonValueKind.Array || args.GetArrayLength() > 4096) throw new InvalidDataException("Invalid launch arguments.");
        var arguments = args.EnumerateArray().Select(Text).ToArray();
        var environment = root.GetProperty("env");
        if (environment.ValueKind != JsonValueKind.Object) throw new InvalidDataException("Invalid environment.");
        var env = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var units = 1;
        foreach (var item in environment.EnumerateObject())
        {
            var value = Text(item.Value);
            if (item.Name.Length is < 1 or > 32767 || item.Name.Any(c => c < 32 || c == '=') || !env.TryAdd(item.Name, value))
                throw new InvalidDataException("Invalid environment entry.");
            units += item.Name.Length + value.Length + 2;
            if (env.Count > 1024 || units > 32767) throw new InvalidDataException("Oversized environment.");
        }
        var config = new PipeRunConfig(executable, arguments, cwd, env);
        if (config.CommandLine().Length >= 32767) throw new InvalidDataException("Oversized command line.");
        return config;
    }

    private static string Text(JsonElement value)
    {
        if (value.ValueKind != JsonValueKind.String) throw new InvalidDataException("Expected string.");
        var text = value.GetString()!;
        if (text.Length > 32767 || text.Contains('\0')) throw new InvalidDataException("Invalid string.");
        return text;
    }
    private static string AbsolutePath(JsonElement value)
    {
        var text = Text(value);
        if (!Path.IsPathFullyQualified(text) || text.Any(c => c < 32 || c == '"')) throw new InvalidDataException("Invalid launch path.");
        return text;
    }
    internal string CommandLine() => string.Join(" ", new[] { Executable }.Concat(Args).Select(Quote));
    internal string EnvironmentBlock() => string.Join("\0", Env.OrderBy(p => p.Key, StringComparer.OrdinalIgnoreCase).Select(p => p.Key + "=" + p.Value)) + "\0\0";

    // CommandLineToArgvW/CRT quoting. Shell metacharacters remain argument data.
    internal static string Quote(string text)
    {
        var result = new StringBuilder("\"");
        var slashes = 0;
        foreach (var c in text)
        {
            if (c == '\\') { slashes++; continue; }
            result.Append('\\', c == '"' ? slashes * 2 + 1 : slashes);
            result.Append(c); slashes = 0;
        }
        return result.Append('\\', slashes * 2).Append('"').ToString();
    }
}
