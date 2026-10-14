using System.Buffers.Binary;
using System.Text;
using System.Text.Json;

namespace Shellfox.ElevatedTerminal;

// Both boundaries reject oversized frames before allocating the payload.
internal static class Wire
{
    internal const int MaxFrame = 262144;
    internal static readonly UTF8Encoding Utf8 = new(false, true);
    internal static async Task<JsonElement?> ReadFrame(Stream stream, CancellationToken cancel)
    {
        var header = new byte[4];
        var first = await stream.ReadAsync(header.AsMemory(0, 1), cancel);
        if (first == 0) return null;
        await stream.ReadExactlyAsync(header.AsMemory(1), cancel);
        var size = BinaryPrimitives.ReadInt32LittleEndian(header);
        if (size < 2 || size > MaxFrame) throw new InvalidDataException("Invalid frame length.");
        var bytes = new byte[size];
        await stream.ReadExactlyAsync(bytes, cancel);
        return Parse(bytes);
    }
    internal static JsonElement Parse(byte[] bytes)
    {
        if (bytes.Length > MaxFrame) throw new InvalidDataException("Oversized frame.");
        using var doc = JsonDocument.Parse(Utf8.GetString(bytes), new JsonDocumentOptions { MaxDepth = 8 });
        if (doc.RootElement.ValueKind != JsonValueKind.Object) throw new InvalidDataException("Expected an object.");
        return doc.RootElement.Clone();
    }
    internal static async Task<JsonElement?> ReadLine(Stream stream, CancellationToken cancel)
    {
        // Stdin is buffered by the caller. No unbounded StreamReader.ReadLine.
        using var bytes = new MemoryStream();
        var single = new byte[1];
        while (await stream.ReadAsync(single, cancel) != 0)
        {
            if (single[0] == 10) return Parse(bytes.ToArray());
            if (bytes.Length >= MaxFrame) throw new InvalidDataException("Oversized line.");
            bytes.WriteByte(single[0]);
        }
        if (bytes.Length != 0) throw new EndOfStreamException("Incomplete line.");
        return null;
    }
    internal static async Task Send(Stream stream, object value, bool line, CancellationToken cancel)
    {
        // Forward validated JSON without re-escaping it. The default .NET
        // encoder turns each apostrophe into six bytes, unlike JSON.stringify,
        // and can inflate an accepted input frame beyond the wire limit.
        var bytes = value is JsonElement json ? Utf8.GetBytes(json.GetRawText()) : JsonSerializer.SerializeToUtf8Bytes(value);
        if (bytes.Length > MaxFrame) throw new InvalidDataException("Oversized message.");
        if (!line)
        {
            var header = new byte[4]; BinaryPrimitives.WriteInt32LittleEndian(header, bytes.Length);
            await stream.WriteAsync(header, cancel);
        }
        await stream.WriteAsync(bytes, cancel);
        if (line) await stream.WriteAsync(new byte[] { 10 }, cancel);
        await stream.FlushAsync(cancel);
    }
    internal static string Type(JsonElement value) => value.GetProperty("type").GetString() ?? "";
    internal static void Fields(JsonElement value, params string[] fields)
    {
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var item in value.EnumerateObject())
            if (!fields.Contains(item.Name) || !seen.Add(item.Name)) throw new InvalidDataException("Unexpected or duplicate field.");
        if (seen.Count != fields.Length) throw new InvalidDataException("Missing field.");
    }
}
