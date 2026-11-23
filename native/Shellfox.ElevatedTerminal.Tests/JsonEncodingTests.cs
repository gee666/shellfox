using System.Globalization;
using System.Text;
using System.Text.Json;
using Shellfox.ElevatedTerminal;
using Xunit;

namespace Shellfox.ElevatedTerminal.Tests;

public sealed class JsonEncodingTests
{
    [Fact]
    public void ExplicitDomFramesMatchPreviousReflectionSerializerBytes()
    {
        var env = new Dictionary<string, string> { ["HOME"] = "C:\\owned\\雪🙂", ["CUSTOM"] = "'\"<&+\n" };
        var corpus = new (object Previous, object Current)[]
        {
            (new { type = "ready", pid = 123, birth = "134355986344672889" }, Wire.Object(("type", "ready"), ("pid", 123), ("birth", "134355986344672889"))),
            (new { type = "data", data = "+/==" }, Wire.Object(("type", "data"), ("data", "+/=="))),
            (new { type = "ack", id = 9007199254740991L }, Wire.Object(("type", "ack"), ("id", 9007199254740991L))),
            (new { type = "exit", exitCode = -1 }, Wire.Object(("type", "exit"), ("exitCode", -1))),
            (new { type = "error", code = "failed", message = "'雪🙂\n\u0001<&+" }, Wire.Object(("type", "error"), ("code", "failed"), ("message", "'雪🙂\n\u0001<&+"))),
            (new { type = "close" }, Wire.Object(("type", "close"))),
            (new { type = "resize", cols = 80, rows = 24 }, Wire.Object(("type", "resize"), ("cols", 80), ("rows", 24))),
            (new { type = "write", id = 1, data = "'雪🙂" }, Wire.Object(("type", "write"), ("id", 1), ("data", "'雪🙂"))),
            (new { type = "launch", cwd = "C:\\owned", env, cols = 80, rows = 24 }, Wire.Object(("type", "launch"), ("cwd", "C:\\owned"), ("env", Wire.Environment(env)), ("cols", 80), ("rows", 24))),
        };
        var original = CultureInfo.CurrentCulture;
        try
        {
            foreach (var culture in new[] { "", "tr-TR", "ar-SA" })
            {
                CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo(culture);
                foreach (var pair in corpus) Assert.Equal(JsonSerializer.SerializeToUtf8Bytes(pair.Previous), Wire.Bytes(pair.Current));
            }
        }
        finally { CultureInfo.CurrentCulture = original; }
    }
    [Fact]
    public async Task RawForwardingRetainsApostrophesAndLengthPrefixWithoutReencoding()
    {
        var bytes = Encoding.UTF8.GetBytes("{\"type\":\"write\",\"id\":1,\"data\":\"" + new string('\'', 50000) + "\"}");
        using var frame = new MemoryStream();
        await Wire.Send(frame, Wire.Parse(bytes), false, CancellationToken.None);
        Assert.Equal(bytes.Length + 4, frame.Length);
        Assert.Equal(bytes, frame.ToArray()[4..]);
    }
    [Fact]
    public void JournalBytesRemainCompatibleWithExistingVersionOneFiles()
    {
        var id = Guid.NewGuid().ToString("D");
        var old = JsonSerializer.SerializeToUtf8Bytes(new { version = 1, scheme = id, ac = 1u, dc = 2u, acOwned = true, dcOwned = false, refreshPending = true });
        Assert.Equal(old, Wire.Bytes(Wire.Object(("version", 1), ("scheme", id), ("ac", 1u), ("dc", 2u), ("acOwned", true), ("dcOwned", false), ("refreshPending", true))));
    }
    [Fact]
    public void ArbitraryRuntimeObjectsCannotReachReflectionSerialization() =>
        Assert.Throws<InvalidDataException>(() => Wire.Bytes(new { type = "not-a-dom-frame" }));
}
