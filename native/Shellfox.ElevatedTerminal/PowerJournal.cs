using System.Buffers.Binary;
using System.Security.Cryptography;

namespace Shellfox.ElevatedTerminal;

// Bounded append-only snapshots. A flushed intent always precedes a setting
// write. Torn tails recover the previous full snapshot, which is conservative.
internal sealed class PowerJournal : IPowerJournal, IDisposable
{
    private const int Maximum = 1024 * 1024, FrameMaximum = 4096;
    private readonly PowerFiles files;
    private FileStream? file;
    private long validLength;
    public PowerSnapshot? Current { get; private set; }
    internal PowerJournal(PowerFiles files)
    {
        this.files = files;
        try { file = files.Open("rollback.log"); Load(); }
        catch { file?.Dispose(); throw; }
    }
    private void Load()
    {
        var stream = file!;
        if (stream.Length > Maximum) throw new InvalidDataException("Oversized power journal.");
        while (stream.Position < stream.Length)
        {
            var header = new byte[4];
            if (stream.Read(header) != 4) break;
            var size = BinaryPrimitives.ReadInt32LittleEndian(header);
            if (size is < 2 or > FrameMaximum || stream.Length - stream.Position < size + 32) break;
            var bytes = new byte[size]; var hash = new byte[32];
            stream.ReadExactly(bytes); stream.ReadExactly(hash);
            if (!CryptographicOperations.FixedTimeEquals(hash, SHA256.HashData(bytes)))
                throw new InvalidDataException("Corrupt power journal record.");
            var value = Wire.Parse(bytes);
            Wire.Fields(value, "version", "scheme", "ac", "dc", "acOwned", "dcOwned", "refreshPending");
            if (value.GetProperty("version").GetInt32() != 1 ||
                !Guid.TryParseExact(value.GetProperty("scheme").GetString(), "D", out var scheme) || scheme == Guid.Empty)
                throw new InvalidDataException("Invalid power journal identity.");
            var ac = value.GetProperty("ac").GetUInt32(); var dc = value.GetProperty("dc").GetUInt32();
            var acOwned = value.GetProperty("acOwned").GetBoolean(); var dcOwned = value.GetProperty("dcOwned").GetBoolean();
            if (ac > 3 || dc > 3 || acOwned && ac == 0 || dcOwned && dc == 0) throw new InvalidDataException("Invalid power journal values.");
            if (Current is { } previous && (previous.Scheme != scheme || previous.Ac != ac || previous.Dc != dc))
                throw new InvalidDataException("Power journal identity changed.");
            Current = new(scheme, ac, dc, acOwned, dcOwned, value.GetProperty("refreshPending").GetBoolean());
            validLength = stream.Position;
        }
        if (stream.Length != 0 && Current == null) throw new InvalidDataException("Unrecoverable power journal.");
    }
    public void Save(PowerSnapshot value)
    {
        if (Current == value) return;
        if (Current is { } previous && (previous.Scheme != value.Scheme || previous.Ac != value.Ac || previous.Dc != value.Dc))
            throw new InvalidDataException("Power journal identity changed.");
        if (value.Scheme == Guid.Empty || value.Ac > 3 || value.Dc > 3 || value.AcOwned && value.Ac == 0 || value.DcOwned && value.Dc == 0)
            throw new InvalidDataException("Invalid power snapshot.");
        var bytes = Wire.Bytes(Wire.Object(
            ("version", 1), ("scheme", value.Scheme.ToString("D")), ("ac", value.Ac), ("dc", value.Dc),
            ("acOwned", value.AcOwned), ("dcOwned", value.DcOwned), ("refreshPending", value.RefreshPending)));
        if (validLength + bytes.Length + 36 > Maximum) throw new InvalidDataException("Power journal full.");
        file ??= files.Open("rollback.log");
        file.SetLength(validLength); // Remove only the uncommitted/torn tail.
        file.Position = validLength;
        var header = new byte[4]; BinaryPrimitives.WriteInt32LittleEndian(header, bytes.Length);
        file.Write(header); file.Write(bytes); file.Write(SHA256.HashData(bytes));
        file.Flush(flushToDisk: true);
        validLength = file.Position;
        Current = value;
    }
    public void Clear()
    {
        if (file != null)
        {
            PowerFiles.DeleteOnClose(file);
            file.Dispose(); file = null;
        }
        Current = null; validLength = 0;
    }
    public void Dispose() { file?.Dispose(); file = null; } // Never erase a failed rollback.
}
