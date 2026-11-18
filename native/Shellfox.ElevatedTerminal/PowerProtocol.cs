using System.Globalization;
using System.Text.Json;

namespace Shellfox.ElevatedTerminal;

internal sealed record PowerInit(string Root, nuint OwnerHandle, long OwnerBirth, string Nonce);
internal static class PowerProtocol
{
    internal static string Probe => Wire.Utf8.GetString(Wire.Bytes(Wire.Object(
        ("protocol", "shellfox-power-guardian"), ("version", 1), ("display", true), ("system", true),
        ("awayMode", true), ("idleMouse", true), ("lidRollback", true))));
    internal static PowerInit Init(JsonElement value)
    {
        Wire.Fields(value, "type", "version", "root", "ownerHandle", "ownerBirth", "nonce");
        if (Wire.Type(value) != "power-init" || value.GetProperty("version").GetInt32() != 1)
            throw new InvalidDataException("Invalid power protocol.");
        var nonce = value.GetProperty("nonce").GetString() ?? "";
        if (nonce.Length != 32 || nonce.Any(c => !char.IsAsciiHexDigitLower(c))) throw new InvalidDataException("Invalid power nonce.");
        var handle = Decimal(value.GetProperty("ownerHandle"));
        var birth = Decimal(value.GetProperty("ownerBirth"));
        if (handle == 0 || handle > (ulong)nuint.MaxValue || birth == 0 || birth > long.MaxValue)
            throw new InvalidDataException("Invalid power owner.");
        return new(PowerFiles.LocalPath(value.GetProperty("root").GetString()), (nuint)handle, (long)birth, nonce);
    }
    private static ulong Decimal(JsonElement value)
    {
        var text = value.GetString() ?? "";
        if (text.Length is < 1 or > 20 || text.Any(c => c is < '0' or > '9') ||
            !ulong.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var result))
            throw new InvalidDataException("Invalid owner identity.");
        return result;
    }
    internal static string Command(JsonElement value, string nonce, bool ready)
    {
        var type = Wire.Type(value);
        if (type == "power-go" && ready)
        {
            Wire.Fields(value, "type", "nonce", "action");
            var action = value.GetProperty("action").GetString();
            if (value.GetProperty("nonce").GetString() != nonce || action is not ("enable" or "recover"))
                throw new InvalidDataException("Invalid power GO.");
            return action;
        }
        Wire.Fields(value, "type", "nonce");
        if (type != "power-stop" || value.GetProperty("nonce").GetString() != nonce)
            throw new InvalidDataException("Invalid power command.");
        return "stop";
    }
}
